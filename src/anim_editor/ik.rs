//! 1:1 Inverse Kinematics solver and hierarchy utilities for RusTudio's native
//! `AnimationClipEditor` (matching `Util/RigIK` and `Util/IKControlManager`).

use std::collections::{HashMap, HashSet};

use glam::{Mat3, Quat, Vec3};
use rbx_dom_weak::types::{Ref, Variant};
use rustudio_core::doc::Document;
use rustudio_core::math::{cf_mul, Xform};

use crate::anim_editor::constants::{
    part1_to_joint, part_ik_constraint, IkConstraintConfig, IkConstraintKind,
};
use crate::anim_editor::model::RotationOrder;
use crate::anim_editor::state::{EditorState, IkMode, IkState, RigCache, RigTypeKind};

/// Maximum reach fraction for unreachable targets (matches Studio IKControl behavior).
const MAX_REACH_FRACTION: f32 = 0.9913;

/// Exact 15-joint R15 muscle rest pose pre-bends (`IKControlManager` `MUSCLE_REST_POSE`),
/// stored as `(joint_name, [rx, ry, rz])` radians in `CFrame.fromOrientation` (`YXZ`) order.
pub const MUSCLE_REST_POSE: &[(&str, [f32; 3])] = &[
    ("Root", [0.0, 0.0, 0.0]),
    ("Waist", [0.0, 0.0, 0.0]),
    ("Neck", [0.0, 0.0, 0.0]),
    ("LeftWrist", [0.0, 0.0, 0.0]),
    ("RightWrist", [0.0, 0.0, 0.0]),
    ("LeftAnkle", [0.0, 0.0, 0.0]),
    ("RightAnkle", [0.0, 0.0, 0.0]),
    ("RightShoulder", [0.408, 0.326, 0.48]),
    ("RightElbow", [1.165, -0.111, -0.342]),
    ("LeftShoulder", [0.408, -0.326, -0.48]),
    ("LeftElbow", [1.165, 0.111, 0.342]),
    ("LeftKnee", [-1.398, -0.098, 0.119]),
    ("LeftHip", [0.561, -0.018, 0.031]),
    ("RightKnee", [-1.399, 0.055, -0.066]),
    ("RightHip", [0.561, 0.018, -0.031]),
];

/// Output of a single IK drag solve step.
#[derive(Debug, Clone)]
pub struct IkSolveResult {
    /// Solved local `Motor6D.Transform` values keyed by `Part1` name in chain order.
    pub joint_transforms: Vec<(String, Xform)>,
    /// Realized world `Xform` of the dragged end-effector part after constraints.
    pub actual_effector_world: Xform,
}

#[derive(Debug, Clone)]
struct ChainLink {
    part0_name: String,
    part1_name: String,
    part0_ref: Option<Ref>,
    part1_ref: Option<Ref>,
    c0: Xform,
    c1: Xform,
    transform: Xform,
}

#[derive(Debug, Clone, Copy)]
enum JointLimit {
    BallSocket {
        a0: Xform,
        a1: Xform,
        limits: bool,
        upper: f32,
        twist_limits: bool,
        twist_lo: f32,
        twist_hi: f32,
    },
    Hinge {
        a0: Xform,
        a1: Xform,
        limits: bool,
        lo: f32,
        hi: f32,
    },
}

#[inline]
fn ident() -> Xform {
    Xform {
        pos: Vec3::ZERO,
        rot: Mat3::IDENTITY,
    }
}

#[inline]
fn ortho(m: Mat3) -> Mat3 {
    Mat3::from_quat(Quat::from_mat3(&m).normalize())
}

fn read_cframe(doc: &Document, r: Ref, prop: &str) -> Option<Xform> {
    match doc.prop(r, prop) {
        Some(Variant::CFrame(cf)) => Some(Xform::from_cframe(&cf)),
        _ => None,
    }
}

fn read_ref(doc: &Document, r: Ref, prop: &str) -> Option<Ref> {
    match doc.prop(r, prop) {
        Some(Variant::Ref(x)) if x.is_some() && doc.exists(x) => Some(x),
        _ => None,
    }
}

fn read_f32(doc: &Document, r: Ref, prop: &str, default: f32) -> f32 {
    match doc.prop(r, prop) {
        Some(Variant::Float32(v)) => v,
        Some(Variant::Float64(v)) => v as f32,
        Some(Variant::Int32(v)) => v as f32,
        Some(Variant::Int64(v)) => v as f32,
        _ => default,
    }
}

/// Returns the `MUSCLE_REST_POSE` rotation `Xform` for an R15 `Part1` or joint name.
pub fn muscle_rest_pose_xform(part_or_joint_name: &str) -> Option<Xform> {
    let joint_name = part1_to_joint(part_or_joint_name).unwrap_or(part_or_joint_name);
    let angles = MUSCLE_REST_POSE
        .iter()
        .find_map(|&(name, a)| (name == joint_name).then_some(a))?;
    let rot = RotationOrder::YXZ.from_euler_angles(Vec3::from_array(angles));
    Some(Xform {
        pos: Vec3::ZERO,
        rot: ortho(rot),
    })
}

/// Returns `(can_use_ik, is_r15)` for the currently cached rig.
pub fn can_use_ik(rig: &RigCache) -> (bool, bool) {
    if rig.root_ref.is_none() || rig.joints_by_part.is_empty() {
        return (false, false);
    }
    let is_r15 = rig.rig_type == RigTypeKind::R15;
    (true, is_r15)
}

/// Returns `true` if `part_name` can be pinned in `FullBody` IK mode.
pub fn is_part_pinnable(rig: &RigCache, part_name: &str) -> bool {
    if rig.root_part_name.as_deref() == Some(part_name) {
        return false;
    }
    rig.joints_by_part.contains_key(part_name)
}

/// Toggles `part_name` in `state.ik.pinned_parts` (matching `Thunks/TogglePinnedPart`).
pub fn toggle_pin_part(state: &mut EditorState, part_name: &str) {
    if !state.ik.pinned_parts.remove(part_name) {
        state.ik.pinned_parts.insert(part_name.to_string());
    }
}

/// Effective pinned parts for a drag, applying `pinForLimbMode` when in `IkMode::BodyPart`.
fn effective_pinned_parts(rig: &RigCache, ik: &IkState, effector_part: &str) -> HashSet<String> {
    let mut pins = ik.pinned_parts.clone();
    if ik.mode == IkMode::BodyPart && rig.rig_type == RigTypeKind::R15 {
        if effector_part == "UpperTorso" || effector_part == "LowerTorso" {
            pins.insert("LeftFoot".to_string());
            pins.insert("RightFoot".to_string());
        } else {
            pins.insert("UpperTorso".to_string());
            pins.insert("LowerTorso".to_string());
        }
    }
    pins
}

/// Returns the ordered list of part names in the IK chain (root-most movable part first,
/// `effector_part` last) for the given `IkState` and `effector_part`.
pub fn get_ik_chain_parts(rig: &RigCache, ik: &IkState, effector_part: &str) -> Vec<String> {
    if !rig.joints_by_part.contains_key(effector_part) {
        return Vec::new();
    }

    let pins = effective_pinned_parts(rig, ik, effector_part);
    let root_name = rig.root_part_name.as_deref();

    let mut chain = Vec::new();
    let mut visited = HashSet::new();
    let mut cur = effector_part.to_string();

    while visited.insert(cur.clone()) {
        if chain.len() >= 64 {
            break;
        }
        // A pinned ancestor stops the chain (its child is the chain root).
        // The dragged effector itself is always allowed to move even if pinned.
        if cur != effector_part && pins.contains(&cur) {
            break;
        }
        if root_name == Some(cur.as_str()) {
            break;
        }
        let Some(joint) = rig.joints_by_part.get(&cur) else {
            break;
        };
        chain.push(cur.clone());

        let Some(parent) = joint
            .part0_name
            .as_deref()
            .or_else(|| rig.parent_map.get(&cur).map(String::as_str))
        else {
            break;
        };
        cur = parent.to_string();
    }

    chain.reverse();
    chain
}

/// Builds the set of parts belonging to the active IK chain (and pinned parts) for UI highlighting.
pub fn get_active_chain_set(
    rig: &RigCache,
    ik: &IkState,
    selected_part: Option<&str>,
) -> HashSet<String> {
    let mut active = HashSet::new();
    if !ik.enabled {
        return active;
    }
    if let Some(part) = selected_part {
        let chain = get_ik_chain_parts(rig, ik, part);
        for p in &chain {
            active.insert(p.clone());
        }
        if let Some(first) = chain.first() {
            if let Some(parent) = rig.parent_map.get(first) {
                active.insert(parent.clone());
            }
        }
    }
    if ik.mode == IkMode::FullBody {
        for pinned in &ik.pinned_parts {
            active.insert(pinned.clone());
        }
    }
    active
}

/// Computes the current world `Xform` of a part via forward kinematics from the rig root.
fn compute_part_world(
    doc: &Document,
    rig: &RigCache,
    part_name: &str,
    overrides: &HashMap<String, Xform>,
    depth: usize,
) -> Xform {
    if let Some(xf) = overrides.get(part_name) {
        return *xf;
    }
    if depth < 32 {
        if let Some(joint) = rig.joints_by_part.get(part_name) {
            if let Some(ref p0_name) = joint.part0_name {
                let p0_world = compute_part_world(doc, rig, p0_name, overrides, depth + 1);
                let t = read_cframe(doc, joint.joint_ref, "Transform").unwrap_or_else(ident);
                return cf_mul(
                    &cf_mul(&cf_mul(&p0_world, &joint.c0), &t),
                    &joint.c1.inverse(),
                );
            }
        }
    }
    if let Some(&r) = rig.part_refs.get(part_name) {
        if let Some(xf) = read_cframe(doc, r, "CFrame") {
            return xf;
        }
    }
    if let Some(&xf) = rig.starting_pose.get(part_name) {
        return xf;
    }
    if rig.root_part_name.as_deref() == Some(part_name) {
        if let Some(r) = rig.root_part_ref {
            if let Some(xf) = read_cframe(doc, r, "CFrame") {
                return xf;
            }
        }
        if let Some(xf) = rig.root_cframe {
            return xf;
        }
    }
    ident()
}

/// Builds a `JointLimit` from an `IkConstraintConfig` table entry.
fn limit_from_config(link: &ChainLink, cfg: IkConstraintConfig) -> Option<JointLimit> {
    let mut a0 = link.c0;
    let a1 = link.c1;

    if let Some(axis_arr) = cfg.axis {
        let x_axis = Vec3::from_array(axis_arr).normalize_or_zero();
        if x_axis.length_squared() > 1e-6 {
            let y_hint = cfg
                .secondary_axis
                .map(Vec3::from_array)
                .unwrap_or(Vec3::Y)
                .normalize_or_zero();
            let z_axis = x_axis.cross(y_hint).normalize_or_zero();
            if z_axis.length_squared() > 1e-6 {
                let y_axis = z_axis.cross(x_axis).normalize_or_zero();
                a0.rot = Mat3::from_cols(x_axis, y_axis, z_axis);
            }
        }
    }

    match cfg.kind {
        IkConstraintKind::BallSocket => {
            if !cfg.limits_enabled && !cfg.twist_limits_enabled {
                return None;
            }
            Some(JointLimit::BallSocket {
                a0,
                a1,
                limits: cfg.limits_enabled,
                upper: cfg.upper_angle.clamp(0.0, 180.0).to_radians(),
                twist_limits: cfg.twist_limits_enabled,
                twist_lo: cfg.twist_lower_angle.to_radians(),
                twist_hi: cfg.twist_upper_angle.to_radians(),
            })
        }
        IkConstraintKind::Hinge => Some(JointLimit::Hinge {
            a0,
            a1,
            limits: cfg.limits_enabled,
            lo: cfg.lower_angle.to_radians(),
            hi: cfg.upper_angle.to_radians(),
        }),
    }
}

/// Discovers DOM `BallSocketConstraint` / `HingeConstraint` limits on the rig, falling back to
/// `IK_CONSTRAINTS_CONFIG` for standard R15 parts.
fn resolve_chain_limits(
    doc: &Document,
    rig: &RigCache,
    links: &[ChainLink],
) -> Vec<Option<JointLimit>> {
    let mut out = vec![None; links.len()];

    if let Some(scope) = rig.root_ref {
        if doc.exists(scope) {
            for inst in doc.dom.descendants_of(scope) {
                let cls = inst.class.as_str();
                if cls != "BallSocketConstraint" && cls != "HingeConstraint" {
                    continue;
                }
                let r = inst.referent();
                let (Some(att0), Some(att1)) = (
                    read_ref(doc, r, "Attachment0"),
                    read_ref(doc, r, "Attachment1"),
                ) else {
                    continue;
                };
                let (Some(p0), Some(p1)) = (
                    doc.dom.get_by_ref(att0).map(|x| x.parent()),
                    doc.dom.get_by_ref(att1).map(|x| x.parent()),
                ) else {
                    continue;
                };
                let f0 = read_cframe(doc, att0, "CFrame").unwrap_or_else(ident);
                let f1 = read_cframe(doc, att1, "CFrame").unwrap_or_else(ident);

                for (idx, link) in links.iter().enumerate() {
                    if out[idx].is_some() {
                        continue;
                    }
                    let (Some(lp0), Some(lp1)) = (link.part0_ref, link.part1_ref) else {
                        continue;
                    };
                    let (a0, a1) = if p0 == lp0 && p1 == lp1 {
                        (f0, f1)
                    } else if p0 == lp1 && p1 == lp0 {
                        (f1, f0)
                    } else {
                        continue;
                    };
                    let limits = matches!(doc.prop(r, "LimitsEnabled"), Some(Variant::Bool(true)));
                    if cls == "BallSocketConstraint" {
                        let twist_limits =
                            matches!(doc.prop(r, "TwistLimitsEnabled"), Some(Variant::Bool(true)));
                        if !limits && !twist_limits {
                            continue;
                        }
                        let upper = read_f32(doc, r, "UpperAngle", 45.0)
                            .clamp(0.0, 180.0)
                            .to_radians();
                        let twist_lo = read_f32(doc, r, "TwistLowerAngle", -45.0).to_radians();
                        let twist_hi = read_f32(doc, r, "TwistUpperAngle", 45.0).to_radians();
                        out[idx] = Some(JointLimit::BallSocket {
                            a0,
                            a1,
                            limits,
                            upper,
                            twist_limits,
                            twist_lo,
                            twist_hi,
                        });
                    } else {
                        let lo = read_f32(doc, r, "LowerAngle", -45.0).to_radians();
                        let hi = read_f32(doc, r, "UpperAngle", 45.0).to_radians();
                        out[idx] = Some(JointLimit::Hinge {
                            a0,
                            a1,
                            limits,
                            lo,
                            hi,
                        });
                    }
                }
            }
        }
    }

    // Fall back to R15 `IK_CONSTRAINTS_CONFIG` for any link without an explicit DOM constraint.
    for (idx, link) in links.iter().enumerate() {
        if out[idx].is_none() {
            if let Some(cfg) = part_ik_constraint(&link.part1_name) {
                out[idx] = limit_from_config(link, cfg);
            }
        }
    }

    out
}

/// Enforces `BallSocketConstraint` / `HingeConstraint` swing-twist limits on a link's `transform`.
fn clamp_link(link: &mut ChainLink, limit: Option<JointLimit>) {
    let Some(lim) = limit else {
        return;
    };
    let (a0, a1) = match lim {
        JointLimit::BallSocket { a0, a1, .. } | JointLimit::Hinge { a0, a1, .. } => (a0, a1),
    };

    // Part1 = Part0 * C0 * T * C1^-1 => A1_in_A0 = A0^-1 * C0 * T * C1^-1 * A1
    let rel_rot =
        a0.rot.transpose() * link.c0.rot * link.transform.rot * link.c1.rot.transpose() * a1.rot;
    let mut q = Quat::from_mat3(&rel_rot).normalize();
    if q.w < 0.0 {
        q = -q;
    }

    // Swing-twist decomposition about local X axis: q = q_swing * q_twist.
    let mut q_twist = Quat::from_xyzw(q.x, 0.0, 0.0, q.w);
    let t_len = q_twist.length();
    q_twist = if t_len > 1e-6 {
        q_twist / t_len
    } else {
        Quat::IDENTITY
    };
    let mut twist_ang = 2.0 * q_twist.x.atan2(q_twist.w);
    if twist_ang > std::f32::consts::PI {
        twist_ang -= std::f32::consts::TAU;
    } else if twist_ang < -std::f32::consts::PI {
        twist_ang += std::f32::consts::TAU;
    }

    let q_clamped = match lim {
        JointLimit::Hinge { limits, lo, hi, .. } => {
            let a = if limits {
                twist_ang.clamp(lo.min(hi), lo.max(hi))
            } else {
                twist_ang
            };
            Quat::from_rotation_x(a)
        }
        JointLimit::BallSocket {
            limits,
            upper,
            twist_limits,
            twist_lo,
            twist_hi,
            ..
        } => {
            let mut q_swing = (q * q_twist.conjugate()).normalize();
            if q_swing.w < 0.0 {
                q_swing = -q_swing;
            }
            if limits {
                let (axis, ang) = q_swing.to_axis_angle();
                if ang > upper {
                    q_swing = Quat::from_axis_angle(axis, upper);
                }
            }
            let tw = if twist_limits {
                twist_ang.clamp(twist_lo.min(twist_hi), twist_lo.max(twist_hi))
            } else {
                twist_ang
            };
            (q_swing * Quat::from_rotation_x(tw)).normalize()
        }
    };

    let clamped_mat = Mat3::from_quat(q_clamped);
    link.transform.rot =
        ortho(link.c0.rot.transpose() * a0.rot * clamped_mat * a1.rot.transpose() * link.c1.rot);
}

/// Forward kinematics for all joint nodes (`Part0 * C0 * Transform`) and part frames (`node * C1^-1`).
fn forward_kinematics(base: &Xform, links: &[ChainLink]) -> (Vec<Xform>, Vec<Xform>) {
    let mut nodes = Vec::with_capacity(links.len());
    let mut parts = Vec::with_capacity(links.len());
    let mut p0 = *base;
    for l in links {
        let node = cf_mul(&cf_mul(&p0, &l.c0), &l.transform);
        let p1 = cf_mul(&node, &l.c1.inverse());
        nodes.push(node);
        parts.push(p1);
        p0 = p1;
    }
    (nodes, parts)
}

fn part0_world_at(base: &Xform, links: &[ChainLink], idx: usize) -> Xform {
    let mut p0 = *base;
    for l in &links[..idx] {
        p0 = cf_mul(&cf_mul(&cf_mul(&p0, &l.c0), &l.transform), &l.c1.inverse());
    }
    p0
}

fn turn_link(links: &mut [ChainLink], idx: usize, world_rot_delta: Quat, p0_world: &Xform) {
    let l = &mut links[idx];
    let joint_base = cf_mul(p0_world, &l.c0);
    let node = cf_mul(&joint_base, &l.transform);
    let new_rot = ortho(Mat3::from_quat(world_rot_delta) * node.rot);
    let new_node = Xform {
        pos: node.pos,
        rot: new_rot,
    };
    l.transform = cf_mul(&joint_base.inverse(), &new_node);
    l.transform.rot = ortho(l.transform.rot);
}

/// FABRIK pass on node positions: `pts[0]` stays anchored, `pts[n-1]` moves toward `goal`.
fn fabrik_positions(pts: &mut [Vec3], goal: Vec3) {
    let n = pts.len();
    if n < 2 {
        return;
    }
    let lens: Vec<f32> = (0..n - 1).map(|i| (pts[i + 1] - pts[i]).length()).collect();
    let total: f32 = lens.iter().sum();
    if total <= 1e-6 {
        return;
    }
    let base = pts[0];
    let mut clamped_goal = goal;
    let to = clamped_goal - base;
    if to.length() > total * MAX_REACH_FRACTION {
        clamped_goal = base + to.normalize_or_zero() * (total * MAX_REACH_FRACTION);
    }

    for _ in 0..20 {
        if (pts[n - 1] - clamped_goal).length() < 1e-4 {
            break;
        }
        pts[n - 1] = clamped_goal;
        for i in (0..n - 1).rev() {
            let dir = (pts[i] - pts[i + 1]).normalize_or_zero();
            if dir.length_squared() > 1e-8 {
                pts[i] = pts[i + 1] + dir * lens[i];
            }
        }
        pts[0] = base;
        for i in 0..n - 1 {
            let dir = (pts[i + 1] - pts[i]).normalize_or_zero();
            if dir.length_squared() > 1e-8 {
                pts[i + 1] = pts[i] + dir * lens[i];
            }
        }
    }
}

/// Applies subtle R15 `MUSCLE_REST_POSE` pre-bend if a chain link is near identity to avoid
/// straight-limb singularities on elbows and knees.
fn apply_muscle_rest_bias(links: &mut [ChainLink], limits: &[Option<JointLimit>], is_r15: bool) {
    if !is_r15 || links.len() < 2 {
        return;
    }
    for (i, link) in links.iter_mut().enumerate() {
        let Some(rest) = muscle_rest_pose_xform(&link.part1_name) else {
            continue;
        };
        let q_cur = Quat::from_mat3(&link.transform.rot).normalize();
        let q_rest = Quat::from_mat3(&rest.rot).normalize();
        // Only bias joints that are close to identity (unposed / straight).
        let angle_from_ident = 2.0 * q_cur.w.abs().clamp(0.0, 1.0).acos();
        if angle_from_ident < 0.05 && q_rest.angle_between(Quat::IDENTITY) > 0.05 {
            let blended = q_cur.slerp(q_rest, 0.25);
            link.transform.rot = Mat3::from_quat(blended);
            clamp_link(link, limits[i]);
        }
    }
}

/// Solves a single chain of `ChainLink`s from `base_world` to `target_world`.
fn solve_chain(
    base_world: Xform,
    links: &mut [ChainLink],
    limits: &[Option<JointLimit>],
    target_world: Xform,
    apply_rotation: bool,
    allow_root_translation: bool,
    is_r15: bool,
) -> (Vec<(String, Xform)>, Xform) {
    let n = links.len();
    debug_assert!(n > 0);

    if n == 1 {
        let l = &mut links[0];
        let joint_base = cf_mul(&base_world, &l.c0);
        if allow_root_translation {
            // Exact Motor6D.Transform = C0^-1 * Part0^-1 * Part1 * C1.
            let target_node = cf_mul(&target_world, &l.c1);
            l.transform = cf_mul(&joint_base.inverse(), &target_node);
            l.transform.rot = ortho(l.transform.rot);
            if !apply_rotation {
                // Keep translation, preserve original rotation if rotation not requested.
            }
        } else if apply_rotation {
            let want_rot = ortho(target_world.rot * l.c1.rot);
            let node_pos = cf_mul(&joint_base, &l.transform).pos;
            let want_node = Xform {
                pos: node_pos,
                rot: want_rot,
            };
            l.transform = cf_mul(&joint_base.inverse(), &want_node);
            l.transform.rot = ortho(l.transform.rot);
            clamp_link(l, limits[0]);
        } else {
            // Point the single bone toward target_world.pos.
            let (nodes, parts) = forward_kinematics(&base_world, std::slice::from_ref(l));
            let a = (parts[0].pos - nodes[0].pos).normalize_or_zero();
            let b = (target_world.pos - nodes[0].pos).normalize_or_zero();
            if a.length_squared() > 1e-6 && b.length_squared() > 1e-6 {
                let dq = Quat::from_rotation_arc(a, b);
                let mut tmp = [l.clone()];
                turn_link(&mut tmp, 0, dq, &base_world);
                clamp_link(&mut tmp[0], limits[0]);
                *l = tmp[0].clone();
            }
        }
    } else {
        apply_muscle_rest_bias(links, limits, is_r15);

        let last = n - 1;
        let has_limits = limits.iter().any(Option::is_some);
        let fabrik_iters = if has_limits { 12 } else { 6 };

        // Phase 1: FABRIK iterations with per-joint swing-twist clamping.
        for _ in 0..fabrik_iters {
            let (nodes, parts) = forward_kinematics(&base_world, links);
            let goal_node_pos = if apply_rotation {
                cf_mul(&target_world, &links[last].c1).pos
            } else {
                let eff_at_target = Xform {
                    pos: target_world.pos,
                    rot: parts[last].rot,
                };
                cf_mul(&eff_at_target, &links[last].c1).pos
            };

            let mut pts: Vec<Vec3> = nodes.iter().map(|xf| xf.pos).collect();
            fabrik_positions(&mut pts, goal_node_pos);

            for i in 0..last {
                let (cur_nodes, _) = forward_kinematics(&base_world, links);
                let a = (cur_nodes[i + 1].pos - cur_nodes[i].pos).normalize_or_zero();
                let b = (pts[i + 1] - cur_nodes[i].pos).normalize_or_zero();
                if a.length_squared() <= 1e-8 || b.length_squared() <= 1e-8 {
                    continue;
                }
                let p0w = part0_world_at(&base_world, links, i);
                turn_link(links, i, Quat::from_rotation_arc(a, b), &p0w);
                clamp_link(&mut links[i], limits[i]);
            }
        }

        // Phase 2: Damped CCD refinement (helps constrained hinges converge accurately).
        for _ in 0..8 {
            let (_, parts) = forward_kinematics(&base_world, links);
            if (parts[last].pos - target_world.pos).length() < 1e-3 {
                break;
            }
            for i in (0..last).rev() {
                let (cur_nodes, cur_parts) = forward_kinematics(&base_world, links);
                let pivot = cur_nodes[i].pos;
                let to_eff = (cur_parts[last].pos - pivot).normalize_or_zero();
                let to_goal = (target_world.pos - pivot).normalize_or_zero();
                if to_eff.length_squared() <= 1e-8 || to_goal.length_squared() <= 1e-8 {
                    continue;
                }
                let full_q = Quat::from_rotation_arc(to_eff, to_goal);
                let damped_q = Quat::IDENTITY.slerp(full_q, 0.75);
                let p0w = part0_world_at(&base_world, links, i);
                turn_link(links, i, damped_q, &p0w);
                clamp_link(&mut links[i], limits[i]);
            }
        }

        // Phase 3: End-effector orientation when `apply_rotation` is requested.
        if apply_rotation {
            let p0w = part0_world_at(&base_world, links, last);
            let l = &mut links[last];
            let joint_base = cf_mul(&p0w, &l.c0);
            let node = cf_mul(&joint_base, &l.transform);
            let want_node = Xform {
                pos: node.pos,
                rot: ortho(target_world.rot * l.c1.rot),
            };
            l.transform = cf_mul(&joint_base.inverse(), &want_node);
            l.transform.rot = ortho(l.transform.rot);
            clamp_link(l, limits[last]);
        }
    }

    // Reconstruct exact local Motor6D.Transform = C0^-1 * Part0^-1 * Part1 * C1 for every link.
    let (_, parts) = forward_kinematics(&base_world, links);
    let mut out = Vec::with_capacity(n);
    let mut prev_world = base_world;
    for (i, l) in links.iter().enumerate() {
        let part1_world = parts[i];
        let local_t = cf_mul(
            &cf_mul(
                &cf_mul(&l.c0.inverse(), &prev_world.inverse()),
                &part1_world,
            ),
            &l.c1,
        );
        out.push((
            l.part1_name.clone(),
            Xform {
                pos: local_t.pos,
                rot: ortho(local_t.rot),
            },
        ));
        prev_world = part1_world;
    }

    let actual_effector_world = *parts.last().unwrap_or(&base_world);
    (out, actual_effector_world)
}

fn build_chain_links(
    doc: &Document,
    rig: &RigCache,
    part_names: &[String],
) -> Option<Vec<ChainLink>> {
    let mut links = Vec::with_capacity(part_names.len());
    for part_name in part_names {
        let joint = rig.joints_by_part.get(part_name)?;
        let part0_name = joint
            .part0_name
            .clone()
            .or_else(|| rig.parent_map.get(part_name).cloned())?;
        let transform = read_cframe(doc, joint.joint_ref, "Transform").unwrap_or_else(ident);
        links.push(ChainLink {
            part0_name,
            part1_name: part_name.clone(),
            part0_ref: joint.part0_ref,
            part1_ref: joint.part1_ref,
            c0: joint.c0,
            c1: joint.c1,
            transform,
        });
    }
    Some(links)
}

/// Solves an IK drag on `effector_part` toward `target_world`, returning the exact local
/// `Motor6D.Transform` for every affected joint in the chain and the realized effector world frame.
pub fn solve_ik_drag(
    doc: &Document,
    rig: &RigCache,
    ik: &IkState,
    effector_part: &str,
    target_world: Xform,
    apply_rotation: bool,
) -> Option<IkSolveResult> {
    let chain_parts = get_ik_chain_parts(rig, ik, effector_part);
    if chain_parts.is_empty() {
        return None;
    }

    let is_r15 = rig.rig_type == RigTypeKind::R15;
    let is_torso_drag = is_r15 && (effector_part == "LowerTorso" || effector_part == "UpperTorso");
    let pins = effective_pinned_parts(rig, ik, effector_part);

    // Capture planted foot targets before moving the torso.
    let empty_overrides = HashMap::new();
    let planted_feet: Vec<(&str, Xform)> = if is_torso_drag {
        ["LeftFoot", "RightFoot"]
            .into_iter()
            .filter(|foot| {
                rig.joints_by_part.contains_key(*foot)
                    && (ik.mode == IkMode::BodyPart || pins.contains(*foot))
            })
            .map(|foot| {
                (
                    foot,
                    compute_part_world(doc, rig, foot, &empty_overrides, 0),
                )
            })
            .collect()
    } else {
        Vec::new()
    };

    let mut links = build_chain_links(doc, rig, &chain_parts)?;
    let limits = resolve_chain_limits(doc, rig, &links);
    let base_world = compute_part_world(doc, rig, &links[0].part0_name, &empty_overrides, 0);

    // Allow translation when the single link is the rig's pelvis/root joint (LowerTorso).
    let allow_root_translation = links.len() == 1
        && (links[0].part1_name == "LowerTorso"
            || rig.root_part_name.as_deref() == Some(links[0].part0_name.as_str()));

    let (mut joint_transforms, actual_effector_world) = solve_chain(
        base_world,
        &mut links,
        &limits,
        target_world,
        apply_rotation,
        allow_root_translation,
        is_r15,
    );

    // If dragging LowerTorso/UpperTorso with planted feet, solve each leg chain back to its foot.
    if !planted_feet.is_empty() {
        let (_, solved_parts) = forward_kinematics(&base_world, &links);
        let mut world_overrides = HashMap::new();
        for (l, p_world) in links.iter().zip(solved_parts.iter()) {
            world_overrides.insert(l.part1_name.clone(), *p_world);
        }

        let leg_chains: &[(&str, [&str; 3])] = &[
            ("LeftFoot", ["LeftUpperLeg", "LeftLowerLeg", "LeftFoot"]),
            ("RightFoot", ["RightUpperLeg", "RightLowerLeg", "RightFoot"]),
        ];

        for (foot_name, foot_world) in planted_feet {
            let Some((_, leg_part_names)) = leg_chains.iter().find(|&&(f, _)| f == foot_name)
            else {
                continue;
            };
            let leg_vec: Vec<String> = leg_part_names.iter().map(|s| (*s).to_string()).collect();
            let Some(mut leg_links) = build_chain_links(doc, rig, &leg_vec) else {
                continue;
            };
            let leg_limits = resolve_chain_limits(doc, rig, &leg_links);
            let leg_base =
                compute_part_world(doc, rig, &leg_links[0].part0_name, &world_overrides, 0);
            let (leg_transforms, _) = solve_chain(
                leg_base,
                &mut leg_links,
                &leg_limits,
                foot_world,
                true,
                false,
                is_r15,
            );
            for (name, xf) in leg_transforms {
                if !joint_transforms.iter().any(|(n, _)| n == &name) {
                    joint_transforms.push((name, xf));
                }
            }
        }
    }

    Some(IkSolveResult {
        joint_transforms,
        actual_effector_world,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim_editor::state::{RigJointInfo, RigJointKind};

    fn make_two_link_arm_rig() -> (Document, RigCache) {
        let doc = Document::empty();
        let mut rig = RigCache::default();
        rig.rig_type = RigTypeKind::Custom;
        rig.rig_scale = 1.0;
        rig.root_part_name = Some("Torso".to_string());
        rig.root_cframe = Some(ident());
        rig.starting_pose.insert("Torso".to_string(), ident());

        // Torso -> UpperArm (length 2 along -Y) -> LowerArm (length 2 along -Y) -> Hand
        let upper_joint = RigJointInfo {
            name: "Shoulder".to_string(),
            kind: RigJointKind::Motor6D,
            joint_ref: Ref::none(),
            part0_name: Some("Torso".to_string()),
            part1_name: "UpperArm".to_string(),
            part0_ref: None,
            part1_ref: None,
            c0: Xform {
                pos: Vec3::new(0.0, 0.0, 0.0),
                rot: Mat3::IDENTITY,
            },
            c1: Xform {
                pos: Vec3::new(0.0, 1.0, 0.0),
                rot: Mat3::IDENTITY,
            },
        };
        let lower_joint = RigJointInfo {
            name: "Elbow".to_string(),
            kind: RigJointKind::Motor6D,
            joint_ref: Ref::none(),
            part0_name: Some("UpperArm".to_string()),
            part1_name: "LowerArm".to_string(),
            part0_ref: None,
            part1_ref: None,
            c0: Xform {
                pos: Vec3::new(0.0, -1.0, 0.0),
                rot: Mat3::IDENTITY,
            },
            c1: Xform {
                pos: Vec3::new(0.0, 1.0, 0.0),
                rot: Mat3::IDENTITY,
            },
        };
        let hand_joint = RigJointInfo {
            name: "Wrist".to_string(),
            kind: RigJointKind::Motor6D,
            joint_ref: Ref::none(),
            part0_name: Some("LowerArm".to_string()),
            part1_name: "Hand".to_string(),
            part0_ref: None,
            part1_ref: None,
            c0: Xform {
                pos: Vec3::new(0.0, -1.0, 0.0),
                rot: Mat3::IDENTITY,
            },
            c1: Xform {
                pos: Vec3::new(0.0, 0.0, 0.0),
                rot: Mat3::IDENTITY,
            },
        };

        rig.joints_by_part
            .insert("UpperArm".to_string(), upper_joint);
        rig.joints_by_part
            .insert("LowerArm".to_string(), lower_joint);
        rig.joints_by_part.insert("Hand".to_string(), hand_joint);

        rig.parent_map
            .insert("UpperArm".to_string(), "Torso".to_string());
        rig.parent_map
            .insert("LowerArm".to_string(), "UpperArm".to_string());
        rig.parent_map
            .insert("Hand".to_string(), "LowerArm".to_string());

        (doc, rig)
    }

    #[test]
    fn test_muscle_rest_pose_has_all_15_joints() {
        assert_eq!(MUSCLE_REST_POSE.len(), 15);
        let right_elbow = muscle_rest_pose_xform("RightLowerArm").expect("RightElbow pose");
        assert!((right_elbow.rot.determinant() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_two_link_arm_reaches_target() {
        let (doc, rig) = make_two_link_arm_rig();
        let ik = IkState {
            enabled: true,
            mode: IkMode::FullBody,
            ..Default::default()
        };

        // Arm segments are 2.0 + 2.0 = 4.0 studs from origin. Target at (2.0, -2.0, 0.0) is distance ~2.83.
        let target = Xform {
            pos: Vec3::new(2.0, -2.0, 0.0),
            rot: Mat3::IDENTITY,
        };
        let result = solve_ik_drag(&doc, &rig, &ik, "Hand", target, false).expect("IK solve");
        assert_eq!(result.joint_transforms.len(), 3);
        let dist = (result.actual_effector_world.pos - target.pos).length();
        assert!(
            dist < 0.05,
            "Expected effector to reach target within 0.05 studs, got dist={dist}"
        );
    }

    #[test]
    fn test_pinned_ancestor_stops_chain() {
        let (_doc, rig) = make_two_link_arm_rig();
        let mut state = EditorState::default();
        state.ik.enabled = true;
        state.ik.mode = IkMode::FullBody;

        let full_chain = get_ik_chain_parts(&rig, &state.ik, "Hand");
        assert_eq!(full_chain, vec!["UpperArm", "LowerArm", "Hand"]);

        toggle_pin_part(&mut state, "UpperArm");
        assert!(state.ik.pinned_parts.contains("UpperArm"));

        let pinned_chain = get_ik_chain_parts(&rig, &state.ik, "Hand");
        assert_eq!(pinned_chain, vec!["LowerArm", "Hand"]);

        toggle_pin_part(&mut state, "UpperArm");
        assert!(!state.ik.pinned_parts.contains("UpperArm"));
    }

    #[test]
    fn test_hinge_and_ball_socket_constraint_clamping() {
        // Test Hinge clamping on Elbow (0..135 deg)
        let mut elbow = ChainLink {
            part0_name: "RightUpperArm".to_string(),
            part1_name: "RightLowerArm".to_string(),
            part0_ref: None,
            part1_ref: None,
            c0: ident(),
            c1: ident(),
            // Try an illegal negative hinge bend of -60 degrees about X plus 30 deg off-axis Y.
            transform: Xform {
                pos: Vec3::ZERO,
                rot: Mat3::from_rotation_x(-60.0_f32.to_radians())
                    * Mat3::from_rotation_y(30.0_f32.to_radians()),
            },
        };
        let elbow_cfg = part_ik_constraint("RightLowerArm").expect("Elbow constraint");
        let elbow_lim = limit_from_config(&elbow, elbow_cfg);
        clamp_link(&mut elbow, elbow_lim);

        // Off-axis rotation must be removed and X angle clamped to >= 0 deg.
        let q = Quat::from_mat3(&elbow.transform.rot).normalize();
        let (_axis, ang) = q.to_axis_angle();
        assert!(
            ang < 1e-3,
            "Expected -60 deg elbow bend to clamp to 0 deg, got {} deg",
            ang.to_degrees()
        );

        // Test BallSocket clamping on Neck (upper_angle = 30 deg)
        let mut neck = ChainLink {
            part0_name: "UpperTorso".to_string(),
            part1_name: "Head".to_string(),
            part0_ref: None,
            part1_ref: None,
            c0: ident(),
            c1: ident(),
            // 90 deg pitch about Z (swing away from X)
            transform: Xform {
                pos: Vec3::ZERO,
                rot: Mat3::from_rotation_z(90.0_f32.to_radians()),
            },
        };
        let neck_cfg = part_ik_constraint("Head").expect("Neck constraint");
        let neck_lim = limit_from_config(&neck, neck_cfg);
        clamp_link(&mut neck, neck_lim);

        let q_neck = Quat::from_mat3(&neck.transform.rot).normalize();
        let (_axis, neck_ang) = q_neck.to_axis_angle();
        assert!(
            neck_ang.to_degrees() <= 30.1,
            "Expected neck swing clamped to <= 30 deg, got {} deg",
            neck_ang.to_degrees()
        );
    }
}

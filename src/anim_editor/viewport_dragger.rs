//! 1:1 3D Viewport Draggers, Joint/Bone Picking, and TransformHandles math
//! for RusTudio's native `AnimationClipEditor`.
//!
//! Derived from:
//! - `Components/InstanceSelector`
//! - `Components/Draggers/DraggerWrapper`
//! - `Components/Draggers/AnimationClipEditorDragger`
//! - `Util/DraggerSchema/getMouseTarget`
//! - `Util/DraggerSchema/SelectionInfo`
//! - `Util/DraggerSchema/TransformHandlesImplementation`

use std::collections::{HashMap, HashSet};

use egui::{Color32, FontId, PointerButton, Pos2, Rect, Stroke, Vec2};
use glam::{Mat3, Quat, Vec3};
use rbx_dom_weak::types::Ref;

use rustudio_core::doc::{self, Document};
use rustudio_core::math::{cf_inv, cf_mul, snap, Ray, Xform};
use rustudio_core::mech;
use rustudio_render::camera::Camera;
use rustudio_render::scene::Scene;

use crate::anim_editor::constants::{PROP_POSITION, PROP_ROTATION, PROP_X, PROP_Y, PROP_Z};
use crate::anim_editor::model::{KeyframeValue, TrackType};
use crate::anim_editor::rig_viz;
use crate::anim_editor::state::{
    ActiveDialog, EditorState, PlayState, RigCache, RigJointKind, ToolMode,
};
use crate::viewport::AXIS_COLORS;

const HOVER_COLOR: Color32 = Color32::from_rgb(255, 214, 92);
const IDENTITY_XFORM: Xform = Xform {
    pos: Vec3::ZERO,
    rot: Mat3::IDENTITY,
};

/// Result returned from [`handle_viewport_interaction`] to the host viewport.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewportAction {
    /// Set when the user clicks a valid rig candidate (`InstanceSelector`).
    pub attach_rig: Option<Ref>,
    /// True if any joint/bone transform was modified this frame.
    pub pose_changed: bool,
    /// True when a new undo waypoint was pushed at drag start/commit.
    pub commit_waypoint: bool,
    /// True if the Animation Editor consumed pointer input (suppressing normal
    /// Studio part picking/marquee).
    pub consumed_pointer: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct GizmoHandle {
    tool: ToolMode,
    axis: usize,
    sign: f32,
}

#[derive(Debug, Clone)]
struct BoneDragSnapshot {
    orig_bone_cframe: Xform,
    orig_transformed_world: Xform,
}

#[derive(Debug, Clone)]
struct ActiveGizmoDrag {
    handle: GizmoHandle,
    pivot: Xform,
    start_param: f32,
    last_angle: f32,
    turned_rad: f32,
    center_px: Pos2,
    /// Snapshotted `Part1.CFrame` for Motor6D / AnimationConstraint joints.
    orig_part1_cframes: HashMap<String, Xform>,
    /// Snapshotted `Bone.CFrame` + `Bone.TransformedWorldCFrame` for Bones.
    orig_bone_snapshots: HashMap<String, BoneDragSnapshot>,
    /// Ordered joint/bone names being dragged.
    dragged_joints: Vec<String>,
    /// Snapshotted effector world CFrame when IK is enabled.
    orig_effector_world: Option<Xform>,
    /// Last applied amount (studs for Move, degrees for Rotate) for the HUD pill.
    amount: f32,
}

fn drag_session_id() -> egui::Id {
    egui::Id::new("ace_viewport_dragger_session")
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t - p).length()
}

fn part_world_xform(doc: &Document, scene: &Scene, r: Ref) -> Option<Xform> {
    scene
        .get(r)
        .map(|sp| sp.xf)
        .or_else(|| doc::xform_of(doc, r))
}

/// Computes the exact 3D gizmo pivot `Xform` for a joint or bone, matching
/// `SelectionInfo:getBoundingBox()`:
/// - **Motor6D / AnimationConstraint**: `Part1.CFrame * C1` in Local space, or
///   `Xform { pos: (Part1.CFrame * C1).pos, rot: Mat3::IDENTITY }` in World space.
/// - **Bone**: `Bone.TransformedWorldCFrame` in Local space, or translation-only
///   in World space.
pub fn compute_joint_pivot(
    doc: &Document,
    scene: &Scene,
    rig: &RigCache,
    joint_name: &str,
    local_space: bool,
) -> Option<Xform> {
    if let Some(joint) = rig.joints_by_part.get(joint_name) {
        let world_cf = match joint.kind {
            RigJointKind::Motor6D | RigJointKind::AnimationConstraint => {
                let part1_ref = joint.part1_ref?;
                let part1_xf = part_world_xform(doc, scene, part1_ref)?;
                cf_mul(&part1_xf, &joint.c1)
            }
            RigJointKind::Bone => rig_viz::bone_transformed_world_cframe(doc, joint.joint_ref),
        };
        return Some(if local_space {
            world_cf
        } else {
            Xform {
                pos: world_cf.pos,
                rot: Mat3::IDENTITY,
            }
        });
    }

    if let Some(&bone_ref) = rig.bone_map.get(joint_name) {
        let world_cf = rig_viz::bone_transformed_world_cframe(doc, bone_ref);
        return Some(if local_space {
            world_cf
        } else {
            Xform {
                pos: world_cf.pos,
                rot: Mat3::IDENTITY,
            }
        });
    }

    // Fallback for root part or unjointed part ref.
    let part_ref = *rig.part_refs.get(joint_name)?;
    let part_xf = part_world_xform(doc, scene, part_ref)?;
    Some(if local_space {
        part_xf
    } else {
        Xform {
            pos: part_xf.pos,
            rot: Mat3::IDENTITY,
        }
    })
}

fn is_descendant_of(doc: &Document, mut r: Ref, ancestor: Ref) -> bool {
    if r == ancestor {
        return true;
    }
    while let Some(inst) = doc.dom.get_by_ref(r) {
        let p = inst.parent();
        if p.is_none() {
            return false;
        }
        if p == ancestor {
            return true;
        }
        r = p;
    }
    false
}

/// Picks a rig joint or microbone under `ray`, matching
/// `Util/DraggerSchema/getMouseTarget`:
/// 1. Prioritizes `rig_viz::pick_bone_hit` when `visualize_bones` is enabled.
/// 2. Otherwise raycasts rig `BasePart`s excluding `rig.root_part_ref`,
///    accepting parts present in `rig.joints_by_part`.
pub fn pick_rig_joint(
    doc: &Document,
    scene: &Scene,
    rig: &RigCache,
    ray: &Ray,
    visualize_bones: bool,
) -> Option<String> {
    // 1. Microbone priority when VisualizeBones is enabled.
    if visualize_bones && !rig.bone_map.is_empty() {
        let bones = rig_viz::collect_bone_visuals(doc, rig);
        if let Some(bone_name) = rig_viz::pick_bone_hit(ray, &bones) {
            return Some(bone_name);
        }
    }

    // 2. Raycast rig BaseParts excluding root_part_ref, accepting parts in rig.joints_by_part.
    let mut best: Option<(f32, String)> = None;

    if !scene.parts.is_empty() {
        for (sp, b) in scene.parts.iter().zip(&scene.bounds) {
            if Some(sp.r) == rig.root_part_ref {
                continue;
            }
            let oc = b.truncate() - ray.origin;
            let along = oc.dot(ray.dir);
            if oc.length_squared() - along * along > b.w * b.w || along < -b.w {
                continue;
            }
            if let Some(root_ref) = rig.root_ref {
                if !is_descendant_of(doc, sp.r, root_ref) {
                    continue;
                }
            }
            let Some(inst) = doc.dom.get_by_ref(sp.r) else {
                continue;
            };
            if !rig.joints_by_part.contains_key(&inst.name) {
                continue;
            }
            if let Some(t) = ray.hit_obb(&sp.xf, sp.size) {
                if best.as_ref().is_none_or(|(bt, _)| t < *bt) {
                    best = Some((t, inst.name.clone()));
                }
            }
        }
    } else {
        // Fallback when Scene has not been built (e.g. headless unit tests).
        for (name, joint) in &rig.joints_by_part {
            let Some(part_ref) = joint.part1_ref else {
                continue;
            };
            if Some(part_ref) == rig.root_part_ref {
                continue;
            }
            let Some(xf) = doc::xform_of(doc, part_ref) else {
                continue;
            };
            let size = doc::vec3_of(doc.prop(part_ref, "Size")).unwrap_or(Vec3::ONE);
            if let Some(t) = ray.hit_obb(&xf, size) {
                if best.as_ref().is_none_or(|(bt, _)| t < *bt) {
                    best = Some((t, name.clone()));
                }
            }
        }
    }

    best.map(|(_, name)| name)
}

fn has_animation_controller(doc: &Document, model_ref: Ref) -> bool {
    let Some(inst) = doc.dom.get_by_ref(model_ref) else {
        return false;
    };
    inst.children().iter().any(|&c| {
        doc.class_of(c)
            .is_some_and(|cls| cls == "Humanoid" || cls == "AnimationController")
    })
}

/// Finds the outermost `Model` ancestor under `Workspace` that contains a
/// `Humanoid` or `AnimationController` and is not `current_rig`, matching
/// `Components/InstanceSelector`.
pub fn pick_rig_candidate(
    doc: &Document,
    scene: &Scene,
    ray: &Ray,
    current_rig: Option<Ref>,
) -> Option<Ref> {
    let ws = doc.workspace()?;
    let mut best_part: Option<(f32, Ref)> = None;

    for (sp, b) in scene.parts.iter().zip(&scene.bounds) {
        if sp.locked {
            continue;
        }
        let oc = b.truncate() - ray.origin;
        let along = oc.dot(ray.dir);
        if oc.length_squared() - along * along > b.w * b.w || along < -b.w {
            continue;
        }
        if let Some(t) = ray.hit_obb(&sp.xf, sp.size) {
            if best_part.as_ref().is_none_or(|(bt, _)| t < *bt) {
                best_part = Some((t, sp.r));
            }
        }
    }

    let hit_part = best_part?.1;

    // Walk up from hit_part to Workspace, keeping the outermost valid rig Model.
    let mut outermost_valid: Option<Ref> = None;
    let mut cur = hit_part;
    while let Some(inst) = doc.dom.get_by_ref(cur) {
        if cur == ws || cur == doc.dom.root_ref() {
            break;
        }
        if inst.class == "Model" && has_animation_controller(doc, cur) {
            outermost_valid = Some(cur);
        }
        let parent = inst.parent();
        if parent.is_none() {
            break;
        }
        cur = parent;
    }

    outermost_valid.filter(|&m| Some(m) != current_rig)
}

/// Exact FK Motor6D / AnimationConstraint matrix formula from
/// `TransformHandlesImplementation.applyWorldTransformToPart`:
/// `Transform = (Part0.CFrame * C0)^-1 * (globalDelta * (origPart1 * C1))`,
/// with `Transform.pos` divided by `rig_scale` (`Model:GetScale()`).
pub fn fk_motor6d_transform(
    part0_world: Xform,
    c0: Xform,
    orig_part1_world: Xform,
    c1: Xform,
    global_delta: Xform,
    rig_scale: f32,
) -> Xform {
    let part0_joint_world = cf_mul(&part0_world, &c0);
    let orig_part1_joint_world = cf_mul(&orig_part1_world, &c1);
    let target_part1_joint_world = cf_mul(&global_delta, &orig_part1_joint_world);
    let mut new_transform = cf_mul(&cf_inv(&part0_joint_world), &target_part1_joint_world);
    let scale = if rig_scale.abs() > 1e-6 {
        rig_scale
    } else {
        1.0
    };
    new_transform.pos /= scale;
    new_transform.orthonormalized()
}

/// Exact FK Bone matrix formula from
/// `TransformHandlesImplementation.applyWorldTransformToBone`:
/// `Transform = (parentWorld * origBoneCFrame)^-1 * (globalDelta * origBoneTransformedWorld)`,
/// with `Transform.pos` divided by `rig_scale` (`Model:GetScale()`).
pub fn fk_bone_transform(
    parent_world: Xform,
    orig_bone_cframe: Xform,
    orig_bone_transformed_world: Xform,
    global_delta: Xform,
    rig_scale: f32,
) -> Xform {
    let rest_world = cf_mul(&parent_world, &orig_bone_cframe);
    let target_world = cf_mul(&global_delta, &orig_bone_transformed_world);
    let mut new_transform = cf_mul(&cf_inv(&rest_world), &target_world);
    let scale = if rig_scale.abs() > 1e-6 {
        rig_scale
    } else {
        1.0
    };
    new_transform.pos /= scale;
    new_transform.orthonormalized()
}

/// Writes a normalized joint/bone `Transform` (`pos` already divided by
/// `rig.rig_scale`) into `state` via `ops::value_changed`, matching
/// `DraggerWrapper.OnManipulateJoints`.
fn commit_joint_transform(state: &mut EditorState, joint_name: &str, transform: Xform) {
    if state.read_only || state.playback.play_state != PlayState::Pause {
        return;
    }
    let tick = state.playback.playhead;
    let is_channel = state.is_channel_animation();

    if !is_channel {
        let path = vec![joint_name.to_string()];
        crate::anim_editor::ops::value_changed(
            state,
            "Root",
            &path,
            TrackType::CFrame,
            tick,
            KeyframeValue::CFrame(transform),
            false,
        );
        return;
    }

    // Channel / Curve animation mode: decompose into Position.{X,Y,Z} and Rotation.
    let existing_track = state
        .animation_data
        .as_ref()
        .and_then(|d| d.get_track("Root", &[joint_name.to_string()]));

    let rot_type = existing_track
        .and_then(|t| t.get_rotation_type())
        .unwrap_or(state.settings.default_rotation_type);
    let euler_order = existing_track
        .and_then(|t| t.get_euler_angles_order())
        .unwrap_or(state.settings.default_euler_angles_order);

    for (axis_name, val) in [
        (PROP_X, transform.pos.x),
        (PROP_Y, transform.pos.y),
        (PROP_Z, transform.pos.z),
    ] {
        let path = vec![
            joint_name.to_string(),
            PROP_POSITION.to_string(),
            axis_name.to_string(),
        ];
        crate::anim_editor::ops::value_changed(
            state,
            "Root",
            &path,
            TrackType::Number,
            tick,
            KeyframeValue::Number(val),
            false,
        );
    }

    if rot_type == TrackType::Quaternion {
        let q = Quat::from_mat3(&transform.rot).normalize();
        let path = vec![joint_name.to_string(), PROP_ROTATION.to_string()];
        crate::anim_editor::ops::value_changed(
            state,
            "Root",
            &path,
            TrackType::Quaternion,
            tick,
            KeyframeValue::Quaternion(q),
            false,
        );
    } else {
        let angles = euler_order.to_euler_angles(transform.rot);
        for (axis_name, val) in [(PROP_X, angles.x), (PROP_Y, angles.y), (PROP_Z, angles.z)] {
            let path = vec![
                joint_name.to_string(),
                PROP_ROTATION.to_string(),
                axis_name.to_string(),
            ];
            crate::anim_editor::ops::value_changed(
                state,
                "Root",
                &path,
                TrackType::Angle,
                tick,
                KeyframeValue::Angle(val),
                false,
            );
        }
    }
}

/// Builds screen-space handle polylines for Move (3 axis arrows) or Rotate
/// (3 axis rings) centered at `pivot`.
fn build_gizmo_handles(
    rect: Rect,
    cam: &Camera,
    pivot: &Xform,
    tool: ToolMode,
) -> Vec<(GizmoHandle, Vec<Pos2>)> {
    let mut out = Vec::new();
    let origin = pivot.pos;
    let axes = pivot.rot;
    let wpp = cam.world_per_px(rect, origin);

    for axis in 0..3 {
        let dir = axes.col(axis);
        match tool {
            ToolMode::Move => {
                for sign in [1.0f32, -1.0] {
                    let base = origin + dir * sign * (6.0 * wpp);
                    let tip = base + dir * sign * (mech::GIZMO_ARROW_PX * 0.55 * wpp);
                    if let (Some(a), Some(b)) = (cam.project(rect, base), cam.project(rect, tip)) {
                        out.push((GizmoHandle { tool, axis, sign }, vec![a, b]));
                    }
                }
            }
            ToolMode::Rotate => {
                let radius = mech::GIZMO_RING_PX * wpp;
                let (u, v) = (axes.col((axis + 1) % 3), axes.col((axis + 2) % 3));
                let pts: Vec<Pos2> = (0..=64)
                    .filter_map(|i| {
                        let a = i as f32 / 64.0 * std::f32::consts::TAU;
                        cam.project(rect, origin + (u * a.cos() + v * a.sin()) * radius)
                    })
                    .collect();
                if pts.len() > 2 {
                    out.push((
                        GizmoHandle {
                            tool,
                            axis,
                            sign: 1.0,
                        },
                        pts,
                    ));
                }
            }
        }
    }

    out
}

fn hit_test_handles(handles: &[(GizmoHandle, Vec<Pos2>)], p: Pos2) -> Option<GizmoHandle> {
    let mut best: Option<(f32, GizmoHandle)> = None;
    for (h, pts) in handles {
        let d = pts
            .windows(2)
            .map(|w| dist_to_segment(p, w[0], w[1]))
            .fold(f32::MAX, f32::min);
        if d < mech::GIZMO_HIT_PX && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, *h));
        }
    }
    best.map(|(_, h)| h)
}

fn draw_gizmo_handles(
    painter: &egui::Painter,
    handles: &[(GizmoHandle, Vec<Pos2>)],
    hovered: Option<GizmoHandle>,
    active: Option<GizmoHandle>,
) {
    for (h, pts) in handles {
        let is_active = active == Some(*h);
        let is_hover = !is_active && hovered == Some(*h);
        let color = if is_active || is_hover {
            HOVER_COLOR
        } else {
            AXIS_COLORS[h.axis]
        };
        let width = if is_active {
            3.5
        } else if is_hover {
            3.0
        } else {
            2.2
        };

        match h.tool {
            ToolMode::Move if pts.len() >= 2 => {
                let (a, b) = (pts[0], pts[1]);
                painter.line_segment([a, b], Stroke::new(width, color));
                let dir = (b - a).normalized();
                if dir.length_sq() > 0.5 {
                    let perp = Vec2::new(-dir.y, dir.x);
                    let tip = b + dir * 8.0;
                    let left = b - dir * 2.0 + perp * 4.5;
                    let right = b - dir * 2.0 - perp * 4.5;
                    painter.add(egui::Shape::convex_polygon(
                        vec![tip, left, right],
                        color,
                        Stroke::NONE,
                    ));
                }
            }
            ToolMode::Rotate => {
                painter.add(egui::Shape::line(pts.clone(), Stroke::new(width, color)));
            }
            _ => {}
        }
    }
}

/// Draws the on-screen drag readout pill (e.g. `45.0°` or `1.25 studs`).
fn draw_drag_readout_pill(
    painter: &egui::Painter,
    rect: Rect,
    pivot_px: Option<Pos2>,
    tool: ToolMode,
    amount: f32,
) {
    let text = match tool {
        ToolMode::Rotate => format!("{amount:.1}°"),
        ToolMode::Move => format!("{amount:.2} studs"),
    };
    let font = FontId::proportional(13.0);
    let galley = painter.layout_no_wrap(text, font, Color32::WHITE);
    let pad = Vec2::new(8.0, 4.0);
    let size = galley.size() + pad * 2.0;

    let anchor = pivot_px
        .map(|p| p + Vec2::new(14.0, -size.y - 10.0))
        .unwrap_or_else(|| rect.min + Vec2::new(12.0, 12.0));
    let clamped = Pos2::new(
        anchor.x.clamp(
            rect.min.x + 6.0,
            (rect.max.x - size.x - 6.0).max(rect.min.x + 6.0),
        ),
        anchor.y.clamp(
            rect.min.y + 6.0,
            (rect.max.y - size.y - 6.0).max(rect.min.y + 6.0),
        ),
    );
    let pill_rect = Rect::from_min_size(clamped, size);
    painter.rect_filled(
        pill_rect,
        4.0,
        Color32::from_rgba_unmultiplied(24, 24, 28, 220),
    );
    painter.rect_stroke(
        pill_rect,
        4.0,
        Stroke::new(1.0, Color32::from_rgb(0, 162, 255)),
        egui::StrokeKind::Inside,
    );
    painter.galley(pill_rect.min + pad, galley, Color32::WHITE);
}

fn resolve_selected_joint_names(doc: &Document, state: &EditorState) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for path in &state.selected_tracks {
        if let Some(name) = path.first() {
            if seen.insert(name.clone()) {
                out.push(name.clone());
            }
        }
    }

    if out.is_empty() {
        for &r in &state.selected_track_instances {
            if let Some(inst) = doc.dom.get_by_ref(r) {
                if seen.insert(inst.name.clone()) {
                    out.push(inst.name.clone());
                }
            }
        }
    }

    out
}

fn select_joint_in_state(state: &mut EditorState, joint_name: &str, additive: bool) {
    let inst_ref = state
        .rig_cache
        .joints_by_part
        .get(joint_name)
        .and_then(|j| match j.kind {
            RigJointKind::Bone => Some(j.joint_ref),
            RigJointKind::Motor6D | RigJointKind::AnimationConstraint => j.part1_ref,
        })
        .or_else(|| state.rig_cache.bone_map.get(joint_name).copied())
        .or_else(|| state.rig_cache.part_refs.get(joint_name).copied());

    let path = vec![joint_name.to_string()];

    if additive {
        if let Some(pos) = state.selected_tracks.iter().position(|p| p == &path) {
            state.selected_tracks.remove(pos);
            if let Some(r) = inst_ref {
                state.selected_track_instances.retain(|&x| x != r);
            }
        } else {
            state.selected_tracks.push(path.clone());
            if let Some(r) = inst_ref {
                if !state.selected_track_instances.contains(&r) {
                    state.selected_track_instances.push(r);
                }
            }
            state.last_selected_path = Some(path);
        }
    } else {
        state.selected_tracks.clear();
        state.selected_track_instances.clear();
        state.selected_tracks.push(path.clone());
        if let Some(r) = inst_ref {
            state.selected_track_instances.push(r);
        }
        state.last_selected_path = Some(path);
    }
}

/// Main per-frame viewport interaction handler for the Animation Clip Editor.
///
/// Handles:
/// - Hover highlighting & click-picking of rig candidates (`InstanceSelector`)
/// - Hover highlighting & click-picking of rig joints/microbones (Ctrl/Shift multi-select)
/// - Move (3 axis arrows) and Rotate (3 axis rings) gizmo handles at the last selected joint pivot
/// - Snapping (Shift disables snapping!)
/// - FK Motor6D / AnimationConstraint / Bone drags and IK drags via `ik::solve_ik_drag`
/// - Single undo waypoint on drag start/commit
/// - On-screen drag readout pill, microbone visualization, and selection boxes
#[allow(clippy::too_many_arguments)]
pub fn handle_viewport_interaction(
    ctx: &egui::Context,
    painter: &egui::Painter,
    rect: Rect,
    resp: &egui::Response,
    cam: &Camera,
    scene: &Scene,
    doc: &mut Document,
    state: &mut EditorState,
    snap_move: Option<f32>,
    snap_rotate_deg: Option<f32>,
) -> ViewportAction {
    let mut action = ViewportAction::default();

    let pointer_pos = ctx.input(|i| i.pointer.interact_pos());
    let primary_pressed = ctx.input(|i| i.pointer.button_pressed(PointerButton::Primary));
    let primary_down = ctx.input(|i| i.pointer.button_down(PointerButton::Primary));
    let primary_released = ctx.input(|i| i.pointer.button_released(PointerButton::Primary));
    let modifiers = ctx.input(|i| i.modifiers);
    let in_rect = pointer_pos.is_some_and(|p| rect.contains(p));
    let has_active_clip = state.animation_data.is_some() && state.rig_cache.root_ref.is_some();
    if has_active_clip {
        state.sync_selected_track_instances();
    }

    // Handle Animation Editor viewport hotkeys (Space, R, Ctrl+L, Ctrl+Z/Y) when hovering 3D viewport.
    if has_active_clip
        && resp.hovered()
        && !ctx.egui_wants_keyboard_input()
        && matches!(state.active_dialog, ActiveDialog::None)
    {
        ctx.input_mut(|i| {
            let ctrl = i.modifiers.ctrl || i.modifiers.command;
            let shift = i.modifiers.shift;
            if !ctrl && i.consume_key(egui::Modifiers::NONE, egui::Key::Space) {
                if state.playback.play_state == PlayState::Pause {
                    let end_tick = state
                        .animation_data
                        .as_ref()
                        .map(|a| a.metadata.end_tick)
                        .unwrap_or(0);
                    if end_tick > 0 {
                        if state.playback.playhead >= end_tick {
                            state.playback.playhead = 0;
                        }
                        state.playback.set_play_state(PlayState::Play);
                    }
                } else {
                    state.playback.set_play_state(PlayState::Pause);
                }
            }
            if !ctrl && i.consume_key(egui::Modifiers::NONE, egui::Key::R) {
                state.viewport_dragger.tool = match state.viewport_dragger.tool {
                    ToolMode::Rotate => ToolMode::Move,
                    ToolMode::Move => ToolMode::Rotate,
                };
            }
            if ctrl && i.consume_key(egui::Modifiers::COMMAND, egui::Key::L) {
                state.viewport_dragger.world_space = !state.viewport_dragger.world_space;
            }
            if ctrl && shift && i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z) {
                crate::anim_editor::history::redo(state);
                action.pose_changed = true;
            } else if ctrl && i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z) {
                crate::anim_editor::history::undo(state);
                action.pose_changed = true;
            } else if ctrl && i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y) {
                crate::anim_editor::history::redo(state);
                action.pose_changed = true;
            }
        });
    }

    let mut active_drag: Option<ActiveGizmoDrag> = ctx.data_mut(|d| d.get_temp(drag_session_id()));

    // ------------------------------------------------------------------------
    // 1. Collect bones & compute gizmo pivot for the last selected joint/bone
    // ------------------------------------------------------------------------
    let mut selected_names = resolve_selected_joint_names(doc, state);
    let mut selected_set: HashSet<String> = selected_names.iter().cloned().collect();
    let local_space = !state.viewport_dragger.world_space;

    let bones = if has_active_clip && state.settings.visualize_bones {
        rig_viz::collect_bone_visuals(doc, &state.rig_cache)
    } else {
        Vec::new()
    };

    let gizmo_pivot = if has_active_clip {
        selected_names
            .last()
            .and_then(|name| compute_joint_pivot(doc, scene, &state.rig_cache, name, local_space))
    } else {
        None
    };

    let mut handles = gizmo_pivot
        .as_ref()
        .map(|piv| build_gizmo_handles(rect, cam, piv, state.viewport_dragger.tool))
        .unwrap_or_default();

    let hovered_handle = if active_drag.is_none() && in_rect {
        pointer_pos.and_then(|p| hit_test_handles(&handles, p))
    } else {
        None
    };

    // ------------------------------------------------------------------------
    // 2. Active gizmo drag update / end
    // ------------------------------------------------------------------------
    if let Some(mut drag) = active_drag.take() {
        action.consumed_pointer = true;

        if primary_down {
            if let Some(p) = pointer_pos {
                let ray = cam.ray(rect, p);
                // Shift temporarily disables snapping (`DraggerContext_PluginImpl`).
                let disable_snap = modifiers.shift;
                let global_delta = match drag.handle.tool {
                    ToolMode::Move => {
                        let dir = drag.pivot.rot.col(drag.handle.axis) * drag.handle.sign;
                        let raw_d = ray
                            .closest_on_line(drag.pivot.pos, dir)
                            .map(|t| t - drag.start_param)
                            .unwrap_or(drag.amount);
                        let inc = if disable_snap {
                            0.0
                        } else {
                            snap_move.unwrap_or(0.0)
                        };
                        let d = snap(raw_d, inc);
                        drag.amount = d;
                        Some(Xform {
                            pos: dir * d,
                            rot: Mat3::IDENTITY,
                        })
                    }
                    ToolMode::Rotate => {
                        let a = (p.y - drag.center_px.y).atan2(p.x - drag.center_px.x);
                        let pi = std::f32::consts::PI;
                        drag.turned_rad += (a - drag.last_angle + pi).rem_euclid(2.0 * pi) - pi;
                        drag.last_angle = a;

                        let axis = drag.pivot.rot.col(drag.handle.axis);
                        let facing = axis.dot(cam.pos - drag.pivot.pos) > 0.0;
                        let mut deg = drag.turned_rad.to_degrees();
                        if facing {
                            deg = -deg;
                        }
                        let inc = if disable_snap {
                            0.0
                        } else {
                            snap_rotate_deg.unwrap_or(0.0)
                        };
                        let snapped_deg = snap(deg, inc);
                        drag.amount = snapped_deg;
                        let rad = snapped_deg.to_radians();
                        let rot = Mat3::from_axis_angle(axis.normalize_or(Vec3::X), rad);
                        let pivot_pos = drag.pivot.pos;
                        // pivot * R * pivot^-1
                        Some(Xform {
                            pos: pivot_pos - rot * pivot_pos,
                            rot,
                        })
                    }
                };

                if let Some(delta) = global_delta {
                    if !state.read_only && state.playback.play_state == PlayState::Pause {
                        // Add ONE undo waypoint on drag start/first move.
                        if !state.viewport_dragger.has_drag_waypoint {
                            state.add_waypoint();
                            state.viewport_dragger.has_drag_waypoint = true;
                            action.commit_waypoint = true;
                        }

                        let rig_scale = state.rig_cache.rig_scale;

                        if state.ik.enabled {
                            if let (Some(effector_name), Some(orig_eff)) =
                                (drag.dragged_joints.last(), drag.orig_effector_world)
                            {
                                let target_world = cf_mul(&delta, &orig_eff);
                                let solved = crate::anim_editor::ik::solve_ik_drag(
                                    doc,
                                    &state.rig_cache,
                                    &state.ik,
                                    effector_name,
                                    target_world,
                                    drag.handle.tool == ToolMode::Rotate,
                                );
                                let scale = if rig_scale.abs() > 1e-6 {
                                    rig_scale
                                } else {
                                    1.0
                                };
                                if let Some(solved) = solved {
                                    for (j_name, mut xf) in solved.joint_transforms {
                                        xf.pos /= scale;
                                        commit_joint_transform(
                                            state,
                                            &j_name,
                                            xf.orthonormalized(),
                                        );
                                        action.pose_changed = true;
                                    }
                                }
                            }
                        } else {
                            // FK drag: Motor6D, AnimationConstraint, and Bone.
                            let joint_names = drag.dragged_joints.clone();
                            for j_name in joint_names {
                                if let Some(joint) =
                                    state.rig_cache.joints_by_part.get(&j_name).cloned()
                                {
                                    match joint.kind {
                                        RigJointKind::Motor6D
                                        | RigJointKind::AnimationConstraint => {
                                            let Some(part0_ref) = joint.part0_ref else {
                                                continue;
                                            };
                                            let Some(orig_p1) =
                                                drag.orig_part1_cframes.get(&j_name).copied()
                                            else {
                                                continue;
                                            };
                                            let part0_world = if doc
                                                .class_of(part0_ref)
                                                .is_some_and(|c| c == "Bone")
                                            {
                                                rig_viz::bone_transformed_world_cframe(
                                                    doc, part0_ref,
                                                )
                                            } else {
                                                part_world_xform(doc, scene, part0_ref)
                                                    .unwrap_or(IDENTITY_XFORM)
                                            };
                                            let new_tf = fk_motor6d_transform(
                                                part0_world,
                                                joint.c0,
                                                orig_p1,
                                                joint.c1,
                                                delta,
                                                rig_scale,
                                            );
                                            commit_joint_transform(state, &j_name, new_tf);
                                            action.pose_changed = true;
                                        }
                                        RigJointKind::Bone => {
                                            let Some(snap_b) =
                                                drag.orig_bone_snapshots.get(&j_name).cloned()
                                            else {
                                                continue;
                                            };
                                            let parent_world = rig_viz::bone_parent_world_cframe(
                                                doc,
                                                joint.joint_ref,
                                            );
                                            let new_tf = fk_bone_transform(
                                                parent_world,
                                                snap_b.orig_bone_cframe,
                                                snap_b.orig_transformed_world,
                                                delta,
                                                rig_scale,
                                            );
                                            commit_joint_transform(state, &j_name, new_tf);
                                            action.pose_changed = true;
                                        }
                                    }
                                } else if let Some(&bone_ref) =
                                    state.rig_cache.bone_map.get(&j_name)
                                {
                                    if let Some(snap_b) =
                                        drag.orig_bone_snapshots.get(&j_name).cloned()
                                    {
                                        let parent_world =
                                            rig_viz::bone_parent_world_cframe(doc, bone_ref);
                                        let new_tf = fk_bone_transform(
                                            parent_world,
                                            snap_b.orig_bone_cframe,
                                            snap_b.orig_transformed_world,
                                            delta,
                                            rig_scale,
                                        );
                                        commit_joint_transform(state, &j_name, new_tf);
                                        action.pose_changed = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if !primary_released {
                state.viewport_dragger.dragging = true;
                active_drag = Some(drag);
            } else {
                state.viewport_dragger.dragging = false;
                state.viewport_dragger.has_drag_waypoint = false;
                state.viewport_dragger.drag_start_transforms.clear();
            }
        } else {
            state.viewport_dragger.dragging = false;
            state.viewport_dragger.has_drag_waypoint = false;
            state.viewport_dragger.drag_start_transforms.clear();
        }
    } else if in_rect && (resp.hovered() || primary_pressed) {
        // --------------------------------------------------------------------
        // 3. Not currently dragging: hover & click-picking / drag start
        // --------------------------------------------------------------------
        let Some(p) = pointer_pos else {
            ctx.data_mut(|d| d.remove::<ActiveGizmoDrag>(drag_session_id()));
            return action;
        };
        let ray = cam.ray(rect, p);

        // 3a. Starting a gizmo handle drag?
        if let (Some(handle), Some(pivot)) = (hovered_handle, gizmo_pivot) {
            action.consumed_pointer = true;
            if primary_pressed && !state.read_only && state.playback.play_state == PlayState::Pause
            {
                let dir = pivot.rot.col(handle.axis) * handle.sign;
                let start_param = ray.closest_on_line(pivot.pos, dir).unwrap_or(0.0);
                let center_px = cam.project(rect, pivot.pos).unwrap_or(p);
                let last_angle = (p.y - center_px.y).atan2(p.x - center_px.x);

                let mut orig_part1_cframes = HashMap::new();
                let mut orig_bone_snapshots = HashMap::new();
                state.viewport_dragger.drag_start_transforms.clear();

                for name in &selected_names {
                    if let Some(joint) = state.rig_cache.joints_by_part.get(name) {
                        match joint.kind {
                            RigJointKind::Motor6D | RigJointKind::AnimationConstraint => {
                                if let Some(p1_ref) = joint.part1_ref {
                                    if let Some(xf) = part_world_xform(doc, scene, p1_ref) {
                                        orig_part1_cframes.insert(name.clone(), xf);
                                        state
                                            .viewport_dragger
                                            .drag_start_transforms
                                            .insert(name.clone(), xf);
                                    }
                                }
                            }
                            RigJointKind::Bone => {
                                let orig_bone_cframe =
                                    rig_viz::read_cframe_prop(doc, joint.joint_ref, "CFrame");
                                let orig_transformed_world =
                                    rig_viz::bone_transformed_world_cframe(doc, joint.joint_ref);
                                orig_bone_snapshots.insert(
                                    name.clone(),
                                    BoneDragSnapshot {
                                        orig_bone_cframe,
                                        orig_transformed_world,
                                    },
                                );
                                state
                                    .viewport_dragger
                                    .drag_start_transforms
                                    .insert(name.clone(), orig_transformed_world);
                            }
                        }
                    } else if let Some(&bone_ref) = state.rig_cache.bone_map.get(name) {
                        let orig_bone_cframe = rig_viz::read_cframe_prop(doc, bone_ref, "CFrame");
                        let orig_transformed_world =
                            rig_viz::bone_transformed_world_cframe(doc, bone_ref);
                        orig_bone_snapshots.insert(
                            name.clone(),
                            BoneDragSnapshot {
                                orig_bone_cframe,
                                orig_transformed_world,
                            },
                        );
                        state
                            .viewport_dragger
                            .drag_start_transforms
                            .insert(name.clone(), orig_transformed_world);
                    }
                }

                let orig_effector_world = selected_names.last().and_then(|name| {
                    orig_part1_cframes.get(name).copied().or_else(|| {
                        orig_bone_snapshots
                            .get(name)
                            .map(|b| b.orig_transformed_world)
                    })
                });

                // Push ONE undo waypoint at drag start (`TransformHandlesImplementation:beginDrag`).
                state.add_waypoint();
                state.viewport_dragger.has_drag_waypoint = true;
                state.viewport_dragger.dragging = true;
                action.commit_waypoint = true;

                active_drag = Some(ActiveGizmoDrag {
                    handle,
                    pivot,
                    start_param,
                    last_angle,
                    turned_rad: 0.0,
                    center_px,
                    orig_part1_cframes,
                    orig_bone_snapshots,
                    dragged_joints: selected_names.clone(),
                    orig_effector_world,
                    amount: 0.0,
                });
            }
        } else {
            // 3b. Hover & click picking for rig candidates (`InstanceSelector`)
            // and rig joints/bones (`DraggerSchema.getMouseTarget`).
            let candidate_rig = pick_rig_candidate(doc, scene, &ray, state.rig_cache.root_ref);

            if let Some(cand_ref) = candidate_rig {
                // Hovering another valid rig Model (or any rig when no clip is active).
                rig_viz::draw_rig_hover_box(painter, rect, cam, scene, doc, cand_ref);
                state.viewport_dragger.hovered_part =
                    doc.dom.get_by_ref(cand_ref).map(|inst| inst.name.clone());

                if primary_pressed && !state.ik.is_selecting_target {
                    action.attach_rig = Some(cand_ref);
                    action.consumed_pointer = true;
                }
            } else if has_active_clip {
                let picked_joint = pick_rig_joint(
                    doc,
                    scene,
                    &state.rig_cache,
                    &ray,
                    state.settings.visualize_bones,
                );
                state.viewport_dragger.hovered_part = picked_joint.clone();

                if primary_pressed {
                    let additive = modifiers.ctrl || modifiers.command || modifiers.shift;
                    if let Some(joint_name) = picked_joint {
                        select_joint_in_state(state, &joint_name, additive);
                        // Refresh selected names & gizmo handles immediately on the click frame.
                        selected_names = resolve_selected_joint_names(doc, state);
                        selected_set = selected_names.iter().cloned().collect();
                        if let Some(new_piv) = selected_names.last().and_then(|name| {
                            compute_joint_pivot(doc, scene, &state.rig_cache, name, local_space)
                        }) {
                            handles = build_gizmo_handles(
                                rect,
                                cam,
                                &new_piv,
                                state.viewport_dragger.tool,
                            );
                        } else {
                            handles.clear();
                        }
                        ctx.request_repaint();
                        action.consumed_pointer = true;
                    } else if !additive {
                        state.selected_tracks.clear();
                        state.selected_track_instances.clear();
                        selected_names.clear();
                        selected_set.clear();
                        handles.clear();
                        ctx.request_repaint();
                        action.consumed_pointer = true;
                    }
                }
            } else {
                state.viewport_dragger.hovered_part = None;
            }
        }
    } else {
        state.viewport_dragger.hovered_part = None;
    }

    // ------------------------------------------------------------------------
    // 4. Render viewport overlays: SelectionBoxes, Microbones, Gizmos, HUD Pill
    // ------------------------------------------------------------------------
    let rig_opt = has_active_clip.then_some(&state.rig_cache);
    rig_viz::draw_selection_boxes(painter, rect, cam, scene, doc, rig_opt, state);

    if !bones.is_empty() {
        rig_viz::draw_bone_visualization(
            painter,
            rect,
            cam,
            &bones,
            &selected_set,
            state.viewport_dragger.hovered_part.as_deref(),
        );
    }

    if !handles.is_empty() {
        draw_gizmo_handles(
            painter,
            &handles,
            hovered_handle,
            active_drag.as_ref().map(|d| d.handle),
        );
    }

    if let Some(ref drag) = active_drag {
        let pivot_px = cam.project(rect, drag.pivot.pos);
        draw_drag_readout_pill(painter, rect, pivot_px, drag.handle.tool, drag.amount);
        ctx.data_mut(|d| d.insert_temp(drag_session_id(), drag.clone()));
    } else {
        ctx.data_mut(|d| d.remove::<ActiveGizmoDrag>(drag_session_id()));
    }

    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom_weak::{InstanceBuilder, WeakDom};

    use crate::anim_editor::state::RigJointInfo;

    fn approx_vec3(a: Vec3, b: Vec3, eps: f32) {
        assert!(
            (a - b).length() <= eps,
            "expected {b:?}, got {a:?} (diff {})",
            (a - b).length()
        );
    }

    fn approx_xform(a: Xform, b: Xform, eps: f32) {
        approx_vec3(a.pos, b.pos, eps);
        approx_vec3(a.rot.x_axis, b.rot.x_axis, eps);
        approx_vec3(a.rot.y_axis, b.rot.y_axis, eps);
        approx_vec3(a.rot.z_axis, b.rot.z_axis, eps);
    }

    #[test]
    fn test_compute_joint_pivot_motor6d_and_bone_local_vs_world() {
        let mut dom = WeakDom::new(InstanceBuilder::new("DataModel"));
        let ws = dom.insert(dom.root_ref(), InstanceBuilder::new("Workspace"));
        let model = dom.insert(ws, InstanceBuilder::new("Rig"));

        let part0_xf = Xform::from_orientation_deg(Vec3::new(1.0, 2.0, 3.0), Vec3::ZERO);
        let part1_xf =
            Xform::from_orientation_deg(Vec3::new(4.0, 5.0, 6.0), Vec3::new(0.0, 90.0, 0.0));
        let c1 = Xform::from_orientation_deg(Vec3::new(0.0, -1.0, 0.0), Vec3::new(0.0, 0.0, 45.0));

        let part0 = dom.insert(
            model,
            InstanceBuilder::new("Part")
                .with_name("Torso")
                .with_property("CFrame", part0_xf.to_cframe()),
        );
        let part1 = dom.insert(
            model,
            InstanceBuilder::new("Part")
                .with_name("Arm")
                .with_property("CFrame", part1_xf.to_cframe()),
        );
        let motor = dom.insert(part0, InstanceBuilder::new("Motor6D").with_name("Shoulder"));

        let bone_local =
            Xform::from_orientation_deg(Vec3::new(0.0, 2.0, 0.0), Vec3::new(30.0, 0.0, 0.0));
        let bone_tf =
            Xform::from_orientation_deg(Vec3::new(0.5, 0.0, 0.0), Vec3::new(0.0, 45.0, 0.0));
        let bone = dom.insert(
            part0,
            InstanceBuilder::new("Bone")
                .with_name("Spine")
                .with_property("CFrame", bone_local.to_cframe())
                .with_property("Transform", bone_tf.to_cframe()),
        );

        let doc = Document::from_play_dom(dom, "test".to_string());
        let scene = Scene::default();
        let mut rig = RigCache::default();
        rig.rig_scale = 1.0;
        rig.joints_by_part.insert(
            "Arm".to_string(),
            RigJointInfo {
                name: "Shoulder".to_string(),
                kind: RigJointKind::Motor6D,
                joint_ref: motor,
                part0_name: Some("Torso".to_string()),
                part1_name: "Arm".to_string(),
                part0_ref: Some(part0),
                part1_ref: Some(part1),
                c0: IDENTITY_XFORM,
                c1,
            },
        );
        rig.bone_map.insert("Spine".to_string(), bone);

        // Motor6D Local = Part1.CFrame * C1
        let expected_motor_local = cf_mul(&part1_xf, &c1);
        let got_motor_local = compute_joint_pivot(&doc, &scene, &rig, "Arm", true).unwrap();
        approx_xform(got_motor_local, expected_motor_local, 1e-4);

        // Motor6D World = translation-only at (Part1.CFrame * C1).pos
        let got_motor_world = compute_joint_pivot(&doc, &scene, &rig, "Arm", false).unwrap();
        approx_xform(
            got_motor_world,
            Xform {
                pos: expected_motor_local.pos,
                rot: Mat3::IDENTITY,
            },
            1e-4,
        );

        // Bone Local = parentWorld * Bone.CFrame * Bone.Transform
        let expected_bone_local = cf_mul(&cf_mul(&part0_xf, &bone_local), &bone_tf);
        let got_bone_local = compute_joint_pivot(&doc, &scene, &rig, "Spine", true).unwrap();
        approx_xform(got_bone_local, expected_bone_local, 1e-4);

        // Bone World = translation-only at TransformedWorldCFrame.pos
        let got_bone_world = compute_joint_pivot(&doc, &scene, &rig, "Spine", false).unwrap();
        approx_xform(
            got_bone_world,
            Xform {
                pos: expected_bone_local.pos,
                rot: Mat3::IDENTITY,
            },
            1e-4,
        );
    }

    #[test]
    fn test_fk_motor6d_matrix_formula() {
        let part0 =
            Xform::from_orientation_deg(Vec3::new(2.0, 3.0, 0.0), Vec3::new(0.0, 90.0, 0.0));
        let c0 = Xform::from_orientation_deg(Vec3::new(0.0, 1.0, 0.0), Vec3::ZERO);
        let c1 = Xform::from_orientation_deg(Vec3::new(0.0, -0.5, 0.0), Vec3::ZERO);

        // Start from an initial Transform of identity -> orig_part1 * c1 == part0 * c0.
        let orig_part1 = cf_mul(&cf_mul(&part0, &c0), &cf_inv(&c1));

        // Apply a 45-degree Y rotation around the joint pivot (part0 * c0).pos.
        let pivot = cf_mul(&part0, &c0).pos;
        let rot = Mat3::from_rotation_y(45.0_f32.to_radians());
        let global_delta = Xform {
            pos: pivot - rot * pivot,
            rot,
        };

        let tf = fk_motor6d_transform(part0, c0, orig_part1, c1, global_delta, 1.0);
        // Rotating around the joint pivot produces zero translation and +45° Y rotation.
        approx_vec3(tf.pos, Vec3::ZERO, 1e-4);
        approx_vec3(tf.rot.x_axis, rot.x_axis, 1e-4);
        approx_vec3(tf.rot.y_axis, rot.y_axis, 1e-4);
        approx_vec3(tf.rot.z_axis, rot.z_axis, 1e-4);
    }

    #[test]
    fn test_fk_bone_matrix_formula() {
        let parent_world =
            Xform::from_orientation_deg(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 90.0, 0.0));
        let orig_bone_cf = Xform::from_orientation_deg(Vec3::new(0.0, 1.0, 0.0), Vec3::ZERO);
        let orig_transformed_world = cf_mul(&parent_world, &orig_bone_cf);

        // Pure world translation along +X by 2.0 studs.
        let global_delta = Xform {
            pos: Vec3::new(2.0, 0.0, 0.0),
            rot: Mat3::IDENTITY,
        };

        let tf = fk_bone_transform(
            parent_world,
            orig_bone_cf,
            orig_transformed_world,
            global_delta,
            1.0,
        );

        // Verify roundtrip: parent_world * orig_bone_cf * tf == global_delta * orig_transformed_world
        let reconstructed = cf_mul(&cf_mul(&parent_world, &orig_bone_cf), &tf);
        let expected = cf_mul(&global_delta, &orig_transformed_world);
        approx_xform(reconstructed, expected, 1e-4);
    }

    #[test]
    fn test_model_get_scale_division() {
        let parent = IDENTITY_XFORM;
        let c0 = IDENTITY_XFORM;
        let orig_part1 = IDENTITY_XFORM;
        let c1 = IDENTITY_XFORM;
        let global_delta = Xform {
            pos: Vec3::new(4.0, -6.0, 2.0),
            rot: Mat3::from_rotation_z(30.0_f32.to_radians()),
        };

        let tf_motor = fk_motor6d_transform(parent, c0, orig_part1, c1, global_delta, 2.0);
        approx_vec3(tf_motor.pos, Vec3::new(2.0, -3.0, 1.0), 1e-4);
        // Rotation is NOT scaled.
        approx_vec3(tf_motor.rot.x_axis, global_delta.rot.x_axis, 1e-4);

        let tf_bone = fk_bone_transform(parent, IDENTITY_XFORM, IDENTITY_XFORM, global_delta, 4.0);
        approx_vec3(tf_bone.pos, Vec3::new(1.0, -1.5, 0.5), 1e-4);
        approx_vec3(tf_bone.rot.y_axis, global_delta.rot.y_axis, 1e-4);
    }
}

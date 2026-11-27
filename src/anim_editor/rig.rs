//! Studio-matched Rig Detection, Validation, Caching, Posing, Edit-Mode FK,
//! and `AnimSaves` / `ServerStorage.RBX_ANIMSAVES` management for RusTudio's
//! native `AnimationClipEditor` (derived from `Util/RigInfo`,
//! `Util/RigUtils`, and `Util/RigValidation`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use glam::{Mat3, Vec3};
use rbx_dom_weak::types::{CFrame, Ref, Tags, Variant};
use rbx_dom_weak::{ustr, InstanceBuilder};
use rustudio_core::body::{is_rigid_joint, joint_enabled, joint_frames, joint_parts};
use rustudio_core::doc::{self, Document};
use rustudio_core::math::{cf_inv, cf_mul, Xform};

use crate::anim_editor::constants::{ANIMSAVES_NAME, FACS_CONTROLS, RBX_ANIMSAVES_NAME};
use crate::anim_editor::curves::evaluate_track_value;
use crate::anim_editor::model::{AnimationData, Tick, TrackType};
use crate::anim_editor::serialization;
use crate::anim_editor::state::{
    MigrationChoice, RigAnimSaveEntry, RigCache, RigJointInfo, RigJointKind, RigTypeKind,
    RigValidationIssue, ValidationSeverity,
};

const IDENTITY_XFORM: Xform = Xform {
    pos: Vec3::ZERO,
    rot: Mat3::IDENTITY,
};

// ============================================================================
// Internal DOM Helpers
// ============================================================================

fn is_anim_saves_container(class: &str, name: &str) -> bool {
    name == ANIMSAVES_NAME
        && (doc::is_a(class, "Model")
            || class == "ObjectValue"
            || class == "Folder"
            || class == "Configuration")
}

/// Collects all descendants of `root` skipping any `AnimSaves` container
/// (`RigInfo.getDescendants`).
fn collect_rig_descendants(doc: &Document, root: Ref) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut stack = Vec::new();
    if let Some(inst) = doc.dom.get_by_ref(root) {
        for &c in inst.children().iter().rev() {
            stack.push(c);
        }
    }
    while let Some(r) = stack.pop() {
        let Some(inst) = doc.dom.get_by_ref(r) else {
            continue;
        };
        if is_anim_saves_container(inst.class.as_str(), &inst.name) {
            continue;
        }
        out.push(r);
        for &c in inst.children().iter().rev() {
            stack.push(c);
        }
    }
    out
}

fn find_direct_child_by_class(doc: &Document, parent: Ref, class: &str) -> Option<Ref> {
    let inst = doc.dom.get_by_ref(parent)?;
    inst.children().iter().copied().find(|&c| {
        doc.dom
            .get_by_ref(c)
            .is_some_and(|ci| ci.class.as_str() == class)
    })
}

fn find_direct_child_by_name(doc: &Document, parent: Ref, name: &str) -> Option<Ref> {
    let inst = doc.dom.get_by_ref(parent)?;
    inst.children()
        .iter()
        .copied()
        .find(|&c| doc.dom.get_by_ref(c).is_some_and(|ci| ci.name == name))
}

fn find_animation_controller(doc: &Document, model: Ref) -> Option<Ref> {
    find_direct_child_by_class(doc, model, "Humanoid")
        .or_else(|| find_direct_child_by_class(doc, model, "AnimationController"))
}

fn ref_prop(doc: &Document, r: Ref, name: &str) -> Option<Ref> {
    match doc.prop(r, name) {
        Some(Variant::Ref(x)) if x.is_some() && doc.dom.get_by_ref(x).is_some() => Some(x),
        _ => None,
    }
}

fn cframe_prop(doc: &Document, r: Ref, name: &str) -> Xform {
    match doc.prop(r, name) {
        Some(Variant::CFrame(cf)) => Xform::from_cframe(&cf),
        _ => IDENTITY_XFORM,
    }
}

fn bool_prop(doc: &Document, r: Ref, name: &str) -> bool {
    matches!(doc.prop(r, name), Some(Variant::Bool(true)))
}

fn is_base_part_or_attachment(doc: &Document, r: Ref) -> bool {
    doc.class_of(r).is_some_and(|c| {
        doc::is_a(c.as_str(), "BasePart")
            || doc::is_a(c.as_str(), "Attachment")
            || c.as_str() == "Bone"
    })
}

fn is_base_part(doc: &Document, r: Ref) -> bool {
    doc.class_of(r)
        .is_some_and(|c| doc::is_a(c.as_str(), "BasePart"))
}

fn is_bone(doc: &Document, r: Ref) -> bool {
    doc.class_of(r).is_some_and(|c| c.as_str() == "Bone")
}

/// `RigInfo.getPart0(constraint)` / `getPart1(constraint)`:
/// returns `Attachment{0,1}.Parent` iff it is a `BasePart` or `Attachment`/`Bone`.
fn anim_constraint_endpoint(
    doc: &Document,
    ac: Ref,
    attachment_prop: &str,
) -> Option<(Ref, Xform)> {
    let att_ref = ref_prop(doc, ac, attachment_prop)?;
    let att_inst = doc.dom.get_by_ref(att_ref)?;
    let parent_ref = att_inst.parent();
    if parent_ref.is_none() || !is_base_part_or_attachment(doc, parent_ref) {
        return None;
    }
    let att_cf = cframe_prop(doc, att_ref, "CFrame");
    Some((parent_ref, att_cf))
}

fn has_valid_parent(doc: &Document, r: Ref) -> bool {
    doc.dom
        .get_by_ref(r)
        .is_some_and(|i| i.parent().is_some() && doc.dom.get_by_ref(i.parent()).is_some())
}

fn is_r6_humanoid(doc: &Document, model: Ref) -> bool {
    let Some(hum) = find_direct_child_by_class(doc, model, "Humanoid") else {
        return false;
    };
    let rig_type = match doc.prop(hum, "RigType") {
        Some(Variant::Enum(e)) => e.to_u32(),
        _ => 0, // Default Enum.HumanoidRigType.R6 = 0
    };
    rig_type == 0
}

fn detect_rig_type(doc: &Document, model: Ref) -> (RigTypeKind, bool, Option<f32>) {
    if let Some(hum) = find_direct_child_by_class(doc, model, "Humanoid") {
        let rig_type = match doc.prop(hum, "RigType") {
            Some(Variant::Enum(e)) if e.to_u32() == 1 => RigTypeKind::R15,
            Some(Variant::Enum(e)) if e.to_u32() == 0 => RigTypeKind::R6,
            None => RigTypeKind::R6,
            _ => RigTypeKind::Custom,
        };
        let walkspeed = match doc.prop(hum, "WalkSpeed") {
            Some(Variant::Float32(v)) => Some(v),
            Some(Variant::Float64(v)) => Some(v as f32),
            _ => Some(16.0),
        };
        (rig_type, true, walkspeed)
    } else {
        (RigTypeKind::Custom, false, None)
    }
}

/// Exact `RigInfo.findRootPart(rig, motors, animConstraints, bones)`.
fn find_root_part(
    doc: &Document,
    model: Ref,
    descendants: &[Ref],
    motors: &[Ref],
    anim_constraints: &[Ref],
    bones: &[Ref],
) -> Option<Ref> {
    let primary = ref_prop(doc, model, "PrimaryPart").filter(|&r| is_base_part(doc, r));
    let hrp =
        find_direct_child_by_name(doc, model, "HumanoidRootPart").filter(|&r| is_base_part(doc, r));

    if is_r6_humanoid(doc, model) {
        if let Some(r) = hrp.or(primary) {
            return Some(r);
        }
    } else if let Some(r) = primary.or(hrp) {
        return Some(r);
    }

    // Fallback: walk up from Motor6D -> Bone -> AnimationConstraint chains.
    let mut part1_to_motor_part0: HashMap<String, String> = HashMap::new();
    for &m in motors {
        if let (Some(p0), Some(p1)) = (ref_prop(doc, m, "Part0"), ref_prop(doc, m, "Part1")) {
            if let (Some(i0), Some(i1)) = (doc.dom.get_by_ref(p0), doc.dom.get_by_ref(p1)) {
                part1_to_motor_part0
                    .entry(i1.name.clone())
                    .or_insert_with(|| i0.name.clone());
            }
        }
    }

    let mut bone_name_to_parent: HashMap<String, String> = HashMap::new();
    for &b in bones {
        if let Some(bi) = doc.dom.get_by_ref(b) {
            if let Some(pi) = doc.dom.get_by_ref(bi.parent()) {
                bone_name_to_parent
                    .entry(bi.name.clone())
                    .or_insert_with(|| pi.name.clone());
            }
        }
    }

    let mut part1_to_ac_part0: HashMap<String, String> = HashMap::new();
    for &ac in anim_constraints {
        if let (Some((p0, _)), Some((p1, _))) = (
            anim_constraint_endpoint(doc, ac, "Attachment0"),
            anim_constraint_endpoint(doc, ac, "Attachment1"),
        ) {
            if let (Some(i0), Some(i1)) = (doc.dom.get_by_ref(p0), doc.dom.get_by_ref(p1)) {
                part1_to_ac_part0
                    .entry(i1.name.clone())
                    .or_insert_with(|| i0.name.clone());
            }
        }
    }

    let walk_up = |map: &HashMap<String, String>| -> Option<String> {
        let mut cur = map.keys().next()?.clone();
        let mut visited = HashSet::new();
        while visited.insert(cur.clone()) {
            if let Some(next) = map.get(&cur) {
                cur = next.clone();
            } else {
                return Some(cur);
            }
        }
        None
    };

    let root_name = walk_up(&part1_to_motor_part0)
        .or_else(|| walk_up(&bone_name_to_parent))
        .or_else(|| walk_up(&part1_to_ac_part0))?;

    descendants.iter().copied().find(|&r| {
        is_base_part(doc, r) && doc.dom.get_by_ref(r).is_some_and(|i| i.name == root_name)
    })
}

/// Returns the path of instance names from child-of-rig down to the first
/// `FaceControls` instance (e.g. `["Head", "FaceControls"]`).
pub fn get_face_controls_path(doc: &Document, root_model: Ref) -> Vec<String> {
    let descendants = collect_rig_descendants(doc, root_model);
    let Some(fc_ref) = descendants.iter().copied().find(|&r| {
        doc.class_of(r)
            .is_some_and(|c| c.as_str() == "FaceControls")
    }) else {
        return Vec::new();
    };
    let mut path = Vec::new();
    let mut cur = fc_ref;
    while cur != root_model && cur.is_some() {
        let Some(inst) = doc.dom.get_by_ref(cur) else {
            break;
        };
        path.push(inst.name.clone());
        cur = inst.parent();
    }
    path.reverse();
    path
}

// ============================================================================
// Public Rig Candidate & Scale API
// ============================================================================

/// Returns `true` if `model` is a `Model` (not `Workspace`, not `AnimSaves`)
/// containing a `Humanoid` or `AnimationController`.
pub fn is_valid_rig_candidate(doc: &Document, model: Ref) -> bool {
    let Some(inst) = doc.dom.get_by_ref(model) else {
        return false;
    };
    if inst.class.as_str() != "Model" {
        return false;
    }
    if inst.name == ANIMSAVES_NAME || inst.name == RBX_ANIMSAVES_NAME {
        return false;
    }
    // Exclude models stored inside AnimSaves / RBX_ANIMSAVES.
    let mut cur = inst.parent();
    while let Some(p) = doc.dom.get_by_ref(cur) {
        if p.name == ANIMSAVES_NAME || p.name == RBX_ANIMSAVES_NAME {
            return false;
        }
        cur = p.parent();
    }
    find_animation_controller(doc, model).is_some()
}

/// Reads `Model.Scale` (matching `Model:GetScale()`), defaulting to `1.0`.
pub fn model_scale(doc: &Document, model: Ref) -> f32 {
    let s = match doc.prop(model, "Scale") {
        Some(Variant::Float32(v)) => v,
        Some(Variant::Float64(v)) => v as f32,
        _ => 1.0,
    };
    if s.is_finite() && s > 0.0 {
        s
    } else {
        1.0
    }
}

// ============================================================================
// Rig Validation (`Util/RigValidation` — all 9 ordered validators)
// ============================================================================

fn push_issue(
    issues: &mut Vec<RigValidationIssue>,
    id: &str,
    message: &str,
    severity: ValidationSeverity,
    mut details: Vec<String>,
) {
    details.sort();
    details.dedup();
    issues.push(RigValidationIssue {
        id: id.to_string(),
        message: message.to_string(),
        severity,
        details,
    });
}

fn has_errors(issues: &[RigValidationIssue]) -> bool {
    issues
        .iter()
        .any(|i| i.severity == ValidationSeverity::Error)
}

fn sort_issues(issues: &mut [RigValidationIssue]) {
    issues.sort_by(|a, b| {
        (b.severity as u8)
            .cmp(&(a.severity as u8))
            .then_with(|| a.id.cmp(&b.id))
    });
}

/// Runs all 9 ordered rig validation checks from `RigValidation`,
/// stopping after the first validator that emits an `Error` and sorting
/// results by severity descending then `id` ascending.
pub fn validate_rig(doc: &Document, model: Ref) -> Vec<RigValidationIssue> {
    let mut issues = Vec::new();
    let descendants = collect_rig_descendants(doc, model);

    let mut motors = Vec::new();
    let mut bones = Vec::new();
    let mut anim_constraints = Vec::new();
    let mut face_controls = Vec::new();

    for &r in &descendants {
        let Some(class) = doc.class_of(r) else {
            continue;
        };
        match class.as_str() {
            "Motor6D" => motors.push(r),
            "Bone" => bones.push(r),
            "AnimationConstraint" => anim_constraints.push(r),
            "FaceControls" => face_controls.push(r),
            _ => {}
        }
    }

    let root_part = find_root_part(doc, model, &descendants, &motors, &anim_constraints, &bones);

    // ------------------------------------------------------------------------
    // 1. Name collisions (parts, bones, and part/bone overlap)
    // ------------------------------------------------------------------------
    {
        let mut part_refs_by_name: BTreeMap<String, HashSet<Ref>> = BTreeMap::new();
        let mut register_part = |r: Ref| {
            if is_base_part(doc, r) {
                if let Some(inst) = doc.dom.get_by_ref(r) {
                    part_refs_by_name
                        .entry(inst.name.clone())
                        .or_default()
                        .insert(r);
                }
            }
        };
        if let Some(rp) = root_part {
            register_part(rp);
        }
        for &m in &motors {
            if let Some(p0) = ref_prop(doc, m, "Part0") {
                register_part(p0);
            }
            if let Some(p1) = ref_prop(doc, m, "Part1") {
                register_part(p1);
            }
        }
        for &ac in &anim_constraints {
            if let Some((p0, _)) = anim_constraint_endpoint(doc, ac, "Attachment0") {
                register_part(p0);
            }
            if let Some((p1, _)) = anim_constraint_endpoint(doc, ac, "Attachment1") {
                register_part(p1);
            }
        }

        let part_collisions: Vec<String> = part_refs_by_name
            .iter()
            .filter(|(_, refs)| refs.len() > 1)
            .map(|(name, _)| name.clone())
            .collect();
        if !part_collisions.is_empty() {
            push_issue(
                &mut issues,
                "NameCollision",
                "The following parts have the same name:",
                ValidationSeverity::Error,
                part_collisions,
            );
        }

        let mut bone_refs_by_name: BTreeMap<String, HashSet<Ref>> = BTreeMap::new();
        for &b in &bones {
            if let Some(inst) = doc.dom.get_by_ref(b) {
                bone_refs_by_name
                    .entry(inst.name.clone())
                    .or_default()
                    .insert(b);
            }
        }
        let bone_collisions: Vec<String> = bone_refs_by_name
            .iter()
            .filter(|(_, refs)| refs.len() > 1)
            .map(|(name, _)| name.clone())
            .collect();
        if !bone_collisions.is_empty() {
            push_issue(
                &mut issues,
                "BoneNameCollision",
                "The following bones have the same name:",
                ValidationSeverity::Error,
                bone_collisions,
            );
        }

        let part_bone_collisions: Vec<String> = part_refs_by_name
            .keys()
            .filter(|name| bone_refs_by_name.contains_key(*name))
            .cloned()
            .collect();
        if !part_bone_collisions.is_empty() {
            push_issue(
                &mut issues,
                "PartBoneNameCollision",
                "The following parts and bones share the same name:",
                ValidationSeverity::Error,
                part_bone_collisions,
            );
        }

        if has_errors(&issues) {
            sort_issues(&mut issues);
            return issues;
        }
    }

    // ------------------------------------------------------------------------
    // 2. Missing joints or missing root part
    // ------------------------------------------------------------------------
    {
        if motors.is_empty() && anim_constraints.is_empty() && bones.is_empty() {
            push_issue(
                &mut issues,
                "NoJoints",
                "The rig has no Motor6Ds, AnimationConstraints, or Bones.",
                ValidationSeverity::Error,
                Vec::new(),
            );
        }
        if root_part.is_none() {
            push_issue(
                &mut issues,
                "NoRoot",
                "The rig does not have a valid root part.",
                ValidationSeverity::Error,
                Vec::new(),
            );
        }
        if has_errors(&issues) {
            sort_issues(&mut issues);
            return issues;
        }
    }

    // ------------------------------------------------------------------------
    // 3. Missing Part0 / Part1 on Motor6D / AnimationConstraint
    // ------------------------------------------------------------------------
    {
        let mut missing_p0 = Vec::new();
        let mut missing_p1 = Vec::new();
        for &m in &motors {
            let name = doc
                .dom
                .get_by_ref(m)
                .map(|i| i.name.clone())
                .unwrap_or_default();
            if ref_prop(doc, m, "Part0").is_none_or(|p| !has_valid_parent(doc, p)) {
                missing_p0.push(name.clone());
            }
            if ref_prop(doc, m, "Part1").is_none_or(|p| !has_valid_parent(doc, p)) {
                missing_p1.push(name);
            }
        }
        if !missing_p0.is_empty() {
            push_issue(
                &mut issues,
                "MissingPart0_Migrated",
                "The following Motor6Ds do not have Part0 set:",
                ValidationSeverity::Error,
                missing_p0,
            );
        }
        if !missing_p1.is_empty() {
            push_issue(
                &mut issues,
                "MissingPart1_Migrated",
                "The following Motor6Ds do not have Part1 set:",
                ValidationSeverity::Error,
                missing_p1,
            );
        }

        let mut ac_missing_p0 = Vec::new();
        let mut ac_missing_p1 = Vec::new();
        for &ac in &anim_constraints {
            let name = doc
                .dom
                .get_by_ref(ac)
                .map(|i| i.name.clone())
                .unwrap_or_default();
            if anim_constraint_endpoint(doc, ac, "Attachment0")
                .is_none_or(|(p, _)| !has_valid_parent(doc, p))
            {
                ac_missing_p0.push(name.clone());
            }
            if anim_constraint_endpoint(doc, ac, "Attachment1")
                .is_none_or(|(p, _)| !has_valid_parent(doc, p))
            {
                ac_missing_p1.push(name);
            }
        }
        if !ac_missing_p0.is_empty() {
            push_issue(
                &mut issues,
                "AnimConstraintMissingPart0",
                "The following AnimationConstraints are missing Attachment0 or its parent:",
                ValidationSeverity::Error,
                ac_missing_p0,
            );
        }
        if !ac_missing_p1.is_empty() {
            push_issue(
                &mut issues,
                "AnimConstraintMissingPart1",
                "The following AnimationConstraints are missing Attachment1 or its parent:",
                ValidationSeverity::Error,
                ac_missing_p1,
            );
        }

        if has_errors(&issues) {
            sort_issues(&mut issues);
            return issues;
        }
    }

    // ------------------------------------------------------------------------
    // 4. Anchored non-root parts (skipped if rig has Bones)
    // ------------------------------------------------------------------------
    if bones.is_empty() {
        let mut non_root_parts: HashSet<Ref> = HashSet::new();
        for &m in &motors {
            if let Some(p0) = ref_prop(doc, m, "Part0") {
                if Some(p0) != root_part && is_base_part(doc, p0) {
                    non_root_parts.insert(p0);
                }
            }
            if let Some(p1) = ref_prop(doc, m, "Part1") {
                if Some(p1) != root_part && is_base_part(doc, p1) {
                    non_root_parts.insert(p1);
                }
            }
        }
        for &ac in &anim_constraints {
            if let Some((p0, _)) = anim_constraint_endpoint(doc, ac, "Attachment0") {
                if Some(p0) != root_part && is_base_part(doc, p0) {
                    non_root_parts.insert(p0);
                }
            }
            if let Some((p1, _)) = anim_constraint_endpoint(doc, ac, "Attachment1") {
                if Some(p1) != root_part && is_base_part(doc, p1) {
                    non_root_parts.insert(p1);
                }
            }
        }

        let mut anchored_names = Vec::new();
        let mut unanchored_count = 0usize;
        for part in &non_root_parts {
            if bool_prop(doc, *part, "Anchored") {
                if let Some(inst) = doc.dom.get_by_ref(*part) {
                    anchored_names.push(inst.name.clone());
                }
            } else {
                unanchored_count += 1;
            }
        }

        if !non_root_parts.is_empty() && unanchored_count == 0 {
            push_issue(
                &mut issues,
                "PartsAnchored",
                "All non-root parts in the rig are anchored:",
                ValidationSeverity::Error,
                anchored_names,
            );
            sort_issues(&mut issues);
            return issues;
        } else if !anchored_names.is_empty() {
            push_issue(
                &mut issues,
                "PartsAnchoredWarning",
                "The following non-root parts are anchored and may not animate as expected:",
                ValidationSeverity::Warning,
                anchored_names,
            );
        }
    }

    // Collect valid joint edges (joint_name, part0_ref, part1_ref) for checks 5, 6, 7.
    let descendant_set: HashSet<Ref> = descendants.iter().copied().collect();
    let mut joint_edges: Vec<(String, Ref, Ref)> = Vec::new();
    for &m in &motors {
        if let (Some(p0), Some(p1)) = (ref_prop(doc, m, "Part0"), ref_prop(doc, m, "Part1")) {
            let name = doc
                .dom
                .get_by_ref(m)
                .map(|i| i.name.clone())
                .unwrap_or_default();
            joint_edges.push((name, p0, p1));
        }
    }
    for &ac in &anim_constraints {
        if let (Some((p0, _)), Some((p1, _))) = (
            anim_constraint_endpoint(doc, ac, "Attachment0"),
            anim_constraint_endpoint(doc, ac, "Attachment1"),
        ) {
            let name = doc
                .dom
                .get_by_ref(ac)
                .map(|i| i.name.clone())
                .unwrap_or_default();
            joint_edges.push((name, p0, p1));
        }
    }

    // ------------------------------------------------------------------------
    // 5. Multiple parents for same part
    // ------------------------------------------------------------------------
    {
        let mut drivers_by_part1: HashMap<Ref, Vec<String>> = HashMap::new();
        for (jname, p0, p1) in &joint_edges {
            if descendant_set.contains(p0) && descendant_set.contains(p1) {
                drivers_by_part1.entry(*p1).or_default().push(jname.clone());
            }
        }
        let mut details = Vec::new();
        for (p1, mut joints) in drivers_by_part1 {
            if joints.len() > 1 {
                joints.sort();
                let part_name = doc
                    .dom
                    .get_by_ref(p1)
                    .map(|i| i.name.as_str())
                    .unwrap_or("?");
                details.push(format!("{part_name} ({})", joints.join(", ")));
            }
        }
        if !details.is_empty() {
            push_issue(
                &mut issues,
                "MultipleMotorsOrAnimConstraints",
                "The following parts are driven by multiple Motor6Ds or AnimationConstraints:",
                ValidationSeverity::Error,
                details,
            );
            sort_issues(&mut issues);
            return issues;
        }
    }

    // ------------------------------------------------------------------------
    // 6. Circular rig cycles
    // ------------------------------------------------------------------------
    {
        let mut parent_of: HashMap<Ref, Ref> = HashMap::new();
        let mut self_loops = Vec::new();
        for (_, p0, p1) in &joint_edges {
            if p0 == p1 {
                if let Some(inst) = doc.dom.get_by_ref(*p1) {
                    self_loops.push(format!("{} / {}", inst.name, inst.name));
                }
            } else {
                parent_of.entry(*p1).or_insert(*p0);
            }
        }

        let mut cycle_details: BTreeSet<String> = self_loops.into_iter().collect();
        let mut seen_cycle_sets: HashSet<Vec<String>> = HashSet::new();

        for &start in parent_of.keys() {
            let mut path: Vec<Ref> = Vec::new();
            let mut pos_in_path: HashMap<Ref, usize> = HashMap::new();
            let mut cur = start;
            while let Some(&next) = parent_of.get(&cur) {
                if let Some(&cycle_start) = pos_in_path.get(&cur) {
                    let cycle_refs = &path[cycle_start..];
                    let names: Vec<String> = cycle_refs
                        .iter()
                        .filter_map(|&r| doc.dom.get_by_ref(r).map(|i| i.name.clone()))
                        .collect();
                    let mut key = names.clone();
                    key.sort();
                    if !names.is_empty() && seen_cycle_sets.insert(key) {
                        cycle_details.insert(names.join(" / "));
                    }
                    break;
                }
                pos_in_path.insert(cur, path.len());
                path.push(cur);
                cur = next;
                if doc.class_of(cur).is_some_and(|c| c.as_str() == "Workspace") {
                    break;
                }
            }
        }

        if !cycle_details.is_empty() {
            push_issue(
                &mut issues,
                "CircularRig",
                "The rig contains circular joint dependencies:",
                ValidationSeverity::Error,
                cycle_details.into_iter().collect(),
            );
            sort_issues(&mut issues);
            return issues;
        }
    }

    // ------------------------------------------------------------------------
    // 7. Double joints between same unordered Part0 / Part1 pair
    // ------------------------------------------------------------------------
    {
        let mut pairs: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for (jname, p0, p1) in &joint_edges {
            let n0 = doc
                .dom
                .get_by_ref(*p0)
                .map(|i| i.name.clone())
                .unwrap_or_default();
            let n1 = doc
                .dom
                .get_by_ref(*p1)
                .map(|i| i.name.clone())
                .unwrap_or_default();
            let pair = if n0 <= n1 { (n0, n1) } else { (n1, n0) };
            pairs.entry(pair).or_default().push(jname.clone());
        }
        let mut details = Vec::new();
        for ((a, b), mut joints) in pairs {
            if joints.len() > 1 {
                joints.sort();
                details.push(format!("{a} - {b} ({})", joints.join(", ")));
            }
        }
        if !details.is_empty() {
            push_issue(
                &mut issues,
                "DoubleJoints",
                "Multiple joints connect the same pair of parts:",
                ValidationSeverity::Error,
                details,
            );
            sort_issues(&mut issues);
            return issues;
        }
    }

    // ------------------------------------------------------------------------
    // 8. Missing AnimationController / Humanoid
    // ------------------------------------------------------------------------
    {
        if find_animation_controller(doc, model).is_none() {
            push_issue(
                &mut issues,
                "NoAnimationController",
                "The rig must contain a Humanoid or AnimationController.",
                ValidationSeverity::Error,
                Vec::new(),
            );
            sort_issues(&mut issues);
            return issues;
        }
    }

    // ------------------------------------------------------------------------
    // 9. FaceControls count > 1 or invalid parent
    // ------------------------------------------------------------------------
    {
        if face_controls.len() > 1 {
            let names: Vec<String> = face_controls
                .iter()
                .filter_map(|&r| doc.dom.get_by_ref(r).map(|i| i.name.clone()))
                .collect();
            push_issue(
                &mut issues,
                "MultipleFaceControls",
                "The rig contains multiple FaceControls instances:",
                ValidationSeverity::Error,
                names,
            );
        }
        let mut invalid_parents = Vec::new();
        for &fc in &face_controls {
            let Some(inst) = doc.dom.get_by_ref(fc) else {
                continue;
            };
            let p = inst.parent();
            if !(is_base_part(doc, p) || is_bone(doc, p)) {
                invalid_parents.push(inst.name.clone());
            }
        }
        if !invalid_parents.is_empty() {
            push_issue(
                &mut issues,
                "FaceControlsParent",
                "FaceControls must be parented to a BasePart or Bone:",
                ValidationSeverity::Error,
                invalid_parents,
            );
        }
    }

    sort_issues(&mut issues);
    issues
}

// ============================================================================
// Rig Cache Construction (`RigInfo.getRigInfo` + `buildRigHierarchy`)
// ============================================================================

/// Builds a complete [`RigCache`] for `root_model`, detecting R15/R6/Custom,
/// root part, Motor6Ds, AnimationConstraints, Bones, FaceControls, hierarchy,
/// starting pose, model scale, `AnimSaves`, and validation issues.
pub fn build_rig_cache(doc: &Document, root_model: Ref) -> RigCache {
    let mut cache = RigCache::default();
    cache.rig_scale = model_scale(doc, root_model);

    let Some(root_inst) = doc.dom.get_by_ref(root_model) else {
        return cache;
    };
    cache.root_ref = Some(root_model);
    cache.root_name = root_inst.name.clone();

    let (rig_type, has_humanoid, walkspeed) = detect_rig_type(doc, root_model);
    cache.rig_type = rig_type;
    cache.has_humanoid = has_humanoid;
    cache.humanoid_walkspeed = walkspeed;

    let descendants = collect_rig_descendants(doc, root_model);
    let mut motors = Vec::new();
    let mut bones = Vec::new();
    let mut anim_constraints = Vec::new();

    for &r in &descendants {
        let Some(class) = doc.class_of(r) else {
            continue;
        };
        match class.as_str() {
            "Motor6D" => motors.push(r),
            "Bone" => bones.push(r),
            "AnimationConstraint" => anim_constraints.push(r),
            "FaceControls" if cache.face_controls_ref.is_none() => {
                cache.face_controls_ref = Some(r);
            }
            _ => {}
        }
    }

    let root_part = find_root_part(
        doc,
        root_model,
        &descendants,
        &motors,
        &anim_constraints,
        &bones,
    );
    cache.root_part_ref = root_part;
    if let Some(rp) = root_part {
        if let Some(rp_inst) = doc.dom.get_by_ref(rp) {
            cache.root_part_name = Some(rp_inst.name.clone());
            cache.part_refs.insert(rp_inst.name.clone(), rp);
            let cf = cframe_prop(doc, rp, "CFrame");
            cache.root_cframe = Some(cf);
            cache.starting_pose.insert(rp_inst.name.clone(), cf);
        }
    }

    // Register Motor6Ds
    for &m in &motors {
        let (Some(p0), Some(p1)) = (ref_prop(doc, m, "Part0"), ref_prop(doc, m, "Part1")) else {
            continue;
        };
        let (Some(i0), Some(i1)) = (doc.dom.get_by_ref(p0), doc.dom.get_by_ref(p1)) else {
            continue;
        };
        let p0_name = i0.name.clone();
        let p1_name = i1.name.clone();
        let c0 = cframe_prop(doc, m, "C0");
        let c1 = cframe_prop(doc, m, "C1");
        let joint_name = doc
            .dom
            .get_by_ref(m)
            .map(|i| i.name.clone())
            .unwrap_or_else(|| p1_name.clone());

        cache.motor6d_map.entry(p1_name.clone()).or_insert(m);
        cache.part_refs.entry(p0_name.clone()).or_insert(p0);
        cache.part_refs.entry(p1_name.clone()).or_insert(p1);
        cache
            .starting_pose
            .entry(p0_name.clone())
            .or_insert_with(|| cframe_prop(doc, p0, "CFrame"));
        cache
            .starting_pose
            .entry(p1_name.clone())
            .or_insert_with(|| cframe_prop(doc, p1, "CFrame"));

        cache
            .joints_by_part
            .entry(p1_name.clone())
            .or_insert(RigJointInfo {
                name: joint_name,
                kind: RigJointKind::Motor6D,
                joint_ref: m,
                part0_name: Some(p0_name.clone()),
                part1_name: p1_name.clone(),
                part0_ref: Some(p0),
                part1_ref: Some(p1),
                c0,
                c1,
            });
        cache.parent_map.entry(p1_name).or_insert(p0_name);
    }

    // Register AnimationConstraints
    for &ac in &anim_constraints {
        let (Some((p0, c0)), Some((p1, c1))) = (
            anim_constraint_endpoint(doc, ac, "Attachment0"),
            anim_constraint_endpoint(doc, ac, "Attachment1"),
        ) else {
            continue;
        };
        let (Some(i0), Some(i1)) = (doc.dom.get_by_ref(p0), doc.dom.get_by_ref(p1)) else {
            continue;
        };
        let p0_name = i0.name.clone();
        let p1_name = i1.name.clone();
        let joint_name = doc
            .dom
            .get_by_ref(ac)
            .map(|i| i.name.clone())
            .unwrap_or_else(|| p1_name.clone());

        cache
            .anim_constraint_map
            .entry(p1_name.clone())
            .or_insert(ac);
        cache.part_refs.entry(p0_name.clone()).or_insert(p0);
        cache.part_refs.entry(p1_name.clone()).or_insert(p1);
        if is_base_part(doc, p0) {
            cache
                .starting_pose
                .entry(p0_name.clone())
                .or_insert_with(|| cframe_prop(doc, p0, "CFrame"));
        }
        if is_base_part(doc, p1) {
            cache
                .starting_pose
                .entry(p1_name.clone())
                .or_insert_with(|| cframe_prop(doc, p1, "CFrame"));
        }

        cache
            .joints_by_part
            .entry(p1_name.clone())
            .or_insert(RigJointInfo {
                name: joint_name,
                kind: RigJointKind::AnimationConstraint,
                joint_ref: ac,
                part0_name: Some(p0_name.clone()),
                part1_name: p1_name.clone(),
                part0_ref: Some(p0),
                part1_ref: Some(p1),
                c0,
                c1,
            });
        cache.parent_map.entry(p1_name).or_insert(p0_name);
    }

    // Register Bones
    for &b in &bones {
        let Some(bi) = doc.dom.get_by_ref(b) else {
            continue;
        };
        let b_name = bi.name.clone();
        let p_ref = bi.parent();
        let p_name = doc.dom.get_by_ref(p_ref).map(|pi| pi.name.clone());

        cache.bone_map.entry(b_name.clone()).or_insert(b);
        cache.part_refs.entry(b_name.clone()).or_insert(b);
        let bone_cf = cframe_prop(doc, b, "CFrame");
        cache.starting_pose.entry(b_name.clone()).or_insert(bone_cf);

        cache
            .joints_by_part
            .entry(b_name.clone())
            .or_insert(RigJointInfo {
                name: b_name.clone(),
                kind: RigJointKind::Bone,
                joint_ref: b,
                part0_name: p_name.clone(),
                part1_name: b_name.clone(),
                part0_ref: p_ref.is_some().then_some(p_ref),
                part1_ref: Some(b),
                c0: bone_cf,
                c1: IDENTITY_XFORM,
            });
        if let Some(pn) = p_name {
            cache.parent_map.entry(b_name).or_insert(pn);
        }
    }

    // Build children_map and DFS hierarchy_order from root_part_name.
    for (child, parent) in &cache.parent_map {
        cache
            .children_map
            .entry(parent.clone())
            .or_default()
            .push(child.clone());
    }
    for children in cache.children_map.values_mut() {
        children.sort();
    }

    let mut visited = HashSet::new();
    if let Some(root_name) = cache.root_part_name.clone() {
        dfs_hierarchy(
            &root_name,
            &cache.children_map,
            &mut visited,
            &mut cache.hierarchy_order,
        );
    }
    // Append any disconnected joints deterministically so nothing is missed.
    let mut remaining: Vec<String> = cache
        .joints_by_part
        .keys()
        .filter(|k| !visited.contains(*k))
        .cloned()
        .collect();
    remaining.sort();
    for name in remaining {
        dfs_hierarchy(
            &name,
            &cache.children_map,
            &mut visited,
            &mut cache.hierarchy_order,
        );
    }

    // Scan AnimSaves & ServerStorage.RBX_ANIMSAVES (already sorted newest-first)
    let (all_saves, _has_legacy) = list_anim_saves(doc, root_model);
    for entry in all_saves {
        if entry.is_legacy_anim_saves {
            cache.legacy_anim_saves.push(entry);
        } else {
            cache.anim_saves.push(entry);
        }
    }
    sort_anim_saves_latest(&mut cache.anim_saves);
    sort_anim_saves_latest(&mut cache.legacy_anim_saves);

    cache.validation_issues = validate_rig(doc, root_model);
    cache
}

fn dfs_hierarchy(
    node: &str,
    children_map: &HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
    order: &mut Vec<String>,
) {
    if !visited.insert(node.to_string()) {
        return;
    }
    order.push(node.to_string());
    if let Some(children) = children_map.get(node) {
        for child in children {
            dfs_hierarchy(child, children_map, visited, order);
        }
    }
}

// ============================================================================
// Posing, Forward Kinematics & Edit-Mode Viewport Sync
// ============================================================================

fn write_prop_live(doc: &mut Document, r: Ref, key: &str, value: Variant) {
    let u = ustr(key);
    if let Some(inst) = doc.dom.get_by_ref_mut(r) {
        inst.properties.insert(u, value);
    }
    doc.mark_prop(r, u);
}

/// Solves forward kinematics down the rig's Motor6D / AnimationConstraint
/// hierarchy AND updates rigidly welded / accessory parts so the Edit-mode
/// 3D viewport animates in real time without touching the place undo stack.
fn solve_edit_mode_fk(
    doc: &mut Document,
    rig: &RigCache,
    root_world: Xform,
    transforms: &HashMap<String, Xform>,
) {
    let mut world_cframes: HashMap<Ref, Xform> = HashMap::new();

    if let Some(rp) = rig.root_part_ref {
        world_cframes.insert(rp, root_world);
        write_prop_live(doc, rp, "CFrame", Variant::CFrame(root_world.to_cframe()));
    }

    // 1. Solve Motor6D & AnimationConstraint chain top-down.
    for part_name in &rig.hierarchy_order {
        let Some(joint) = rig.joints_by_part.get(part_name) else {
            continue;
        };
        if joint.kind == RigJointKind::Bone {
            continue;
        }
        let (Some(p0_ref), Some(p1_ref)) = (joint.part0_ref, joint.part1_ref) else {
            continue;
        };
        if !is_base_part(doc, p1_ref) {
            continue;
        }
        let p0_cf = world_cframes
            .get(&p0_ref)
            .copied()
            .unwrap_or_else(|| cframe_prop(doc, p0_ref, "CFrame"));
        let transform = transforms.get(part_name).copied().unwrap_or(IDENTITY_XFORM);

        // Part1.CFrame = Part0.CFrame * C0 * Transform * C1^-1
        let p1_cf = cf_mul(
            &cf_mul(&cf_mul(&p0_cf, &joint.c0), &transform),
            &cf_inv(&joint.c1),
        );
        world_cframes.insert(p1_ref, p1_cf);
        write_prop_live(doc, p1_ref, "CFrame", Variant::CFrame(p1_cf.to_cframe()));
    }

    // 2. Propagate rigidly welded / accessory parts attached to solved body parts.
    let Some(root_model) = rig.root_ref else {
        return;
    };
    let descendants = collect_rig_descendants(doc, root_model);

    enum RigidEdge {
        /// `Part1 = Part0 * C0 * C1^-1`
        Frame {
            p0: Ref,
            p1: Ref,
            c0: Xform,
            c1: Xform,
        },
        /// `WeldConstraint`: `Part1 = Part0 * rel`
        WeldConstraint { p0: Ref, p1: Ref, rel: Xform },
    }

    let mut edges = Vec::new();
    for &r in &descendants {
        let Some(class) = doc.class_of(r) else {
            continue;
        };
        let cs = class.as_str();
        if cs == "Motor6D" || cs == "AnimationConstraint" || !is_rigid_joint(cs) {
            continue;
        }
        if !joint_enabled(doc, r) {
            continue;
        }
        if cs == "WeldConstraint" {
            if let Some((p0, p1)) = joint_parts(doc, r) {
                if !is_base_part(doc, p0) || !is_base_part(doc, p1) {
                    continue;
                }
                let rest_p0 = doc
                    .dom
                    .get_by_ref(p0)
                    .and_then(|i| rig.starting_pose.get(&i.name).copied())
                    .unwrap_or_else(|| cframe_prop(doc, p0, "CFrame"));
                let rest_p1 = doc
                    .dom
                    .get_by_ref(p1)
                    .and_then(|i| rig.starting_pose.get(&i.name).copied())
                    .unwrap_or_else(|| cframe_prop(doc, p1, "CFrame"));
                let rel = cf_mul(&cf_inv(&rest_p0), &rest_p1);
                edges.push(RigidEdge::WeldConstraint { p0, p1, rel });
            }
        } else if cs == "RigidConstraint" {
            if let (Some((p0, c0)), Some((p1, c1))) = (
                anim_constraint_endpoint(doc, r, "Attachment0"),
                anim_constraint_endpoint(doc, r, "Attachment1"),
            ) {
                if is_base_part(doc, p0) && is_base_part(doc, p1) {
                    edges.push(RigidEdge::Frame { p0, p1, c0, c1 });
                }
            }
        } else if let Some((p0, p1, c0, c1)) = joint_frames(doc, r) {
            if is_base_part(doc, p0) && is_base_part(doc, p1) {
                edges.push(RigidEdge::Frame { p0, p1, c0, c1 });
            }
        }
    }

    // Fixed-point propagation for chained welds / accessories.
    let mut progress = true;
    while progress {
        progress = false;
        for edge in &edges {
            match *edge {
                RigidEdge::Frame { p0, p1, c0, c1 } => {
                    let has0 = world_cframes.contains_key(&p0);
                    let has1 = world_cframes.contains_key(&p1);
                    if has0 && !has1 {
                        let cf0 = world_cframes[&p0];
                        let cf1 = cf_mul(&cf_mul(&cf0, &c0), &cf_inv(&c1));
                        world_cframes.insert(p1, cf1);
                        write_prop_live(doc, p1, "CFrame", Variant::CFrame(cf1.to_cframe()));
                        progress = true;
                    } else if has1 && !has0 {
                        let cf1 = world_cframes[&p1];
                        let cf0 = cf_mul(&cf_mul(&cf1, &c1), &cf_inv(&c0));
                        world_cframes.insert(p0, cf0);
                        write_prop_live(doc, p0, "CFrame", Variant::CFrame(cf0.to_cframe()));
                        progress = true;
                    }
                }
                RigidEdge::WeldConstraint { p0, p1, rel } => {
                    let has0 = world_cframes.contains_key(&p0);
                    let has1 = world_cframes.contains_key(&p1);
                    if has0 && !has1 {
                        let cf0 = world_cframes[&p0];
                        let cf1 = cf_mul(&cf0, &rel);
                        world_cframes.insert(p1, cf1);
                        write_prop_live(doc, p1, "CFrame", Variant::CFrame(cf1.to_cframe()));
                        progress = true;
                    } else if has1 && !has0 {
                        let cf1 = world_cframes[&p1];
                        let cf0 = cf_mul(&cf1, &cf_inv(&rel));
                        world_cframes.insert(p0, cf0);
                        write_prop_live(doc, p0, "CFrame", Variant::CFrame(cf0.to_cframe()));
                        progress = true;
                    }
                }
            }
        }
    }
}

/// Evaluates `data` at `tick`, writes `Motor6D.Transform`,
/// `AnimationConstraint.Transform`, `Bone.Transform` (scaling translation by
/// `rig.rig_scale`), and `FaceControls` FACS weights directly into `doc.dom`
/// with `doc.mark_prop`, and solves Edit-mode FK + rigid welds.
pub fn step_rig_animation(doc: &mut Document, rig: &RigCache, data: &AnimationData, tick: Tick) {
    let scale = if rig.rig_scale.is_finite() && rig.rig_scale > 0.0 {
        rig.rig_scale
    } else {
        1.0
    };
    let root_tracks = data.instances.get("Root").map(|inst| &inst.tracks);

    // Optional root motion on root_part.
    let base_root = rig.root_cframe.unwrap_or(IDENTITY_XFORM);
    let root_world = if let (Some(root_name), Some(tracks)) = (&rig.root_part_name, root_tracks) {
        if let Some(root_track) = tracks.get(root_name) {
            if let Some(mut delta) =
                evaluate_track_value(root_track, tick).and_then(|v| v.as_cframe())
            {
                delta.pos *= scale;
                cf_mul(&base_root, &delta)
            } else {
                base_root
            }
        } else {
            base_root
        }
    } else {
        base_root
    };

    // Evaluate and write joint / bone transforms.
    let mut evaluated_transforms: HashMap<String, Xform> = HashMap::new();
    for (part_name, joint) in &rig.joints_by_part {
        let mut xf = IDENTITY_XFORM;
        if let Some(tracks) = root_tracks {
            if let Some(track) = tracks.get(part_name) {
                if let Some(val) = evaluate_track_value(track, tick).and_then(|v| v.as_cframe()) {
                    xf = Xform {
                        pos: val.pos * scale,
                        rot: val.rot,
                    };
                }
            }
        }
        write_prop_live(
            doc,
            joint.joint_ref,
            "Transform",
            Variant::CFrame(xf.to_cframe()),
        );
        evaluated_transforms.insert(part_name.clone(), xf);
    }

    // Evaluate and write FaceControls FACS properties.
    if let Some(fc_ref) = rig.face_controls_ref {
        for &(facs_name, _) in FACS_CONTROLS {
            let val = root_tracks
                .and_then(|tracks| tracks.get(facs_name))
                .filter(|t| t.track_type == TrackType::Facs)
                .and_then(|t| evaluate_track_value(t, tick))
                .and_then(|v| v.as_f32())
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            write_prop_live(doc, fc_ref, facs_name, Variant::Float32(val));
        }
    }

    // Solve Edit-mode FK + rigid welds for live viewport animation.
    solve_edit_mode_fk(doc, rig, root_world, &evaluated_transforms);
}

/// Resets all joint/bone `Transform`s to identity, resets all `FaceControls`
/// weights to `0.0`, restores `rig.root_cframe`, and recomputes rest part
/// `CFrame`s.
pub fn reset_rig_pose(doc: &mut Document, rig: &RigCache) {
    let id_cf = CFrame::identity();
    for joint in rig.joints_by_part.values() {
        write_prop_live(doc, joint.joint_ref, "Transform", Variant::CFrame(id_cf));
    }
    if let Some(fc_ref) = rig.face_controls_ref {
        for &(facs_name, _) in FACS_CONTROLS {
            write_prop_live(doc, fc_ref, facs_name, Variant::Float32(0.0));
        }
    }
    let root_world = rig.root_cframe.unwrap_or(IDENTITY_XFORM);
    let empty_transforms = HashMap::new();
    solve_edit_mode_fk(doc, rig, root_world, &empty_transforms);
}

// ============================================================================
// AnimSaves & ServerStorage.RBX_ANIMSAVES Management (`RigUtils`)
// ============================================================================

fn read_timestamp_tag(doc: &Document, r: Ref) -> Option<u64> {
    match doc.prop(r, "Tags") {
        Some(Variant::Tags(tags)) => tags.iter().find_map(|t| t.parse::<u64>().ok()),
        Some(Variant::BinaryString(bs)) => std::str::from_utf8(bs.as_ref())
            .ok()
            .and_then(|s| s.split('\0').find_map(|t| t.parse::<u64>().ok())),
        Some(Variant::String(s)) => s.split('\0').find_map(|t| t.parse::<u64>().ok()),
        _ => None,
    }
}

/// Sorts saved animation entries newest-first, matching Studio's
/// `LoadLatestAnimation`: tagged clips before untagged, descending by numeric
/// Unix timestamp tag, tie-breaking by name ascending.
pub fn sort_anim_saves_latest(entries: &mut [RigAnimSaveEntry]) {
    entries.sort_by(|a, b| match (a.timestamp_tag, b.timestamp_tag) {
        (Some(ta), Some(tb)) => tb.cmp(&ta).then_with(|| a.name.cmp(&b.name)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.name.cmp(&b.name),
    });
}

fn collect_clips_in_container(
    doc: &Document,
    container: Ref,
    is_legacy: bool,
    seen: &mut HashSet<Ref>,
    out: &mut Vec<RigAnimSaveEntry>,
) {
    let Some(inst) = doc.dom.get_by_ref(container) else {
        return;
    };
    for &child in inst.children() {
        let Some(ci) = doc.dom.get_by_ref(child) else {
            continue;
        };
        if matches!(ci.class.as_str(), "KeyframeSequence" | "CurveAnimation") && seen.insert(child) {
            out.push(RigAnimSaveEntry {
                anim_ref: child,
                name: ci.name.clone(),
                is_legacy_anim_saves: is_legacy,
                timestamp_tag: read_timestamp_tag(doc, child),
            });
        }
    }
}

/// Lists all saved animations for `root_model` across both
/// `ServerStorage.RBX_ANIMSAVES` (linked via `rig.AnimSaves` `ObjectValue` or
/// discovered via reverse-lookup `entry.Value == root_model`) and legacy
/// children stored directly under any `rig.AnimSaves` child container.
///
/// Returned entries are sorted newest-first via [`sort_anim_saves_latest`].
/// Returns `(entries, has_legacy)`.
pub fn list_anim_saves(doc: &Document, root_model: Ref) -> (Vec<RigAnimSaveEntry>, bool) {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();

    // 1. Check ALL direct children of `root_model` named "AnimSaves"
    // (a rig can have both an empty legacy container and an ObjectValue link).
    if let Some(root_inst) = doc.dom.get_by_ref(root_model) {
        for &child in root_inst.children() {
            let Some(anim_saves_inst) = doc.dom.get_by_ref(child) else {
                continue;
            };
            if anim_saves_inst.name != ANIMSAVES_NAME {
                continue;
            }
            if anim_saves_inst.class.as_str() == "ObjectValue" {
                if let Some(server_target) = ref_prop(doc, child, "Value") {
                    // Only treat as valid server entry if it points back to this rig
                    // (or has no Value set, e.g. a Model/Folder target).
                    let target_val = ref_prop(doc, server_target, "Value");
                    if target_val.is_none() || target_val == Some(root_model) {
                        collect_clips_in_container(doc, server_target, false, &mut seen, &mut entries);
                    }
                }
                // Any animation clips still parented directly under the ObjectValue are legacy.
                collect_clips_in_container(doc, child, true, &mut seen, &mut entries);
            } else {
                // Legacy Model / Folder / Configuration container under the rig.
                collect_clips_in_container(doc, child, true, &mut seen, &mut entries);
            }
        }
    }

    // 2. Reverse-lookup in `ServerStorage.RBX_ANIMSAVES`: find any ObjectValue
    // whose `Value == root_model` even if `rig.AnimSaves` is missing or stale.
    if let Some(server_storage) = doc.find_child(doc.dom.root_ref(), "ServerStorage") {
        if let Some(ss_inst) = doc.dom.get_by_ref(server_storage) {
            for &rbx_folder in ss_inst.children() {
                let Some(folder_inst) = doc.dom.get_by_ref(rbx_folder) else {
                    continue;
                };
                if folder_inst.name != RBX_ANIMSAVES_NAME {
                    continue;
                }
                for &entry in folder_inst.children() {
                    if doc.class_of(entry).is_some_and(|c| c.as_str() == "ObjectValue")
                        && ref_prop(doc, entry, "Value") == Some(root_model)
                    {
                        collect_clips_in_container(doc, entry, false, &mut seen, &mut entries);
                    }
                }
            }
        }
    }

    sort_anim_saves_latest(&mut entries);
    let has_legacy = entries.iter().any(|e| e.is_legacy_anim_saves);
    (entries, has_legacy)
}

fn find_or_create_server_anim_saves(doc: &mut Document) -> Ref {
    let root = doc.dom.root_ref();
    let server_storage = doc.find_child(root, "ServerStorage").unwrap_or_else(|| {
        doc.insert(
            root,
            InstanceBuilder::new("ServerStorage").with_name("ServerStorage"),
        )
    });
    find_direct_child_by_name(doc, server_storage, RBX_ANIMSAVES_NAME).unwrap_or_else(|| {
        doc.insert(
            server_storage,
            InstanceBuilder::new("Model").with_name(RBX_ANIMSAVES_NAME),
        )
    })
}

/// Ensures `ServerStorage.RBX_ANIMSAVES[rig.Name]` and `rig.AnimSaves`
/// `ObjectValue` link exist, matching `RigUtils.getAnimSavesFolder(rig, true)`.
/// Returns `(server_entry_ref, rig_anim_saves_ov_ref)`.
fn ensure_anim_saves_folder(doc: &mut Document, root_model: Ref) -> (Ref, Ref) {
    let existing = find_direct_child_by_name(doc, root_model, ANIMSAVES_NAME);
    let ov_ref = match existing {
        Some(r) if doc.class_of(r).is_some_and(|c| c.as_str() == "ObjectValue") => r,
        Some(legacy_container) => {
            let ov = doc.insert(
                root_model,
                InstanceBuilder::new("ObjectValue").with_name(ANIMSAVES_NAME),
            );
            let legacy_children: Vec<Ref> = doc
                .dom
                .get_by_ref(legacy_container)
                .map(|i| i.children().to_vec())
                .unwrap_or_default();
            if !legacy_children.is_empty() {
                doc.reparent(&legacy_children, ov);
            }
            doc.remove(&[legacy_container]);
            ov
        }
        None => doc.insert(
            root_model,
            InstanceBuilder::new("ObjectValue").with_name(ANIMSAVES_NAME),
        ),
    };

    let server_folder = find_or_create_server_anim_saves(doc);
    let current_target = ref_prop(doc, ov_ref, "Value");
    let valid_target = current_target
        .filter(|&t| {
            doc.class_of(t).is_some_and(|c| c.as_str() == "ObjectValue")
                && ref_prop(doc, t, "Value") == Some(root_model)
        })
        .or_else(|| {
            doc.dom.get_by_ref(server_folder).and_then(|sf| {
                sf.children().iter().copied().find(|&c| {
                    doc.class_of(c).is_some_and(|cl| cl.as_str() == "ObjectValue")
                        && ref_prop(doc, c, "Value") == Some(root_model)
                })
            })
        });

    let server_entry = match valid_target {
        Some(t) => {
            if doc.dom.get_by_ref(t).is_some_and(|i| i.parent().is_none()) {
                doc.reparent(&[t], server_folder);
            }
            if ref_prop(doc, ov_ref, "Value") != Some(t) {
                doc.set_prop_live(ov_ref, ustr("Value"), Variant::Ref(t));
            }
            t
        }
        None => {
            let rig_name = doc
                .dom
                .get_by_ref(root_model)
                .map(|i| i.name.clone())
                .unwrap_or_else(|| "Rig".to_string());
            let entry = doc.insert(
                server_folder,
                InstanceBuilder::new("ObjectValue")
                    .with_name(rig_name)
                    .with_property("Value", Variant::Ref(root_model)),
            );
            doc.set_prop_live(ov_ref, ustr("Value"), Variant::Ref(entry));
            entry
        }
    };

    (server_entry, ov_ref)
}

fn refresh_rig_anim_saves(doc: &Document, rig: &mut RigCache) {
    rig.anim_saves.clear();
    rig.legacy_anim_saves.clear();
    let Some(root_model) = rig.root_ref else {
        return;
    };
    let (all, _) = list_anim_saves(doc, root_model);
    for entry in all {
        if entry.is_legacy_anim_saves {
            rig.legacy_anim_saves.push(entry);
        } else {
            rig.anim_saves.push(entry);
        }
    }
    sort_anim_saves_latest(&mut rig.anim_saves);
    sort_anim_saves_latest(&mut rig.legacy_anim_saves);
}

/// Serializes `data` as a `CurveAnimation` (if channel animation) or
/// `KeyframeSequence`, saves it into `ServerStorage.RBX_ANIMSAVES` linked via
/// `rig.AnimSaves`, sets a Unix timestamp tag, and refreshes `rig.anim_saves`.
pub fn save_clip_to_anim_saves(
    doc: &mut Document,
    rig: &mut RigCache,
    data: &AnimationData,
    name: &str,
) -> Result<Ref, String> {
    let root_model = rig
        .root_ref
        .ok_or_else(|| "No active rig attached".to_string())?;
    if !doc.exists(root_model) {
        return Err("Rig no longer exists in document".to_string());
    }

    let (server_entry, _) = ensure_anim_saves_folder(doc, root_model);

    // Destroy any existing saved animation with the same name in target folder.
    if let Some(existing) = find_direct_child_by_name(doc, server_entry, name) {
        doc.remove(&[existing]);
    }

    let mut named_data = data.clone();
    named_data.metadata.name = name.to_string();

    let clip_ref = if named_data.is_channel_animation() {
        serialization::to_curve_animation(doc, server_entry, &named_data, Some(rig))
    } else {
        serialization::to_keyframe_sequence(doc, server_entry, &named_data, Some(rig))
    };

    // Tag with current Unix timestamp (matching SaveAnimation).
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let tags = Tags::from(vec![now_secs.to_string()]);
    doc.set_prop_live(clip_ref, ustr("Tags"), Variant::Tags(tags));

    refresh_rig_anim_saves(doc, rig);
    Ok(clip_ref)
}

/// Applies user migration choices for legacy animations stored directly under
/// `rig.AnimSaves` (`MigrationDialog`), then refreshes `rig.anim_saves`.
pub fn migrate_legacy_anim_saves(
    doc: &mut Document,
    rig: &mut RigCache,
    choices: &[(Ref, MigrationChoice)],
) {
    let Some(root_model) = rig.root_ref else {
        return;
    };
    let mut server_entry: Option<Ref> = None;

    for &(anim_ref, choice) in choices {
        if !doc.exists(anim_ref) {
            continue;
        }
        match choice {
            MigrationChoice::Migrate => {
                let target = *server_entry
                    .get_or_insert_with(|| ensure_anim_saves_folder(doc, root_model).0);
                doc.reparent(&[anim_ref], target);
            }
            MigrationChoice::Delete => {
                doc.remove(&[anim_ref]);
            }
            MigrationChoice::Ignore => {}
        }
    }

    refresh_rig_anim_saves(doc, rig);
}

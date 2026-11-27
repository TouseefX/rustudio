//! Complete Facial Animation / FaceControls (FACS) utilities for RusTudio's native
//! `AnimationClipEditor` (matching `Util/FacsUtils`, `FaceControlsEditorWindow`,
//! `RigUtils.focusCameraOnFace`, and `Thunks/MergeBodyAndFacialAnimation`).

use glam::{Mat3, Vec3};
use rbx_dom_weak::types::{Ref, Variant};
use rustudio_core::doc::Document;
use rustudio_core::math::Xform;

use crate::anim_editor::constants::{
    facs_controls_in_region, facs_slider_partner, facs_symmetry_partner, is_facs_name, FacsRegion,
    FACS_CONTROLS,
};
use crate::anim_editor::model::{
    AnimationData, EasingStyle, KeyInterpolationMode, KeyframeValue, RotationOrder, Tick, Track,
    TrackType,
};
use crate::anim_editor::state::{EditorState, RigCache, TrackListEntry};

/// Default root instance name in `AnimationData.instances`.
const ROOT_INSTANCE: &str = "Root";

/// Evaluates a scalar track (e.g. `TrackType::Facs`) at `tick`.
fn evaluate_scalar_track(track: &Track, tick: Tick) -> f32 {
    if track.keyframes.is_empty() {
        return 0.0;
    }
    if let Some(kf) = track.data.get(&tick) {
        return kf.value.as_f32().unwrap_or(0.0);
    }
    let first_t = track.keyframes[0];
    if tick <= first_t {
        return track
            .data
            .get(&first_t)
            .and_then(|kf| kf.value.as_f32())
            .unwrap_or(0.0);
    }
    let last_t = *track.keyframes.last().unwrap();
    if tick >= last_t {
        return track
            .data
            .get(&last_t)
            .and_then(|kf| kf.value.as_f32())
            .unwrap_or(0.0);
    }

    let idx = track.keyframes.partition_point(|&t| t < tick);
    let t0 = track.keyframes[idx - 1];
    let t1 = track.keyframes[idx];
    let Some(kf0) = track.data.get(&t0) else {
        return 0.0;
    };
    let Some(kf1) = track.data.get(&t1) else {
        return kf0.value.as_f32().unwrap_or(0.0);
    };
    let v0 = kf0.value.as_f32().unwrap_or(0.0);
    let v1 = kf1.value.as_f32().unwrap_or(0.0);

    if kf0.easing_style == Some(EasingStyle::Constant)
        || kf0.interpolation_mode == Some(KeyInterpolationMode::Constant)
    {
        return v0;
    }

    let dt = (t1 - t0) as f32;
    if dt <= 0.0 {
        return v0;
    }
    let alpha = ((tick - t0) as f32 / dt).clamp(0.0, 1.0);

    // Cubic Hermite evaluation when curve slopes are present.
    if kf0.interpolation_mode == Some(KeyInterpolationMode::Cubic)
        && (kf0.right_slope.is_some() || kf1.left_slope.is_some())
    {
        let m0 = kf0.right_slope.unwrap_or(0.0) * dt;
        let m1 = kf1.left_slope.unwrap_or(0.0) * dt;
        let t = alpha;
        let t2 = t * t;
        let t3 = t2 * t;
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        return h00 * v0 + h10 * m0 + h01 * v1 + h11 * m1;
    }

    v0 + (v1 - v0) * alpha
}

/// Evaluates the FACS track `facs_name` at `state.playback.playhead`, or returns `0.0`
/// if no track or animation data exists.
pub fn get_facs_value(state: &EditorState, facs_name: &str) -> f32 {
    let Some(anim) = &state.animation_data else {
        return 0.0;
    };
    let path = [facs_name.to_string()];
    let Some(track) = anim.get_track(ROOT_INSTANCE, &path) else {
        return 0.0;
    };
    evaluate_scalar_track(track, state.playback.playhead).clamp(0.0, 1.0)
}

/// Returns `true` if a keyframe exists for `facs_name` at `state.playback.playhead`.
pub fn has_facs_keyframe_at_playhead(state: &EditorState, facs_name: &str) -> bool {
    let Some(anim) = &state.animation_data else {
        return false;
    };
    let path = [facs_name.to_string()];
    anim.get_track(ROOT_INSTANCE, &path)
        .is_some_and(|t| t.data.contains_key(&state.playback.playhead))
}

/// Returns all FACS control names not currently present in `Root.tracks`, sorted alphabetically
/// (`Util/FacsUtils` `getUnusedFacs`).
pub fn get_unused_facs(state: &EditorState) -> Vec<&'static str> {
    let existing = state
        .animation_data
        .as_ref()
        .and_then(|a| a.instances.get(ROOT_INSTANCE))
        .map(|inst| &inst.tracks);

    let mut unused: Vec<&'static str> = FACS_CONTROLS
        .iter()
        .map(|&(name, _)| name)
        .filter(|&name| existing.is_none_or(|tracks| !tracks.contains_key(name)))
        .collect();
    unused.sort_unstable();
    unused
}

/// Adds tracks for all 50 FACS controls that do not yet exist in the animation.
pub fn add_all_facs_tracks(state: &mut EditorState) {
    let unused: Vec<&'static str> = get_unused_facs(state);
    if unused.is_empty() {
        return;
    }
    state.add_waypoint();
    let rot_type = state.settings.default_rotation_type;
    let euler_order = state.settings.default_euler_angles_order;

    if let Some(anim) = &mut state.animation_data {
        for name in &unused {
            anim.add_track(ROOT_INSTANCE, *name, TrackType::Facs, rot_type, euler_order);
        }
    }
    for name in unused {
        if !state
            .tracks
            .iter()
            .any(|t| t.name == name && t.instance == ROOT_INSTANCE)
        {
            state
                .tracks
                .push(TrackListEntry::new(name, ROOT_INSTANCE, TrackType::Facs, 0));
        }
    }
    state.unused_facs.clear();
    state.is_dirty = true;
}

/// Returns the mutually exclusive opposing FACS control for dual-slider and eye-gaze partners.
fn opposing_facs_partner(facs_name: &str) -> Option<&'static str> {
    match facs_name {
        "EyesLookLeft" => Some("EyesLookRight"),
        "EyesLookRight" => Some("EyesLookLeft"),
        "EyesLookUp" => Some("EyesLookDown"),
        "EyesLookDown" => Some("EyesLookUp"),
        _ => facs_slider_partner(facs_name),
    }
}

/// Writes a single FACS keyframe through `crate::anim_editor::ops::value_changed`.
fn write_facs_keyframe(state: &mut EditorState, facs_name: &str, value: f32) {
    let tick = state.playback.playhead;
    let path = [facs_name.to_string()];
    crate::anim_editor::ops::value_changed(
        state,
        ROOT_INSTANCE,
        &path,
        TrackType::Facs,
        tick,
        KeyframeValue::Facs(value.clamp(0.0, 1.0)),
        false,
    );
}

/// Sets a FACS control value in `[0.0, 1.0]` at `state.playback.playhead`, enforcing:
/// - `[0.0, 1.0]` clamping
/// - Mutual exclusion for opposing dual-slider / eye-gaze partners when `value > 0.0`
/// - Symmetry mirroring across all 14 `FACS_SYMMETRY_PAIRS` when `state.facs.symmetry_enabled`
/// - Keyframe authoring via `crate::anim_editor::ops::value_changed`
pub fn set_facs_value(state: &mut EditorState, facs_name: &str, value: f32, add_waypoint: bool) {
    let val = value.clamp(0.0, 1.0);
    if add_waypoint {
        state.add_waypoint();
    }

    write_facs_keyframe(state, facs_name, val);

    if val > 0.0 {
        if let Some(opp) = opposing_facs_partner(facs_name) {
            write_facs_keyframe(state, opp, 0.0);
        }
    }

    if state.facs.symmetry_enabled {
        if let Some(sym) = facs_symmetry_partner(facs_name) {
            write_facs_keyframe(state, sym, val);
            if val > 0.0 {
                if let Some(sym_opp) = opposing_facs_partner(sym) {
                    write_facs_keyframe(state, sym_opp, 0.0);
                }
            }
        }
    }
}

/// Sets a signed dual-slider pair (`signed_val` in `[-1.0, 1.0]`):
/// - `signed_val < 0`: `neg_control = |signed_val|`, `pos_control = 0.0`
/// - `signed_val > 0`: `neg_control = 0.0`, `pos_control = signed_val`
/// - `signed_val == 0`: both `0.0`
pub fn set_dual_slider_value(
    state: &mut EditorState,
    neg_control: &str,
    pos_control: &str,
    signed_val: f32,
    add_waypoint: bool,
) {
    let v = signed_val.clamp(-1.0, 1.0);
    if add_waypoint {
        state.add_waypoint();
    }
    if v < 0.0 {
        set_facs_value(state, pos_control, 0.0, false);
        set_facs_value(state, neg_control, (-v).clamp(0.0, 1.0), false);
    } else if v > 0.0 {
        set_facs_value(state, neg_control, 0.0, false);
        set_facs_value(state, pos_control, v.clamp(0.0, 1.0), false);
    } else {
        set_facs_value(state, neg_control, 0.0, false);
        set_facs_value(state, pos_control, 0.0, false);
    }
}

/// Reads a signed dual-slider value in `[-1.0, 1.0]` from `(neg_control, pos_control)`.
pub fn get_dual_slider_value(state: &EditorState, neg_control: &str, pos_control: &str) -> f32 {
    let neg = get_facs_value(state, neg_control);
    let pos = get_facs_value(state, pos_control);
    if pos > 0.0 {
        pos.clamp(0.0, 1.0)
    } else if neg > 0.0 {
        -neg.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Sets the 2D `EyesDragBox` gaze coordinates:
/// - `x` in `[-1.0, 1.0]` -> `EyesLookLeft` (negative) / `EyesLookRight` (positive)
/// - `y` in `[-1.0, 1.0]` -> `EyesLookDown` (negative) / `EyesLookUp` (positive)
pub fn set_eyes_drag_box(state: &mut EditorState, x: f32, y: f32, add_waypoint: bool) {
    if add_waypoint {
        state.add_waypoint();
    }
    set_dual_slider_value(state, "EyesLookLeft", "EyesLookRight", x, false);
    set_dual_slider_value(state, "EyesLookDown", "EyesLookUp", y, false);
}

/// Reads the current 2D `EyesDragBox` coordinates `(x, y)` in `[-1.0, 1.0]^2`.
pub fn get_eyes_drag_box(state: &EditorState) -> (f32, f32) {
    let x = get_dual_slider_value(state, "EyesLookLeft", "EyesLookRight");
    let y = get_dual_slider_value(state, "EyesLookDown", "EyesLookUp");
    (x, y)
}

/// Resets all non-zero or keyed FACS controls at the current playhead to `0.0`.
pub fn reset_all_facs(state: &mut EditorState) {
    let controls_to_reset: Vec<&'static str> = FACS_CONTROLS
        .iter()
        .map(|&(name, _)| name)
        .filter(|&name| {
            get_facs_value(state, name) > 0.0 || has_facs_keyframe_at_playhead(state, name)
        })
        .collect();
    if controls_to_reset.is_empty() {
        return;
    }
    state.add_waypoint();
    for name in controls_to_reset {
        write_facs_keyframe(state, name, 0.0);
    }
}

/// Resets all FACS controls in `region` at the current playhead to `0.0`.
pub fn reset_region_facs(state: &mut EditorState, region: FacsRegion) {
    let controls_to_reset: Vec<&'static str> = facs_controls_in_region(region)
        .filter(|&name| {
            get_facs_value(state, name) > 0.0 || has_facs_keyframe_at_playhead(state, name)
        })
        .collect();
    if controls_to_reset.is_empty() {
        return;
    }
    state.add_waypoint();
    for name in controls_to_reset {
        write_facs_keyframe(state, name, 0.0);
    }
}

fn read_cframe(doc: &Document, r: Ref, prop: &str) -> Option<Xform> {
    match doc.prop(r, prop) {
        Some(Variant::CFrame(cf)) => Some(Xform::from_cframe(&cf)),
        _ => None,
    }
}

fn read_vec3(doc: &Document, r: Ref, prop: &str) -> Option<Vec3> {
    match doc.prop(r, prop) {
        Some(Variant::Vector3(v)) => Some(Vec3::new(v.x, v.y, v.z)),
        _ => None,
    }
}

/// Computes `(camera_pos, look_at_head_center)` facing the Head's front face at exact framing
/// distance (matching `RigUtils.focusCameraOnFace` + `CharacterUtility.CalculateHeadExtents`).
pub fn focus_camera_on_face_target(doc: &Document, rig: &RigCache) -> Option<(Vec3, Vec3)> {
    let head_ref = rig
        .face_controls_ref
        .and_then(|fc| doc.dom.get_by_ref(fc).map(|inst| inst.parent()))
        .filter(|&p| p.is_some() && doc.exists(p))
        .or_else(|| rig.part_refs.get("Head").copied())?;

    let head_cf = read_cframe(doc, head_ref, "CFrame")?;
    let head_size = read_vec3(doc, head_ref, "Size").unwrap_or(Vec3::new(1.2, 1.2, 1.2));

    // Locate parent reference part (e.g. UpperTorso via Neck) for front facing direction.
    let parent_cf = rig
        .joints_by_part
        .get("Head")
        .and_then(|j| j.part0_ref)
        .and_then(|p0| read_cframe(doc, p0, "CFrame"))
        .unwrap_or(head_cf);

    // CalculateTargetCFrame: flatten Head look vector onto XZ plane.
    let head_look = -(head_cf.rot.z_axis).normalize_or_zero();
    let dir = if head_look.y.abs() > 0.9 {
        head_cf.rot.y_axis
    } else {
        head_look
    };
    let mut flat_dir = Vec3::new(dir.x, 0.0, dir.z).normalize_or_zero();
    if flat_dir.length_squared() < 1e-6 {
        flat_dir = Vec3::NEG_Z;
    }
    let z_axis = -flat_dir;
    let x_axis = Vec3::Y.cross(z_axis).normalize_or_zero();
    let y_axis = z_axis.cross(x_axis).normalize_or_zero();
    let target_cf = Xform {
        pos: head_cf.pos,
        rot: Mat3::from_cols(x_axis, y_axis, z_axis),
    };
    let target_inv = target_cf.inverse();

    // Project the 8 corners of Head into target_cf local space.
    let half = head_size * 0.5;
    let mut min_ext = Vec3::splat(f32::INFINITY);
    let mut max_ext = Vec3::splat(f32::NEG_INFINITY);
    for sx in [-1.0_f32, 1.0] {
        for sy in [-1.0_f32, 1.0] {
            for sz in [-1.0_f32, 1.0] {
                let world_corner = head_cf.apply(Vec3::new(sx * half.x, sy * half.y, sz * half.z));
                let local_corner = target_inv.apply(world_corner);
                min_ext = min_ext.min(local_corner);
                max_ext = max_ext.max(local_corner);
            }
        }
    }

    let extents = max_ext - min_ext;
    let base_dist = extents.x.min(extents.z) + 0.25;
    let mut parent_look = -(parent_cf.rot.z_axis).normalize_or_zero();
    if parent_look.length_squared() < 1e-6 {
        parent_look = flat_dir;
    }

    let cam_pos = head_cf.pos + parent_look * (base_dist * 1.5);
    Some((cam_pos, head_cf.pos))
}

/// Merges a body animation and a facial animation, keeping `Head` and all FACS tracks from
/// `facial` and all other body tracks from `body` (`Thunks/MergeBodyAndFacialAnimation`).
pub fn merge_body_and_facial_animation(
    body: &AnimationData,
    facial: &AnimationData,
) -> AnimationData {
    let mut merged = body.clone();
    let mut face = facial.clone();

    // If either animation is a channel animation, promote the other so track formats match.
    if merged.is_channel_animation() != face.is_channel_animation() {
        let rot_type = TrackType::EulerAngles;
        let euler_order = RotationOrder::XYZ;
        if !merged.is_channel_animation() {
            let saved_name = merged.metadata.name.clone();
            merged.promote_to_channels(rot_type, euler_order);
            merged.metadata.name = saved_name;
        }
        if !face.is_channel_animation() {
            face.promote_to_channels(rot_type, euler_order);
        }
    }

    // Remove existing FACS tracks and Head from the body animation.
    if let Some(root_inst) = merged.instances.get_mut(ROOT_INSTANCE) {
        root_inst.tracks.retain(|name, track| {
            track.track_type != TrackType::Facs && !is_facs_name(name) && name != "Head"
        });
    }

    // Copy FACS tracks and Head track from the facial animation.
    if let Some(face_root) = face.instances.get(ROOT_INSTANCE) {
        let dest_root = merged
            .instances
            .entry(ROOT_INSTANCE.to_string())
            .or_default();
        for (name, track) in &face_root.tracks {
            if track.track_type == TrackType::Facs || is_facs_name(name) || name == "Head" {
                dest_root.tracks.insert(name.clone(), track.clone());
            }
        }
    }

    merged.set_end_tick();
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim_editor::model::{Keyframe, KeyframeValue};

    fn make_state_with_anim() -> EditorState {
        let mut state = EditorState::default();
        state.animation_data = Some(AnimationData::new_rig_animation("TestClip"));
        state.playback.playhead = 0;
        state
    }

    #[test]
    fn test_symmetry_mirroring_and_mutual_exclusion() {
        let mut state = make_state_with_anim();
        state.facs.symmetry_enabled = true;

        set_facs_value(&mut state, "LeftEyeClosed", 0.8, true);
        assert!((get_facs_value(&state, "LeftEyeClosed") - 0.8).abs() < 1e-4);
        assert!((get_facs_value(&state, "RightEyeClosed") - 0.8).abs() < 1e-4);
        assert!((get_facs_value(&state, "LeftEyeUpperLidRaiser") - 0.0).abs() < 1e-4);
        assert!((get_facs_value(&state, "RightEyeUpperLidRaiser") - 0.0).abs() < 1e-4);

        // Raising LeftEyeUpperLidRaiser should zero both LeftEyeClosed and RightEyeClosed.
        set_facs_value(&mut state, "LeftEyeUpperLidRaiser", 0.6, false);
        assert!((get_facs_value(&state, "LeftEyeUpperLidRaiser") - 0.6).abs() < 1e-4);
        assert!((get_facs_value(&state, "RightEyeUpperLidRaiser") - 0.6).abs() < 1e-4);
        assert!((get_facs_value(&state, "LeftEyeClosed") - 0.0).abs() < 1e-4);
        assert!((get_facs_value(&state, "RightEyeClosed") - 0.0).abs() < 1e-4);
    }

    #[test]
    fn test_dual_slider_maps_signed_range() {
        let mut state = make_state_with_anim();

        set_dual_slider_value(&mut state, "JawLeft", "JawRight", -0.75, false);
        assert!((get_facs_value(&state, "JawLeft") - 0.75).abs() < 1e-4);
        assert!((get_facs_value(&state, "JawRight") - 0.0).abs() < 1e-4);
        assert!((get_dual_slider_value(&state, "JawLeft", "JawRight") - (-0.75)).abs() < 1e-4);

        set_dual_slider_value(&mut state, "JawLeft", "JawRight", 0.4, false);
        assert!((get_facs_value(&state, "JawLeft") - 0.0).abs() < 1e-4);
        assert!((get_facs_value(&state, "JawRight") - 0.4).abs() < 1e-4);
        assert!((get_dual_slider_value(&state, "JawLeft", "JawRight") - 0.4).abs() < 1e-4);
    }

    #[test]
    fn test_eyes_drag_box_roundtrip() {
        let mut state = make_state_with_anim();

        set_eyes_drag_box(&mut state, -0.5, 0.9, false);
        assert!((get_facs_value(&state, "EyesLookLeft") - 0.5).abs() < 1e-4);
        assert!((get_facs_value(&state, "EyesLookRight") - 0.0).abs() < 1e-4);
        assert!((get_facs_value(&state, "EyesLookUp") - 0.9).abs() < 1e-4);
        assert!((get_facs_value(&state, "EyesLookDown") - 0.0).abs() < 1e-4);

        let (x, y) = get_eyes_drag_box(&state);
        assert!((x - (-0.5)).abs() < 1e-4);
        assert!((y - 0.9).abs() < 1e-4);
    }

    #[test]
    fn test_merge_body_and_facial_animation_keeps_facial_head() {
        let mut body = AnimationData::new_rig_animation("BodyClip");
        body.add_track(
            ROOT_INSTANCE,
            "UpperTorso",
            TrackType::CFrame,
            TrackType::EulerAngles,
            RotationOrder::XYZ,
        );
        body.add_keyframe(
            ROOT_INSTANCE,
            &["UpperTorso".to_string()],
            0,
            Keyframe::new_pose(KeyframeValue::CFrame(Xform {
                pos: Vec3::new(0.0, 1.0, 0.0),
                rot: Mat3::IDENTITY,
            })),
        );
        body.add_track(
            ROOT_INSTANCE,
            "Head",
            TrackType::CFrame,
            TrackType::EulerAngles,
            RotationOrder::XYZ,
        );
        body.add_keyframe(
            ROOT_INSTANCE,
            &["Head".to_string()],
            0,
            Keyframe::new_pose(KeyframeValue::CFrame(Xform {
                pos: Vec3::new(9.0, 9.0, 9.0),
                rot: Mat3::IDENTITY,
            })),
        );

        let mut facial = AnimationData::new_rig_animation("FaceClip");
        facial.add_track(
            ROOT_INSTANCE,
            "Head",
            TrackType::CFrame,
            TrackType::EulerAngles,
            RotationOrder::XYZ,
        );
        facial.add_keyframe(
            ROOT_INSTANCE,
            &["Head".to_string()],
            0,
            Keyframe::new_pose(KeyframeValue::CFrame(Xform {
                pos: Vec3::new(0.0, 0.25, 0.0),
                rot: Mat3::IDENTITY,
            })),
        );
        facial.add_track(
            ROOT_INSTANCE,
            "JawDrop",
            TrackType::Facs,
            TrackType::EulerAngles,
            RotationOrder::XYZ,
        );
        facial.add_keyframe(
            ROOT_INSTANCE,
            &["JawDrop".to_string()],
            1200,
            Keyframe::new_pose(KeyframeValue::Facs(0.7)),
        );

        let merged = merge_body_and_facial_animation(&body, &facial);
        assert_eq!(merged.metadata.name, "BodyClip");
        assert!(merged
            .get_track(ROOT_INSTANCE, &["UpperTorso".to_string()])
            .is_some());
        assert!(merged
            .get_track(ROOT_INSTANCE, &["JawDrop".to_string()])
            .is_some());

        let head_kf = merged
            .get_track(ROOT_INSTANCE, &["Head".to_string()])
            .and_then(|t| t.data.get(&0))
            .expect("Merged Head keyframe");
        let head_pos = head_kf.value.as_cframe().unwrap().pos;
        assert!((head_pos - Vec3::new(0.0, 0.25, 0.0)).length() < 1e-4);
        assert_eq!(merged.metadata.end_tick, 1200);
    }
}

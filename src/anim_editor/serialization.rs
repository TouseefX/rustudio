//! Studio-matched bidirectional serialization for RusTudio's native
//! `AnimationClipEditor`:
//! - `KeyframeSequence` <-> `AnimationData`
//! - `CurveAnimation` <-> `AnimationData` (binary `ValuesAndTimes` wire format
//!   100% compatible with Roblox Studio & RusTudio's runtime `Animator`)
//! - FBX animation import via `rustudio_core::fbx_anim`
//! - `.rbxm` / `.rbxmx` animation clip export and import via `rbx_binary` / `rbx_xml`

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::path::Path;

use glam::{Mat3, Quat, Vec3};
use rbx_dom_weak::types::{BinaryString, Enum, Ref, Variant};
use rbx_dom_weak::{InstanceBuilder, WeakDom};
use rustudio_core::doc::Document;
use rustudio_core::math::Xform;

use crate::anim_editor::constants::{
    DEFAULT_FRAMERATE, DEFAULT_IMPORTED_NAME, DEFAULT_KEYFRAME_NAME, FACE_CONTROLS_FOLDER,
    MAX_FRAMERATE, MIN_FRAMERATE, NAMED_KEYFRAMES_CHANNEL, PROP_POSITION, PROP_ROTATION, PROP_X,
    PROP_Y, PROP_Z, TICK_FREQUENCY,
};
use crate::anim_editor::curves;
use crate::anim_editor::model::{
    nearest_tick, seconds_to_tick, tick_to_seconds, AnimationData, AnimationPriority,
    EasingDirection, EasingStyle, KeyInterpolationMode, Keyframe, KeyframeValue, RotationOrder,
    Tick, Track, TrackType,
};
use crate::anim_editor::state::{RigCache, TangentSide};

// ============================================================================
// Binary Curve Wire Format Codecs (`ValuesAndTimes` — matches `anim_lib.luau`)
// ============================================================================
//
// IMPORTANT: Roblox's wire `Enum.KeyInterpolationMode` values are:
//   0 = Constant, 1 = Linear, 2 = Cubic
// whereas `model::KeyInterpolationMode` has its own Rust discriminant order.
// Always convert via `interp_from_wire` / `interp_to_wire`.

fn interp_from_wire(mode: u8) -> KeyInterpolationMode {
    match mode {
        0 => KeyInterpolationMode::Constant,
        1 => KeyInterpolationMode::Linear,
        _ => KeyInterpolationMode::Cubic,
    }
}

fn interp_to_wire(mode: KeyInterpolationMode) -> u8 {
    match mode {
        KeyInterpolationMode::Constant => 0,
        KeyInterpolationMode::Linear => 1,
        KeyInterpolationMode::Cubic => 2,
    }
}

#[derive(Debug, Clone, Copy)]
struct FloatKeyWire {
    tick: Tick,
    value: f32,
    mode: KeyInterpolationMode,
    left_tangent: Option<f32>,
    right_tangent: Option<f32>,
    left_stored: f32,
    right_stored: f32,
}

/// Decodes `FloatCurve.ValuesAndTimes` (version 1 or 2).
fn decode_float_curve_keys(data: &[u8]) -> Vec<FloatKeyWire> {
    if data.len() < 16 {
        return Vec::new();
    }
    let version = u32::from_le_bytes(data[0..4].try_into().unwrap());
    if version != 1 && version != 2 {
        return Vec::new();
    }
    let n = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    let Some(values_bytes) = n.checked_mul(14) else {
        return Vec::new();
    };
    let Some(times_off) = 8usize.checked_add(values_bytes) else {
        return Vec::new();
    };
    let Some(total) = times_off
        .checked_add(8)
        .and_then(|v| v.checked_add(n.checked_mul(4)?))
    else {
        return Vec::new();
    };
    if total > data.len() {
        return Vec::new();
    }
    let time_ver = u32::from_le_bytes(data[times_off..times_off + 4].try_into().unwrap());
    let time_n =
        u32::from_le_bytes(data[times_off + 4..times_off + 8].try_into().unwrap()) as usize;
    if time_ver != 1 || time_n != n {
        return Vec::new();
    }

    let mut keys = Vec::with_capacity(n);
    for i in 0..n {
        let off = 8 + i * 14;
        let word = u16::from_le_bytes(data[off..off + 2].try_into().unwrap());
        let mode = interp_from_wire((word & 0xff) as u8);
        let flags = (word >> 8) as u8;
        let value = f32::from_le_bytes(data[off + 2..off + 6].try_into().unwrap());
        let lt_raw = f32::from_le_bytes(data[off + 6..off + 10].try_into().unwrap());
        let rt_raw = f32::from_le_bytes(data[off + 10..off + 14].try_into().unwrap());
        let t_off = times_off + 8 + i * 4;
        let tick = i32::from_le_bytes(data[t_off..t_off + 4].try_into().unwrap());
        keys.push(FloatKeyWire {
            tick,
            value,
            mode,
            left_tangent: (flags & 1 != 0).then_some(lt_raw),
            right_tangent: (flags & 2 != 0).then_some(rt_raw),
            left_stored: lt_raw,
            right_stored: rt_raw,
        });
    }
    keys
}

/// Encodes `FloatCurve.ValuesAndTimes` (version 2).
///
/// Matches Roblox Studio's wire format: `flags` bit 0/1 record whether the user
/// explicitly set `LeftTangent`/`RightTangent` (so `GetKeys()` and re-import
/// preserve auto vs explicit tangents), while the stored `f32` tangent fields
/// hold the effective slopes (including auto-computed Catmull-Rom tangents)
/// that Studio's runtime `FloatCurve` / `Animator` directly evaluate.
fn encode_float_curve_keys(keys: &[FloatKeyWire]) -> Vec<u8> {
    let n = keys.len();
    let mut out = Vec::with_capacity(16 + n * 18);
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(n as u32).to_le_bytes());
    for k in keys {
        let mode_byte = interp_to_wire(k.mode) as u16;
        let flags = (if k.left_tangent.is_some() { 1u16 } else { 0 })
            | (if k.right_tangent.is_some() { 2u16 } else { 0 });
        let word = mode_byte | (flags << 8);
        out.extend_from_slice(&word.to_le_bytes());
        out.extend_from_slice(&k.value.to_le_bytes());
        out.extend_from_slice(&k.left_tangent.unwrap_or(k.left_stored).to_le_bytes());
        out.extend_from_slice(&k.right_tangent.unwrap_or(k.right_stored).to_le_bytes());
    }
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(n as u32).to_le_bytes());
    for k in keys {
        out.extend_from_slice(&k.tick.to_le_bytes());
    }
    out
}

#[derive(Debug, Clone, Copy)]
struct RotationKeyWire {
    tick: Tick,
    quat: Quat,
    mode: KeyInterpolationMode,
    left_tangent: Option<f32>,
    right_tangent: Option<f32>,
    left_stored: f32,
    right_stored: f32,
}

/// Decodes `RotationCurve.ValuesAndTimes` (version 1).
fn decode_rotation_curve_keys(data: &[u8]) -> Vec<RotationKeyWire> {
    if data.len() < 16 {
        return Vec::new();
    }
    let version = u32::from_le_bytes(data[0..4].try_into().unwrap());
    if version != 1 {
        return Vec::new();
    }
    let n = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    let Some(values_bytes) = n.checked_mul(25) else {
        return Vec::new();
    };
    let Some(times_off) = 8usize.checked_add(values_bytes) else {
        return Vec::new();
    };
    let Some(total) = times_off
        .checked_add(8)
        .and_then(|v| v.checked_add(n.checked_mul(4)?))
    else {
        return Vec::new();
    };
    if total > data.len() {
        return Vec::new();
    }
    let time_ver = u32::from_le_bytes(data[times_off..times_off + 4].try_into().unwrap());
    let time_n =
        u32::from_le_bytes(data[times_off + 4..times_off + 8].try_into().unwrap()) as usize;
    if time_ver != 1 || time_n != n {
        return Vec::new();
    }

    let mut keys = Vec::with_capacity(n);
    for i in 0..n {
        let off = 8 + i * 25;
        let header = data[off];
        let mode = interp_from_wire(header & 3);
        let x = f32::from_le_bytes(data[off + 1..off + 5].try_into().unwrap());
        let y = f32::from_le_bytes(data[off + 5..off + 9].try_into().unwrap());
        let z = f32::from_le_bytes(data[off + 9..off + 13].try_into().unwrap());
        let w = f32::from_le_bytes(data[off + 13..off + 17].try_into().unwrap());
        let lt_raw = f32::from_le_bytes(data[off + 17..off + 21].try_into().unwrap());
        let rt_raw = f32::from_le_bytes(data[off + 21..off + 25].try_into().unwrap());
        let t_off = times_off + 8 + i * 4;
        let tick = i32::from_le_bytes(data[t_off..t_off + 4].try_into().unwrap());
        let q = Quat::from_xyzw(x, y, z, w);
        let quat = if q.length_squared() > 1e-12 {
            q.normalize()
        } else {
            Quat::IDENTITY
        };
        keys.push(RotationKeyWire {
            tick,
            quat,
            mode,
            left_tangent: (header & 4 != 0).then_some(lt_raw),
            right_tangent: (header & 8 != 0).then_some(rt_raw),
            left_stored: lt_raw,
            right_stored: rt_raw,
        });
    }
    keys
}

/// Encodes `RotationCurve.ValuesAndTimes` (version 1).
fn encode_rotation_curve_keys(keys: &[RotationKeyWire]) -> Vec<u8> {
    let n = keys.len();
    let mut out = Vec::with_capacity(16 + n * 29);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(n as u32).to_le_bytes());
    for k in keys {
        let q = if k.quat.length_squared() > 1e-12 {
            k.quat.normalize()
        } else {
            Quat::IDENTITY
        };
        let header = interp_to_wire(k.mode) | 12;
        out.push(header);
        out.extend_from_slice(&q.x.to_le_bytes());
        out.extend_from_slice(&q.y.to_le_bytes());
        out.extend_from_slice(&q.z.to_le_bytes());
        out.extend_from_slice(&q.w.to_le_bytes());
        out.extend_from_slice(&k.left_tangent.unwrap_or(k.left_stored).to_le_bytes());
        out.extend_from_slice(&k.right_tangent.unwrap_or(k.right_stored).to_le_bytes());
    }
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(n as u32).to_le_bytes());
    for k in keys {
        out.extend_from_slice(&k.tick.to_le_bytes());
    }
    out
}

/// Decodes `MarkerCurve.ValuesAndTimes` (version 1 or 2).
fn decode_marker_curve(data: &[u8]) -> Vec<(Tick, String)> {
    if data.len() < 16 {
        return Vec::new();
    }
    let version = u32::from_le_bytes(data[0..4].try_into().unwrap());
    if version != 1 && version != 2 {
        return Vec::new();
    }
    let n = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    if n > data.len() {
        return Vec::new();
    }
    let mut values = Vec::with_capacity(n);
    let mut off = 8usize;
    for _ in 0..n {
        let Some(rel_end) = data[off..].iter().position(|&b| b == 0) else {
            return Vec::new();
        };
        let s = String::from_utf8_lossy(&data[off..off + rel_end]).into_owned();
        values.push(s);
        off += rel_end + 1;
    }
    let Some(total) = off
        .checked_add(8)
        .and_then(|v| v.checked_add(n.checked_mul(4)?))
    else {
        return Vec::new();
    };
    if total > data.len() {
        return Vec::new();
    }
    let time_ver = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
    let time_n = u32::from_le_bytes(data[off + 4..off + 8].try_into().unwrap()) as usize;
    if time_ver != 1 || time_n != n {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(n);
    for (i, val) in values.into_iter().enumerate() {
        let t_off = off + 8 + i * 4;
        let tick = i32::from_le_bytes(data[t_off..t_off + 4].try_into().unwrap());
        out.push((tick, val));
    }
    out
}

/// Encodes `MarkerCurve.ValuesAndTimes` (version 2).
fn encode_marker_curve(markers: &[(Tick, String)]) -> Vec<u8> {
    let n = markers.len();
    let mut out = Vec::new();
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(n as u32).to_le_bytes());
    for (_, val) in markers {
        out.extend_from_slice(val.as_bytes());
        out.push(0);
    }
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(n as u32).to_le_bytes());
    for (tick, _) in markers {
        out.extend_from_slice(&tick.to_le_bytes());
    }
    out
}

// ============================================================================
// DOM Property Reading Helpers
// ============================================================================

fn read_bool(doc: &Document, r: Ref, name: &str, default: bool) -> bool {
    match doc.prop(r, name) {
        Some(Variant::Bool(b)) => b,
        _ => default,
    }
}

fn read_f32(doc: &Document, r: Ref, name: &str, default: f32) -> f32 {
    match doc.prop(r, name) {
        Some(Variant::Float32(v)) => v,
        Some(Variant::Float64(v)) => v as f32,
        _ => default,
    }
}

fn read_string(doc: &Document, r: Ref, name: &str) -> String {
    match doc.prop(r, name) {
        Some(Variant::String(s)) => s,
        _ => String::new(),
    }
}

fn read_enum_u32(doc: &Document, r: Ref, name: &str) -> Option<u32> {
    match doc.prop(r, name) {
        Some(Variant::Enum(e)) => Some(e.to_u32()),
        Some(Variant::Int32(i)) if i >= 0 => Some(i as u32),
        _ => None,
    }
}

fn read_cframe(doc: &Document, r: Ref, name: &str) -> Xform {
    match doc.prop(r, name) {
        Some(Variant::CFrame(cf)) => Xform::from_cframe(&cf),
        _ => Xform {
            pos: Vec3::ZERO,
            rot: Mat3::IDENTITY,
        },
    }
}

fn read_binary<'a>(doc: &'a Document, r: Ref, name: &str) -> Option<&'a [u8]> {
    let inst = doc.dom.get_by_ref(r)?;
    match inst.properties.get(&rbx_dom_weak::ustr(name)) {
        Some(Variant::BinaryString(bs)) => Some(bs.as_ref()),
        Some(Variant::SharedString(s)) => Some(s.data()),
        Some(Variant::String(s)) => Some(s.as_bytes()),
        _ => None,
    }
}

/// Exact `RigSerialization.findFrameRate` inference from sorted keyframe times.
fn infer_frame_rate(times: &[f32]) -> u32 {
    let mut min_delta = f32::INFINITY;
    let mut prev: Option<f32> = None;
    for &t in times {
        if let Some(p) = prev {
            let d = t - p;
            if d > 1e-6 && d < min_delta {
                min_delta = d;
            }
        }
        prev = Some(t);
    }
    let mut fps = if min_delta.is_finite() && min_delta > 0.0 {
        (1.0 / min_delta).round() as u32
    } else {
        DEFAULT_FRAMERATE
    };
    if fps > 0 {
        if 30 % fps == 0 {
            fps = 30;
        } else if 24 % fps == 0 {
            fps = 24;
        } else if 60 % fps == 0 {
            fps = 60;
        }
    }
    fps.clamp(MIN_FRAMERATE, MAX_FRAMERATE)
}

/// Builds ancestor path (root-to-parent, excluding leaf) for every track
/// matching `RigSerialization.buildHierarchyPaths`.
fn build_hierarchy_paths(
    data: &AnimationData,
    rig: Option<&RigCache>,
) -> BTreeMap<String, Vec<String>> {
    let mut paths = BTreeMap::new();
    let Some(root_inst) = data.instances.get("Root") else {
        return paths;
    };

    let parent_chain = |start: &str| -> Vec<String> {
        let mut chain = Vec::new();
        if let Some(r) = rig {
            let mut cur = start;
            let mut guard = 0usize;
            while let Some(parent) = r.parent_map.get(cur) {
                chain.push(parent.clone());
                cur = parent;
                guard += 1;
                if guard > 256 {
                    break;
                }
            }
            chain.reverse();
        }
        chain
    };

    for (track_name, track) in &root_inst.tracks {
        match track.track_type {
            TrackType::Facs => {
                let mut chain = parent_chain("Head");
                chain.push("Head".to_string());
                chain.push(FACE_CONTROLS_FOLDER.to_string());
                paths.insert(track_name.clone(), chain);
            }
            _ => {
                paths.insert(track_name.clone(), parent_chain(track_name));
            }
        }
    }

    paths
}

// ============================================================================
// 1. KeyframeSequence <-> AnimationData
// ============================================================================

/// Deserializes a `KeyframeSequence` instance in `doc` into [`AnimationData`].
pub fn from_keyframe_sequence(
    doc: &Document,
    seq_ref: Ref,
    frame_rate: u32,
) -> Result<AnimationData, String> {
    let seq_inst = doc
        .dom
        .get_by_ref(seq_ref)
        .ok_or_else(|| "KeyframeSequence reference not found in document".to_string())?;
    if seq_inst.class.as_str() != "KeyframeSequence" {
        return Err(format!(
            "Expected KeyframeSequence, found {}",
            seq_inst.class
        ));
    }

    let guid = read_string(doc, seq_ref, "Guid");
    let mut data =
        AnimationData::new_with_guid(seq_inst.name.clone(), (!guid.is_empty()).then_some(guid));
    data.metadata.looping = read_bool(doc, seq_ref, "Loop", false);
    data.metadata.priority = read_enum_u32(doc, seq_ref, "Priority")
        .and_then(AnimationPriority::from_u32)
        .unwrap_or(AnimationPriority::Core);
    data.metadata.is_channel_animation = false;

    // Collect & sort Keyframe children by Time ascending.
    let mut kf_refs: Vec<(f32, Ref)> = seq_inst
        .children()
        .iter()
        .copied()
        .filter(|&c| {
            doc.dom
                .get_by_ref(c)
                .is_some_and(|ci| ci.class.as_str() == "Keyframe")
        })
        .map(|c| (read_f32(doc, c, "Time", 0.0), c))
        .collect();
    kf_refs.sort_by(|a, b| a.0.total_cmp(&b.0));

    let sorted_times: Vec<f32> = kf_refs.iter().map(|(t, _)| *t).collect();
    data.metadata.frame_rate = if frame_rate == 0 {
        infer_frame_rate(&sorted_times)
    } else {
        frame_rate.clamp(MIN_FRAMERATE, MAX_FRAMERATE)
    };

    let mut max_time = 0.0f32;
    let mut last_kf_ref: Option<Ref> = None;

    for (time, kf_ref) in kf_refs {
        let tick = nearest_tick((time as f64) * (TICK_FREQUENCY as f64)).max(0);
        if time >= max_time {
            max_time = time;
            last_kf_ref = Some(kf_ref);
        }

        if let Some(kf_inst) = doc.dom.get_by_ref(kf_ref) {
            if !kf_inst.name.is_empty() && kf_inst.name != DEFAULT_KEYFRAME_NAME {
                data.set_keyframe_name(tick, Some(kf_inst.name.clone()));
            }
            for &child in kf_inst.children() {
                if let Some(ci) = doc.dom.get_by_ref(child) {
                    if ci.class.as_str() == "KeyframeMarker" {
                        let val = read_string(doc, child, "Value");
                        data.add_event(tick, ci.name.clone(), val);
                    } else {
                        traverse_poses(doc, child, tick, &mut data);
                    }
                }
            }
        }
    }

    let end_tick = nearest_tick((max_time as f64) * (TICK_FREQUENCY as f64)).max(0);

    // Trailing empty keyframe handling (`RigSerialization.fromKeyframeSequence`):
    // if last Keyframe has no children, duplicate each track's last value at end_tick.
    if let Some(last_ref) = last_kf_ref {
        if doc
            .dom
            .get_by_ref(last_ref)
            .is_some_and(|i| i.children().is_empty())
        {
            if let Some(root_inst) = data.instances.get_mut("Root") {
                for track in root_inst.tracks.values_mut() {
                    if let Some(&last_t) = track.keyframes.last() {
                        if last_t < end_tick {
                            if let Some(last_kf) = track.data.get(&last_t).copied() {
                                track.add_keyframe(end_tick, Keyframe::new_pose(last_kf.value));
                            }
                        }
                    }
                }
            }
        }
    }

    data.metadata.end_tick = end_tick;
    Ok(data)
}

fn traverse_poses(doc: &Document, node: Ref, tick: Tick, data: &mut AnimationData) {
    let Some(inst) = doc.dom.get_by_ref(node) else {
        return;
    };
    let class = inst.class.as_str();
    let is_pose = class == "Pose";
    let is_number_pose = class == "NumberPose";
    let is_face_folder = class == "Folder" && inst.name == FACE_CONTROLS_FOLDER;

    if is_pose || is_number_pose {
        let weight = read_f32(doc, node, "Weight", 1.0);
        if weight != 0.0 {
            let track_type = if is_pose {
                TrackType::CFrame
            } else {
                TrackType::Facs
            };
            let value = if is_pose {
                KeyframeValue::CFrame(read_cframe(doc, node, "CFrame"))
            } else {
                KeyframeValue::Facs(read_f32(doc, node, "Value", 0.0))
            };
            let easing_style = read_enum_u32(doc, node, "EasingStyle")
                .and_then(EasingStyle::from_u32)
                .unwrap_or(EasingStyle::Linear);
            let easing_direction = read_enum_u32(doc, node, "EasingDirection")
                .and_then(EasingDirection::from_u32)
                .unwrap_or(EasingDirection::In);

            let name = inst.name.clone();
            let root = data.instances.entry("Root".to_string()).or_default();
            let track = root
                .tracks
                .entry(name)
                .or_insert_with(|| Track::new(track_type));
            track.add_keyframe(
                tick,
                Keyframe {
                    value,
                    easing_style: Some(easing_style),
                    easing_direction: Some(easing_direction),
                    interpolation_mode: None,
                    left_slope: None,
                    right_slope: None,
                },
            );
        }
    }

    if is_pose || is_number_pose || is_face_folder {
        let children: Vec<Ref> = inst.children().to_vec();
        for c in children {
            traverse_poses(doc, c, tick, data);
        }
    }
}

/// Tree node used to assemble nested `Pose` / `NumberPose` / `Folder` trees
/// per `Keyframe` before converting to `InstanceBuilder`.
struct PoseTreeNode {
    class: &'static str,
    name: String,
    cframe: Option<Xform>,
    number_value: Option<f32>,
    weight: f32,
    easing_style: EasingStyle,
    easing_direction: EasingDirection,
    children: BTreeMap<String, PoseTreeNode>,
}

impl PoseTreeNode {
    fn new_placeholder(name: &str) -> Self {
        let is_face = name == FACE_CONTROLS_FOLDER;
        Self {
            class: if is_face { "Folder" } else { "Pose" },
            name: name.to_string(),
            cframe: (!is_face).then_some(Xform {
                pos: Vec3::ZERO,
                rot: Mat3::IDENTITY,
            }),
            number_value: None,
            weight: 0.0,
            easing_style: EasingStyle::Linear,
            easing_direction: EasingDirection::In,
            children: BTreeMap::new(),
        }
    }

    fn into_builder(self) -> InstanceBuilder {
        let mut b = InstanceBuilder::new(self.class).with_name(self.name);
        match self.class {
            "Pose" => {
                let cf = self.cframe.unwrap_or(Xform {
                    pos: Vec3::ZERO,
                    rot: Mat3::IDENTITY,
                });
                b = b
                    .with_property("CFrame", Variant::CFrame(cf.to_cframe()))
                    .with_property("Weight", Variant::Float32(self.weight))
                    .with_property(
                        "EasingStyle",
                        Variant::Enum(Enum::from_u32(self.easing_style.to_u32())),
                    )
                    .with_property(
                        "EasingDirection",
                        Variant::Enum(Enum::from_u32(self.easing_direction.to_u32())),
                    );
            }
            "NumberPose" => {
                b = b
                    .with_property("Value", Variant::Float32(self.number_value.unwrap_or(0.0)))
                    .with_property("Weight", Variant::Float32(self.weight))
                    .with_property(
                        "EasingStyle",
                        Variant::Enum(Enum::from_u32(self.easing_style.to_u32())),
                    )
                    .with_property(
                        "EasingDirection",
                        Variant::Enum(Enum::from_u32(self.easing_direction.to_u32())),
                    );
            }
            _ => {}
        }
        for (_, child) in self.children {
            b = b.with_child(child.into_builder());
        }
        b
    }
}

/// Serializes [`AnimationData`] into a `KeyframeSequence` instance parented
/// under `parent` in `doc`.
pub fn to_keyframe_sequence(
    doc: &mut Document,
    parent: Ref,
    data: &AnimationData,
    rig: Option<&RigCache>,
) -> Ref {
    let mut seq = InstanceBuilder::new("KeyframeSequence")
        .with_name(data.metadata.name.clone())
        .with_property("Loop", Variant::Bool(data.metadata.looping))
        .with_property(
            "Priority",
            Variant::Enum(Enum::from_u32(data.metadata.priority.to_u32())),
        );

    let hierarchy_paths = build_hierarchy_paths(data, rig);

    // Collect all ticks from tracks, events, and named keyframes.
    let mut all_ticks: BTreeSet<Tick> = BTreeSet::new();
    if let Some(root_inst) = data.instances.get("Root") {
        for track in root_inst.tracks.values() {
            for &t in &track.keyframes {
                all_ticks.insert(t);
            }
        }
    }
    for &t in &data.events.keyframes {
        all_ticks.insert(t);
    }
    for &t in data.events.named_keyframes.keys() {
        all_ticks.insert(t);
    }

    for tick in all_ticks {
        let kf_name = data
            .events
            .named_keyframes
            .get(&tick)
            .cloned()
            .unwrap_or_else(|| DEFAULT_KEYFRAME_NAME.to_string());
        let mut kf_builder = InstanceBuilder::new("Keyframe")
            .with_name(kf_name)
            .with_property("Time", Variant::Float32(tick_to_seconds(tick)));

        let mut pose_roots: BTreeMap<String, PoseTreeNode> = BTreeMap::new();

        if let Some(root_inst) = data.instances.get("Root") {
            for (track_name, track) in &root_inst.tracks {
                let Some(kf) = track.data.get(&tick) else {
                    continue;
                };
                let ancestors = hierarchy_paths
                    .get(track_name)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);

                let mut map_ref = &mut pose_roots;
                for anc in ancestors {
                    let entry = map_ref
                        .entry(anc.clone())
                        .or_insert_with(|| PoseTreeNode::new_placeholder(anc));
                    map_ref = &mut entry.children;
                }

                let node = map_ref
                    .entry(track_name.clone())
                    .or_insert_with(|| PoseTreeNode::new_placeholder(track_name));
                node.weight = 1.0;
                node.easing_style = kf.easing_style.unwrap_or(EasingStyle::Linear);
                node.easing_direction = kf.easing_direction.unwrap_or(EasingDirection::In);

                if track.track_type == TrackType::Facs {
                    node.class = "NumberPose";
                    node.cframe = None;
                    node.number_value = Some(kf.value.as_f32().unwrap_or(0.0));
                } else {
                    node.class = "Pose";
                    node.cframe = Some(kf.value.as_cframe().unwrap_or(Xform {
                        pos: Vec3::ZERO,
                        rot: Mat3::IDENTITY,
                    }));
                    node.number_value = None;
                }
            }
        }

        for (_, root_node) in pose_roots {
            kf_builder = kf_builder.with_child(root_node.into_builder());
        }

        if let Some(ev_map) = data.events.data.get(&tick) {
            for (ev_name, ev_val) in ev_map {
                kf_builder = kf_builder.with_child(
                    InstanceBuilder::new("KeyframeMarker")
                        .with_name(ev_name.clone())
                        .with_property("Value", Variant::String(ev_val.clone())),
                );
            }
        }

        seq = seq.with_child(kf_builder);
    }

    doc.insert(parent, seq)
}

// ============================================================================
// 2. CurveAnimation <-> AnimationData
// ============================================================================

fn read_float_curve_into_track(doc: &Document, fc_ref: Ref, track: &mut Track) {
    track.is_curve_track = true;
    let Some(bytes) = read_binary(doc, fc_ref, "ValuesAndTimes") else {
        return;
    };
    let freq = TICK_FREQUENCY as f32;
    for k in decode_float_curve_keys(bytes) {
        let value = match track.track_type {
            TrackType::Facs => KeyframeValue::Facs(k.value),
            TrackType::Angle => KeyframeValue::Angle(k.value),
            _ => KeyframeValue::Number(k.value),
        };
        track.add_keyframe(
            k.tick.max(0),
            Keyframe {
                value,
                easing_style: None,
                easing_direction: None,
                interpolation_mode: Some(k.mode),
                left_slope: k.left_tangent.map(|t| t / freq),
                right_slope: k.right_tangent.map(|t| t / freq),
            },
        );
    }
}

fn read_rotation_curve_into_track(doc: &Document, rc_ref: Ref, track: &mut Track) {
    track.is_curve_track = true;
    let Some(bytes) = read_binary(doc, rc_ref, "ValuesAndTimes") else {
        return;
    };
    let freq = TICK_FREQUENCY as f32;
    for k in decode_rotation_curve_keys(bytes) {
        track.add_keyframe(
            k.tick.max(0),
            Keyframe {
                value: KeyframeValue::Quaternion(k.quat),
                easing_style: None,
                easing_direction: None,
                interpolation_mode: Some(k.mode),
                left_slope: k.left_tangent.map(|t| t / freq),
                right_slope: k.right_tangent.map(|t| t / freq),
            },
        );
    }
}

fn find_child_named(doc: &Document, parent: Ref, name: &str) -> Option<Ref> {
    let inst = doc.dom.get_by_ref(parent)?;
    inst.children()
        .iter()
        .copied()
        .find(|&c| doc.dom.get_by_ref(c).is_some_and(|ci| ci.name == name))
}

fn read_vector3_curve(doc: &Document, v3_ref: Ref) -> Track {
    let mut pos_track = Track::new_curve(TrackType::Position);
    for axis in [PROP_X, PROP_Y, PROP_Z] {
        let mut axis_track = Track::new_curve(TrackType::Number);
        if let Some(fc_ref) = find_child_named(doc, v3_ref, axis) {
            if doc
                .class_of(fc_ref)
                .is_some_and(|c| c.as_str() == "FloatCurve")
            {
                read_float_curve_into_track(doc, fc_ref, &mut axis_track);
            }
        }
        pos_track.components.insert(axis.to_string(), axis_track);
    }
    pos_track
}

fn read_euler_rotation_curve(doc: &Document, er_ref: Ref) -> Track {
    let order = read_enum_u32(doc, er_ref, "RotationOrder")
        .and_then(RotationOrder::from_u32)
        .unwrap_or(RotationOrder::XYZ);
    let mut rot_track = Track::new_curve(TrackType::EulerAngles);
    rot_track.euler_angles_order = Some(order);
    for axis in [PROP_X, PROP_Y, PROP_Z] {
        let mut axis_track = Track::new_curve(TrackType::Angle);
        if let Some(fc_ref) = find_child_named(doc, er_ref, axis) {
            if doc
                .class_of(fc_ref)
                .is_some_and(|c| c.as_str() == "FloatCurve")
            {
                read_float_curve_into_track(doc, fc_ref, &mut axis_track);
            }
        }
        rot_track.components.insert(axis.to_string(), axis_track);
    }
    rot_track
}

fn traverse_curve_folders(doc: &Document, folder_ref: Ref, data: &mut AnimationData) {
    let Some(folder_inst) = doc.dom.get_by_ref(folder_ref) else {
        return;
    };
    let folder_name = folder_inst.name.clone();
    let children: Vec<Ref> = folder_inst.children().to_vec();

    if folder_name == FACE_CONTROLS_FOLDER {
        for c in children {
            if let Some(ci) = doc.dom.get_by_ref(c) {
                if ci.class.as_str() == "FloatCurve" {
                    let mut track = Track::new_curve(TrackType::Facs);
                    read_float_curve_into_track(doc, c, &mut track);
                    let root = data.instances.entry("Root".to_string()).or_default();
                    root.tracks.insert(ci.name.clone(), track);
                }
            }
        }
        return;
    }

    if folder_name == "IkTargets" {
        for c in children {
            if let Some(ci) = doc.dom.get_by_ref(c) {
                if ci.class.as_str() == "ObjectValue" {
                    data.metadata.ik_targets.insert(ci.name.clone(), true);
                }
            }
        }
        return;
    }

    let pos_child = find_child_named(doc, folder_ref, PROP_POSITION);
    let rot_child = find_child_named(doc, folder_ref, PROP_ROTATION);

    if pos_child.is_some() || rot_child.is_some() {
        let mut cf_track = Track::new_curve(TrackType::CFrame);

        if let Some(p_ref) = pos_child {
            if doc
                .class_of(p_ref)
                .is_some_and(|c| c.as_str() == "Vector3Curve")
            {
                cf_track
                    .components
                    .insert(PROP_POSITION.to_string(), read_vector3_curve(doc, p_ref));
            }
        }
        if let Some(r_ref) = rot_child {
            let r_class = doc.class_of(r_ref);
            if r_class.as_deref() == Some("RotationCurve") {
                let mut q_track = Track::new_curve(TrackType::Quaternion);
                read_rotation_curve_into_track(doc, r_ref, &mut q_track);
                cf_track
                    .components
                    .insert(PROP_ROTATION.to_string(), q_track);
            } else if r_class.as_deref() == Some("EulerRotationCurve") {
                cf_track.components.insert(
                    PROP_ROTATION.to_string(),
                    read_euler_rotation_curve(doc, r_ref),
                );
            }
        }

        let root = data.instances.entry("Root".to_string()).or_default();
        root.tracks.insert(folder_name, cf_track);
    }

    // Continue recursing into child Folders (for child joints & nested FaceControls).
    for c in children {
        if doc.class_of(c).is_some_and(|cl| cl.as_str() == "Folder") {
            traverse_curve_folders(doc, c, data);
        }
    }
}

/// Deserializes a `CurveAnimation` instance in `doc` into [`AnimationData`].
pub fn from_curve_animation(
    doc: &Document,
    curve_ref: Ref,
    frame_rate: u32,
) -> Result<AnimationData, String> {
    let curve_inst = doc
        .dom
        .get_by_ref(curve_ref)
        .ok_or_else(|| "CurveAnimation reference not found in document".to_string())?;
    if curve_inst.class.as_str() != "CurveAnimation" {
        return Err(format!(
            "Expected CurveAnimation, found {}",
            curve_inst.class
        ));
    }

    let guid = read_string(doc, curve_ref, "Guid");
    let mut data =
        AnimationData::new_with_guid(curve_inst.name.clone(), (!guid.is_empty()).then_some(guid));
    data.metadata.looping = read_bool(doc, curve_ref, "Loop", false);
    data.metadata.priority = read_enum_u32(doc, curve_ref, "Priority")
        .and_then(AnimationPriority::from_u32)
        .unwrap_or(AnimationPriority::Core);
    data.metadata.is_channel_animation = true;
    data.metadata.frame_rate = if frame_rate == 0 {
        DEFAULT_FRAMERATE
    } else {
        frame_rate.clamp(MIN_FRAMERATE, MAX_FRAMERATE)
    };

    let children: Vec<Ref> = curve_inst.children().to_vec();
    for c in children {
        let Some(ci) = doc.dom.get_by_ref(c) else {
            continue;
        };
        match ci.class.as_str() {
            "Folder" => traverse_curve_folders(doc, c, &mut data),
            "MarkerCurve" => {
                if let Some(bytes) = read_binary(doc, c, "ValuesAndTimes") {
                    let markers = decode_marker_curve(bytes);
                    if ci.name == NAMED_KEYFRAMES_CHANNEL {
                        for (tick, name) in markers {
                            data.set_keyframe_name(tick.max(0), Some(name));
                        }
                    } else {
                        for (tick, val) in markers {
                            data.add_event(tick.max(0), ci.name.clone(), val);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    data.set_end_tick();
    Ok(data)
}

fn build_float_curve_builder(name: &str, track: &Track) -> InstanceBuilder {
    let freq = TICK_FREQUENCY as f32;
    let mut wire_keys = Vec::with_capacity(track.keyframes.len());
    let mut prev_mode: Option<KeyInterpolationMode> = None;

    for &tick in &track.keyframes {
        let Some(kf) = track.data.get(&tick) else {
            continue;
        };
        let mode = kf.interpolation_mode.unwrap_or(KeyInterpolationMode::Cubic);
        // Studio rule (`populateFloatCurve`):
        // Explicit LeftTangent flag only if prev key exists, prev key is Cubic, and LeftSlope != nil.
        // Stored left_stored holds the effective (auto or explicit) tangent for runtime evaluation.
        let (left_tangent, left_stored) = if prev_mode == Some(KeyInterpolationMode::Cubic) {
            let explicit = kf.left_slope.map(|s| s * freq);
            let eff = kf
                .left_slope
                .unwrap_or_else(|| curves::get_slope(track, tick, TangentSide::Left).unwrap_or(0.0))
                * freq;
            (explicit, eff)
        } else {
            (None, 0.0)
        };
        // Explicit RightTangent flag only if current key is Cubic and RightSlope != nil.
        // Stored right_stored holds the effective (auto or explicit) tangent for runtime evaluation.
        let (right_tangent, right_stored) = if mode == KeyInterpolationMode::Cubic {
            let explicit = kf.right_slope.map(|s| s * freq);
            let eff = kf
                .right_slope
                .unwrap_or_else(|| curves::get_slope(track, tick, TangentSide::Right).unwrap_or(0.0))
                * freq;
            (explicit, eff)
        } else {
            (None, 0.0)
        };
        wire_keys.push(FloatKeyWire {
            tick,
            value: kf.value.as_f32().unwrap_or(0.0),
            mode,
            left_tangent,
            right_tangent,
            left_stored,
            right_stored,
        });
        prev_mode = Some(mode);
    }

    let bytes = encode_float_curve_keys(&wire_keys);
    InstanceBuilder::new("FloatCurve")
        .with_name(name.to_string())
        .with_property(
            "ValuesAndTimes",
            Variant::BinaryString(BinaryString::from(bytes)),
        )
}

fn build_rotation_curve_builder(name: &str, track: &Track) -> InstanceBuilder {
    let freq = TICK_FREQUENCY as f32;
    let mut wire_keys = Vec::with_capacity(track.keyframes.len());
    let mut prev_mode: Option<KeyInterpolationMode> = None;

    for &tick in &track.keyframes {
        let Some(kf) = track.data.get(&tick) else {
            continue;
        };
        let mode = kf.interpolation_mode.unwrap_or(KeyInterpolationMode::Cubic);
        let (left_tangent, left_stored) = if prev_mode == Some(KeyInterpolationMode::Cubic) {
            let explicit = kf.left_slope.map(|s| s * freq);
            let eff = kf
                .left_slope
                .unwrap_or_else(|| curves::get_slope(track, tick, TangentSide::Left).unwrap_or(0.0))
                * freq;
            (explicit, eff)
        } else {
            (None, 0.0)
        };
        let (right_tangent, right_stored) = if mode == KeyInterpolationMode::Cubic {
            let explicit = kf.right_slope.map(|s| s * freq);
            let eff = kf
                .right_slope
                .unwrap_or_else(|| curves::get_slope(track, tick, TangentSide::Right).unwrap_or(0.0))
                * freq;
            (explicit, eff)
        } else {
            (None, 0.0)
        };
        wire_keys.push(RotationKeyWire {
            tick,
            quat: kf.value.as_quat().unwrap_or(Quat::IDENTITY),
            mode,
            left_tangent,
            right_tangent,
            left_stored,
            right_stored,
        });
        prev_mode = Some(mode);
    }

    let bytes = encode_rotation_curve_keys(&wire_keys);
    InstanceBuilder::new("RotationCurve")
        .with_name(name.to_string())
        .with_property(
            "ValuesAndTimes",
            Variant::BinaryString(BinaryString::from(bytes)),
        )
}

fn build_vector3_curve_builder(name: &str, track: &Track) -> InstanceBuilder {
    let mut b = InstanceBuilder::new("Vector3Curve").with_name(name.to_string());
    for axis in [PROP_X, PROP_Y, PROP_Z] {
        if let Some(axis_track) = track.components.get(axis) {
            b = b.with_child(build_float_curve_builder(axis, axis_track));
        } else {
            b = b.with_child(build_float_curve_builder(
                axis,
                &Track::new_curve(TrackType::Number),
            ));
        }
    }
    b
}

fn build_euler_rotation_curve_builder(name: &str, track: &Track) -> InstanceBuilder {
    let order = track.euler_angles_order.unwrap_or(RotationOrder::XYZ);
    let mut b = InstanceBuilder::new("EulerRotationCurve")
        .with_name(name.to_string())
        .with_property(
            "RotationOrder",
            Variant::Enum(Enum::from_u32(order.to_u32())),
        );
    for axis in [PROP_X, PROP_Y, PROP_Z] {
        if let Some(axis_track) = track.components.get(axis) {
            b = b.with_child(build_float_curve_builder(axis, axis_track));
        } else {
            b = b.with_child(build_float_curve_builder(
                axis,
                &Track::new_curve(TrackType::Angle),
            ));
        }
    }
    b
}

struct CurveFolderNode {
    name: String,
    curves: Vec<InstanceBuilder>,
    subfolders: BTreeMap<String, CurveFolderNode>,
}

impl CurveFolderNode {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            curves: Vec::new(),
            subfolders: BTreeMap::new(),
        }
    }

    fn into_builder(self) -> InstanceBuilder {
        let mut b = InstanceBuilder::new("Folder").with_name(self.name);
        for c in self.curves {
            b = b.with_child(c);
        }
        for (_, sub) in self.subfolders {
            b = b.with_child(sub.into_builder());
        }
        b
    }
}

/// Serializes [`AnimationData`] into a `CurveAnimation` instance parented
/// under `parent` in `doc`.
pub fn to_curve_animation(
    doc: &mut Document,
    parent: Ref,
    data: &AnimationData,
    rig: Option<&RigCache>,
) -> Ref {
    let mut anim = InstanceBuilder::new("CurveAnimation")
        .with_name(data.metadata.name.clone())
        .with_property("Loop", Variant::Bool(data.metadata.looping))
        .with_property(
            "Priority",
            Variant::Enum(Enum::from_u32(data.metadata.priority.to_u32())),
        );

    let hierarchy_paths = build_hierarchy_paths(data, rig);
    let mut root_folders: BTreeMap<String, CurveFolderNode> = BTreeMap::new();

    if let Some(root_inst) = data.instances.get("Root") {
        for (track_name, track) in &root_inst.tracks {
            let ancestors = hierarchy_paths
                .get(track_name)
                .map(Vec::as_slice)
                .unwrap_or(&[]);

            if track.track_type == TrackType::Facs {
                // Place FloatCurve under the FaceControls folder along its path.
                let mut map_ref = &mut root_folders;
                let path: &[String] = if ancestors.is_empty() { &[] } else { ancestors };
                if path.is_empty() {
                    let fc_folder = map_ref
                        .entry(FACE_CONTROLS_FOLDER.to_string())
                        .or_insert_with(|| CurveFolderNode::new(FACE_CONTROLS_FOLDER));
                    fc_folder
                        .curves
                        .push(build_float_curve_builder(track_name, track));
                } else {
                    for seg in &path[..path.len() - 1] {
                        let node = map_ref
                            .entry(seg.clone())
                            .or_insert_with(|| CurveFolderNode::new(seg));
                        map_ref = &mut node.subfolders;
                    }
                    let last = &path[path.len() - 1];
                    let fc_folder = map_ref
                        .entry(last.clone())
                        .or_insert_with(|| CurveFolderNode::new(last));
                    fc_folder
                        .curves
                        .push(build_float_curve_builder(track_name, track));
                }
            } else if track.track_type == TrackType::CFrame {
                let mut map_ref = &mut root_folders;
                for anc in ancestors {
                    let node = map_ref
                        .entry(anc.clone())
                        .or_insert_with(|| CurveFolderNode::new(anc));
                    map_ref = &mut node.subfolders;
                }
                let joint_folder = map_ref
                    .entry(track_name.clone())
                    .or_insert_with(|| CurveFolderNode::new(track_name));

                if let Some(pos_track) = track.components.get(PROP_POSITION) {
                    joint_folder
                        .curves
                        .push(build_vector3_curve_builder(PROP_POSITION, pos_track));
                }
                if let Some(rot_track) = track.components.get(PROP_ROTATION) {
                    if rot_track.track_type == TrackType::Quaternion {
                        joint_folder
                            .curves
                            .push(build_rotation_curve_builder(PROP_ROTATION, rot_track));
                    } else {
                        joint_folder
                            .curves
                            .push(build_euler_rotation_curve_builder(PROP_ROTATION, rot_track));
                    }
                }
            }
        }
    }

    for (_, folder) in root_folders {
        anim = anim.with_child(folder.into_builder());
    }

    // MarkerCurves for animation events + NamedKeyframeEvent.
    let mut markers_by_name: BTreeMap<String, Vec<(Tick, String)>> = BTreeMap::new();
    for (&tick, ev_map) in &data.events.data {
        for (ev_name, ev_val) in ev_map {
            markers_by_name
                .entry(ev_name.clone())
                .or_default()
                .push((tick, ev_val.clone()));
        }
    }
    if !data.events.named_keyframes.is_empty() {
        let entry = markers_by_name
            .entry(NAMED_KEYFRAMES_CHANNEL.to_string())
            .or_default();
        for (&tick, name) in &data.events.named_keyframes {
            entry.push((tick, name.clone()));
        }
    }
    for (name, mut markers) in markers_by_name {
        markers.sort_by_key(|(t, _)| *t);
        let bytes = encode_marker_curve(&markers);
        anim = anim.with_child(
            InstanceBuilder::new("MarkerCurve")
                .with_name(name)
                .with_property(
                    "ValuesAndTimes",
                    Variant::BinaryString(BinaryString::from(bytes)),
                ),
        );
    }

    doc.insert(parent, anim)
}

// ============================================================================
// 3. FBX Animation Import (`rustudio_core::fbx_anim::read`)
// ============================================================================

/// Imports a binary FBX 7.x animation file into [`AnimationData`].
pub fn import_fbx_file(
    path: &Path,
    rig: &RigCache,
    scale_factor: f32,
    is_r15: bool,
) -> Result<AnimationData, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Failed to read FBX file: {e}"))?;
    let clip = rustudio_core::fbx_anim::read(&bytes, is_r15)?;

    let clip_name = if !clip.name.is_empty() {
        clip.name.clone()
    } else {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(DEFAULT_IMPORTED_NAME)
            .to_string()
    };

    let times_f32: Vec<f32> = clip.times.iter().map(|&t| t as f32).collect();
    let fps = infer_frame_rate(&times_f32);

    let mut data = AnimationData::new_rig_animation(clip_name);
    data.metadata.frame_rate = fps;
    data.metadata.looping = true;
    data.metadata.priority = AnimationPriority::Action;

    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };

    // Prefer filtering to bones present on the rig; if none match, keep all bones.
    let any_rig_match = !rig.joints_by_part.is_empty()
        && clip
            .bones
            .iter()
            .any(|b| rig.joints_by_part.contains_key(&b.name));

    let root = data.instances.entry("Root".to_string()).or_default();
    for bone in &clip.bones {
        if any_rig_match && !rig.joints_by_part.contains_key(&bone.name) {
            continue;
        }
        let mut track = Track::new(TrackType::CFrame);
        for (i, &t) in clip.times.iter().enumerate() {
            let Some(xf) = bone.poses.get(i) else {
                continue;
            };
            let pos = xf.pos.as_vec3();
            let rot = xf.rot.as_mat3();
            let safe_pos = if pos.is_finite() {
                pos * scale
            } else {
                Vec3::ZERO
            };
            let safe_rot = if rot.is_finite() { rot } else { Mat3::IDENTITY };
            let tick = seconds_to_tick(t as f32).max(0);
            track.add_keyframe(
                tick,
                Keyframe::new_pose(KeyframeValue::CFrame(Xform {
                    pos: safe_pos,
                    rot: safe_rot,
                })),
            );
        }
        if !track.is_empty() {
            root.tracks.insert(bone.name.clone(), track);
        }
    }

    data.set_end_tick();
    Ok(data)
}

// ============================================================================
// 4. `.rbxm` / `.rbxmx` Export & Import
// ============================================================================

/// Exports `data` to a standalone `.rbxm` (binary) or `.rbxmx` (XML) file
/// without mutating `doc`'s undo stack or tree.
pub fn export_clip_to_rbxm(
    _doc: &mut Document,
    data: &AnimationData,
    rig: Option<&RigCache>,
    path: &Path,
) -> Result<(), String> {
    let mut scratch = Document::new_place();
    let root = scratch.dom.root_ref();
    let clip_ref = if data.is_channel_animation() {
        to_curve_animation(&mut scratch, root, data, rig)
    } else {
        to_keyframe_sequence(&mut scratch, root, data, rig)
    };

    let xml = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rbxmx") || e.eq_ignore_ascii_case("rbxlx"));
    let mut bytes = Vec::new();
    if xml {
        rbx_xml::to_writer_default(&mut bytes, &scratch.dom, &[clip_ref])
            .map_err(|e| format!("Failed to encode XML model: {e}"))?;
    } else {
        rbx_binary::to_writer(&mut bytes, &scratch.dom, &[clip_ref])
            .map_err(|e| format!("Failed to encode binary model: {e}"))?;
    }

    std::fs::write(path, bytes).map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

/// Imports the first `KeyframeSequence` or `CurveAnimation` found in an
/// `.rbxm` / `.rbxmx` file into [`AnimationData`].
pub fn import_clip_from_rbxm(path: &Path, frame_rate: u32) -> Result<AnimationData, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let looks_xml = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rbxmx") || e.eq_ignore_ascii_case("rbxlx"))
        || bytes.starts_with(b"<roblox")
        || bytes.starts_with(b"<?xml");

    let mut parsed_dom: WeakDom = if looks_xml {
        rbx_xml::from_reader_default(Cursor::new(&bytes))
            .map_err(|e| format!("Failed to decode XML model: {e}"))?
    } else {
        rbx_binary::from_reader(Cursor::new(&bytes))
            .map_err(|e| format!("Failed to decode binary model: {e}"))?
    };

    // Transfer decoded top-level instances into a scratch Document so we can
    // reuse `from_keyframe_sequence` / `from_curve_animation`.
    let mut scratch = Document::new_place();
    let scratch_root = scratch.dom.root_ref();
    let top_children: Vec<Ref> = parsed_dom.root().children().to_vec();
    for child in top_children {
        parsed_dom.transfer(child, &mut scratch.dom, scratch_root);
    }

    let clip_ref = scratch
        .dom
        .descendants_of(scratch_root)
        .find(|inst| matches!(inst.class.as_str(), "KeyframeSequence" | "CurveAnimation"))
        .map(|inst| inst.referent())
        .ok_or_else(|| "No KeyframeSequence or CurveAnimation found in file".to_string())?;

    let class = scratch.class_of(clip_ref);
    if class.as_deref() == Some("CurveAnimation") {
        from_curve_animation(&scratch, clip_ref, frame_rate)
    } else {
        from_keyframe_sequence(&scratch, clip_ref, frame_rate)
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim_editor::rig::{
        build_rig_cache, is_valid_rig_candidate, list_anim_saves, migrate_legacy_anim_saves,
        save_clip_to_anim_saves, validate_rig,
    };
    use crate::anim_editor::state::{MigrationChoice, ValidationSeverity};
    use rbx_dom_weak::types::CFrame;

    fn make_simple_r15_rig(doc: &mut Document) -> Ref {
        let ws = doc.workspace().unwrap();
        let model = doc.insert(ws, InstanceBuilder::new("Model").with_name("TestRig"));
        let _hum = doc.insert(
            model,
            InstanceBuilder::new("Humanoid")
                .with_name("Humanoid")
                .with_property("RigType", Variant::Enum(Enum::from_u32(1))),
        );
        let hrp = doc.insert(
            model,
            InstanceBuilder::new("Part")
                .with_name("HumanoidRootPart")
                .with_property("Anchored", Variant::Bool(true))
                .with_property("CFrame", Variant::CFrame(CFrame::identity())),
        );
        let torso = doc.insert(
            model,
            InstanceBuilder::new("Part")
                .with_name("UpperTorso")
                .with_property("Anchored", Variant::Bool(false))
                .with_property("CFrame", Variant::CFrame(CFrame::identity())),
        );
        let head = doc.insert(
            model,
            InstanceBuilder::new("Part")
                .with_name("Head")
                .with_property("Anchored", Variant::Bool(false))
                .with_property("CFrame", Variant::CFrame(CFrame::identity())),
        );
        let _root_motor = doc.insert(
            torso,
            InstanceBuilder::new("Motor6D")
                .with_name("Waist")
                .with_property("Part0", Variant::Ref(hrp))
                .with_property("Part1", Variant::Ref(torso)),
        );
        let _neck_motor = doc.insert(
            head,
            InstanceBuilder::new("Motor6D")
                .with_name("Neck")
                .with_property("Part0", Variant::Ref(torso))
                .with_property("Part1", Variant::Ref(head)),
        );
        let _fc = doc.insert(
            head,
            InstanceBuilder::new("FaceControls").with_name("FaceControls"),
        );
        doc.set_prop_live(model, rbx_dom_weak::ustr("PrimaryPart"), Variant::Ref(hrp));
        model
    }

    #[test]
    fn keyframe_sequence_roundtrip_with_facs_and_events() {
        let mut doc = Document::new_place();
        let rig_ref = make_simple_r15_rig(&mut doc);
        let rig = build_rig_cache(&doc, rig_ref);

        let mut anim = AnimationData::new_rig_animation("WaveClip");
        anim.metadata.looping = true;
        anim.metadata.priority = AnimationPriority::Action;
        anim.metadata.frame_rate = 30;

        let torso_xf =
            Xform::from_orientation_deg(Vec3::new(0.0, 1.5, -0.5), Vec3::new(15.0, 30.0, 0.0));
        let track = anim.add_track(
            "Root",
            "UpperTorso",
            TrackType::CFrame,
            TrackType::EulerAngles,
            RotationOrder::XYZ,
        );
        track.add_keyframe(
            0,
            Keyframe {
                value: KeyframeValue::CFrame(Xform {
                    pos: Vec3::ZERO,
                    rot: Mat3::IDENTITY,
                }),
                easing_style: Some(EasingStyle::Linear),
                easing_direction: Some(EasingDirection::In),
                interpolation_mode: None,
                left_slope: None,
                right_slope: None,
            },
        );
        track.add_keyframe(
            1200,
            Keyframe {
                value: KeyframeValue::CFrame(torso_xf),
                easing_style: Some(EasingStyle::CubicV2),
                easing_direction: Some(EasingDirection::InOut),
                interpolation_mode: None,
                left_slope: None,
                right_slope: None,
            },
        );

        let facs = anim.add_track(
            "Root",
            "JawDrop",
            TrackType::Facs,
            TrackType::EulerAngles,
            RotationOrder::XYZ,
        );
        facs.add_keyframe(1200, Keyframe::new_pose(KeyframeValue::Facs(0.75)));

        anim.add_event(1200, "Footstep", "Left");
        anim.set_keyframe_name(1200, Some("Apex".to_string()));
        anim.set_end_tick();

        let ws = doc.workspace().unwrap();
        let seq_ref = to_keyframe_sequence(&mut doc, ws, &anim, Some(&rig));
        let back = from_keyframe_sequence(&doc, seq_ref, 30).unwrap();

        assert_eq!(back.metadata.name, "WaveClip");
        assert!(back.metadata.looping);
        assert_eq!(back.metadata.priority, AnimationPriority::Action);
        assert_eq!(back.metadata.end_tick, 1200);
        assert_eq!(
            back.events.named_keyframes.get(&1200).map(String::as_str),
            Some("Apex")
        );
        assert_eq!(
            back.events
                .data
                .get(&1200)
                .and_then(|m| m.get("Footstep"))
                .map(String::as_str),
            Some("Left")
        );

        let back_torso = back.get_track("Root", &["UpperTorso".to_string()]).unwrap();
        let kf = back_torso.data.get(&1200).unwrap();
        assert_eq!(kf.easing_style, Some(EasingStyle::CubicV2));
        assert_eq!(kf.easing_direction, Some(EasingDirection::InOut));
        assert!(kf
            .value
            .is_approximately_equal(&KeyframeValue::CFrame(torso_xf)));

        let back_facs = back.get_track("Root", &["JawDrop".to_string()]).unwrap();
        let fkf = back_facs.data.get(&1200).unwrap();
        assert_eq!(back_facs.track_type, TrackType::Facs);
        assert!((fkf.value.as_f32().unwrap() - 0.75).abs() < 1e-4);
    }

    #[test]
    fn curve_animation_roundtrip_with_tangents_facs_and_markers() {
        let mut doc = Document::new_place();
        let rig_ref = make_simple_r15_rig(&mut doc);
        let rig = build_rig_cache(&doc, rig_ref);

        let mut anim = AnimationData::new_rig_animation("CurveClip");
        anim.metadata.is_channel_animation = true;
        anim.metadata.looping = true;
        anim.metadata.priority = AnimationPriority::Movement;

        anim.add_track(
            "Root",
            "UpperTorso",
            TrackType::CFrame,
            TrackType::EulerAngles,
            RotationOrder::YXZ,
        );
        anim.add_keyframe(
            "Root",
            &["UpperTorso".into(), "Position".into(), "Y".into()],
            0,
            Keyframe {
                value: KeyframeValue::Number(0.0),
                easing_style: None,
                easing_direction: None,
                interpolation_mode: Some(KeyInterpolationMode::Cubic),
                left_slope: None,
                right_slope: Some(0.002),
            },
        );
        anim.add_keyframe(
            "Root",
            &["UpperTorso".into(), "Position".into(), "Y".into()],
            2400,
            Keyframe {
                value: KeyframeValue::Number(3.5),
                easing_style: None,
                easing_direction: None,
                interpolation_mode: Some(KeyInterpolationMode::Linear),
                left_slope: Some(-0.001),
                right_slope: None,
            },
        );

        let facs = anim.add_track(
            "Root",
            "LeftEyeClosed",
            TrackType::Facs,
            TrackType::EulerAngles,
            RotationOrder::XYZ,
        );
        facs.add_keyframe(0, Keyframe::new_curve(KeyframeValue::Facs(0.0)));
        facs.add_keyframe(1200, Keyframe::new_curve(KeyframeValue::Facs(1.0)));

        anim.add_event(1200, "Blink", "Closed");
        anim.set_keyframe_name(2400, Some("EndPose".to_string()));
        anim.set_end_tick();

        let ws = doc.workspace().unwrap();
        let curve_ref = to_curve_animation(&mut doc, ws, &anim, Some(&rig));
        let back = from_curve_animation(&doc, curve_ref, 30).unwrap();

        assert!(back.is_channel_animation());
        assert_eq!(back.metadata.name, "CurveClip");
        assert_eq!(back.metadata.priority, AnimationPriority::Movement);
        assert_eq!(back.metadata.end_tick, 2400);

        let rot = back
            .get_track("Root", &["UpperTorso".into(), "Rotation".into()])
            .unwrap();
        assert_eq!(rot.euler_angles_order, Some(RotationOrder::YXZ));

        let pos_y = back
            .get_track(
                "Root",
                &["UpperTorso".into(), "Position".into(), "Y".into()],
            )
            .unwrap();
        let k0 = pos_y.data.get(&0).unwrap();
        let k1 = pos_y.data.get(&2400).unwrap();
        assert_eq!(k0.interpolation_mode, Some(KeyInterpolationMode::Cubic));
        assert!((k0.right_slope.unwrap() - 0.002).abs() < 1e-5);
        assert_eq!(k1.interpolation_mode, Some(KeyInterpolationMode::Linear));
        assert!((k1.left_slope.unwrap() - (-0.001)).abs() < 1e-5);
        assert!((k1.value.as_f32().unwrap() - 3.5).abs() < 1e-4);

        let back_facs = back.get_track("Root", &["LeftEyeClosed".into()]).unwrap();
        assert!((back_facs.data.get(&1200).unwrap().value.as_f32().unwrap() - 1.0).abs() < 1e-4);
        assert_eq!(
            back.events
                .data
                .get(&1200)
                .and_then(|m| m.get("Blink"))
                .map(String::as_str),
            Some("Closed")
        );
        assert_eq!(
            back.events.named_keyframes.get(&2400).map(String::as_str),
            Some("EndPose")
        );
    }

    #[test]
    fn rig_validation_and_anim_saves_work() {
        let mut doc = Document::new_place();
        let rig_ref = make_simple_r15_rig(&mut doc);

        assert!(is_valid_rig_candidate(&doc, rig_ref));
        assert!(validate_rig(&doc, rig_ref).is_empty());

        // Test Save -> ServerStorage.RBX_ANIMSAVES
        let mut rig = build_rig_cache(&doc, rig_ref);
        let anim = AnimationData::new_rig_animation("Idle");
        let saved_ref = save_clip_to_anim_saves(&mut doc, &mut rig, &anim, "Idle").unwrap();
        assert!(doc.exists(saved_ref));
        assert_eq!(rig.anim_saves.len(), 1);
        assert!(!rig.anim_saves[0].is_legacy_anim_saves);

        // Add a legacy clip directly under rig.AnimSaves and migrate it
        let anim_saves_ov = doc
            .dom
            .get_by_ref(rig_ref)
            .unwrap()
            .children()
            .iter()
            .copied()
            .find(|&c| doc.dom.get_by_ref(c).is_some_and(|i| i.name == "AnimSaves"))
            .unwrap();
        let legacy_clip = to_keyframe_sequence(
            &mut doc,
            anim_saves_ov,
            &AnimationData::new_rig_animation("OldWalk"),
            Some(&rig),
        );
        let (_, has_legacy) = list_anim_saves(&doc, rig_ref);
        assert!(has_legacy);

        migrate_legacy_anim_saves(
            &mut doc,
            &mut rig,
            &[(legacy_clip, MigrationChoice::Migrate)],
        );
        let (_, has_legacy_after) = list_anim_saves(&doc, rig_ref);
        assert!(!has_legacy_after);
        assert_eq!(rig.anim_saves.len(), 2);

        // Test Validator 9: Multiple FaceControls
        let head = rig.part_refs["Head"];
        doc.insert(
            head,
            InstanceBuilder::new("FaceControls").with_name("FaceControls2"),
        );
        let issues = validate_rig(&doc, rig_ref);
        assert!(issues
            .iter()
            .any(|i| i.id == "MultipleFaceControls" && i.severity == ValidationSeverity::Error));
    }
}

//! Core animation data model for `AnimationClipEditor`
//! (derived from `Util/Templates`, `Util/AnimationData`,
//! `Util/PathUtils`, and `Util/TrackUtils`).

use std::collections::{BTreeMap, BTreeSet};

use glam::{EulerRot, Mat3, Quat, Vec3};
use rustudio_core::math::Xform;
use serde::{Deserialize, Serialize};

use crate::anim_editor::constants::{
    component_path_value, COMPONENT_PATH_BASE_SCALE, DEFAULT_FRAMERATE, DEFAULT_KEYFRAME_NAME,
    MAX_ANIMATION_LENGTH, MAX_TIME, PROP_IK_DATA, PROP_OFFSET_POS, PROP_OFFSET_ROT, PROP_POSITION,
    PROP_ROTATION, PROP_SURFACE_POINT, PROP_WEIGHT, PROP_X, PROP_Y, PROP_Z, TICK_FREQUENCY,
};

// ============================================================================
// Tick & Path Types and Helpers
// ============================================================================

/// Discrete animation time unit: 2400 ticks = 1 second.
pub type Tick = i32;

/// Hierarchical track path, e.g. `["UpperTorso"]`, `["UpperTorso", "Position", "X"]`.
pub type TrackPath = Vec<String>;

#[inline]
pub fn seconds_to_tick(seconds: f32) -> Tick {
    ((seconds as f64) * (TICK_FREQUENCY as f64) + 0.5).floor() as Tick
}

#[inline]
pub fn tick_to_seconds(tick: Tick) -> f32 {
    (tick as f32) / (TICK_FREQUENCY as f32)
}

#[inline]
pub fn clamp_tick(tick: Tick) -> Tick {
    tick.clamp(0, MAX_ANIMATION_LENGTH)
}

#[inline]
pub fn nearest_tick(tick: f64) -> Tick {
    (tick + 0.5).floor() as Tick
}

/// Exact `KeyframeUtils.getNearestFrame(tck, frameRate)`.
#[inline]
pub fn nearest_frame(tick: Tick, frame_rate: u32) -> Tick {
    let fps = frame_rate.max(1) as f64;
    let freq = TICK_FREQUENCY as f64;
    let frame = ((tick as f64) * fps / freq + 0.5).floor();
    (frame * freq / fps + 0.5).floor() as Tick
}

/// Fuzzy float comparison matching `Framework.Util.Math.fuzzyEq` (epsilon 1e-4).
#[inline]
pub fn fuzzy_eq(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-4
}

/// Fuzzy CFrame comparison matching `Util/fuzzyCFrameEq` (all 12 components).
pub fn fuzzy_cframe_eq(a: &Xform, b: &Xform) -> bool {
    fuzzy_eq(a.pos.x, b.pos.x)
        && fuzzy_eq(a.pos.y, b.pos.y)
        && fuzzy_eq(a.pos.z, b.pos.z)
        && fuzzy_eq(a.rot.x_axis.x, b.rot.x_axis.x)
        && fuzzy_eq(a.rot.x_axis.y, b.rot.x_axis.y)
        && fuzzy_eq(a.rot.x_axis.z, b.rot.x_axis.z)
        && fuzzy_eq(a.rot.y_axis.x, b.rot.y_axis.x)
        && fuzzy_eq(a.rot.y_axis.y, b.rot.y_axis.y)
        && fuzzy_eq(a.rot.y_axis.z, b.rot.y_axis.z)
        && fuzzy_eq(a.rot.z_axis.x, b.rot.z_axis.x)
        && fuzzy_eq(a.rot.z_axis.y, b.rot.z_axis.y)
        && fuzzy_eq(a.rot.z_axis.z, b.rot.z_axis.z)
}

/// `PathUtils.getRelativePath(path)`: returns `path[1..]`.
pub fn path_relative(path: &[String]) -> TrackPath {
    if path.len() <= 1 {
        Vec::new()
    } else {
        path[1..].to_vec()
    }
}

/// `PathUtils.getParentPath(path)`: returns `path[..len-1]`.
pub fn path_parent(path: &[String]) -> TrackPath {
    if path.is_empty() {
        Vec::new()
    } else {
        path[..path.len() - 1].to_vec()
    }
}

/// `PathUtils.equals(a, b)`.
pub fn path_equals(a: &[String], b: &[String]) -> bool {
    a == b
}

/// `PathUtils.toString(path)`.
pub fn path_to_string(path: &[String]) -> String {
    path.join(".")
}

/// `PathUtils.getPathValue(path)` for fractional component ordering.
pub fn path_value(path: &[String]) -> f64 {
    let mut scale = COMPONENT_PATH_BASE_SCALE;
    let mut val = 0.0;
    for seg in path {
        if let Some(w) = component_path_value(seg) {
            val += scale * (w as f64);
        }
        scale *= 0.1;
    }
    val
}

fn generate_guid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:04X}-{:012X}}}",
        (nanos >> 32) as u32,
        ((nanos >> 16) & 0xFFFF) as u16,
        (0x4000 | (nanos & 0x0FFF)) as u16,
        (0x8000 | ((seq >> 48) & 0x3FFF)) as u16,
        seq & 0xFFFF_FFFF_FFFF
    )
}

// ============================================================================
// Core Enumerations
// ============================================================================

/// Exact `Constants.TRACK_TYPES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrackType {
    CFrame,
    Position,
    Number,
    Angle,
    Quaternion,
    EulerAngles,
    Facs,
    IkData,
    IkTarget,
    Folder,
    Other,
}

impl TrackType {
    pub fn as_str(self) -> &'static str {
        match self {
            TrackType::CFrame => "CFrame",
            TrackType::Position => "Position",
            TrackType::Number => "Number",
            TrackType::Angle => "Angle",
            TrackType::Quaternion => "Quaternion",
            TrackType::EulerAngles => "EulerAngles",
            TrackType::Facs => "Facs",
            TrackType::IkData => "IkData",
            TrackType::IkTarget => "IkTarget",
            TrackType::Folder => "Folder",
            TrackType::Other => "Other",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "CFrame" => Some(TrackType::CFrame),
            "Position" => Some(TrackType::Position),
            "Number" => Some(TrackType::Number),
            "Angle" => Some(TrackType::Angle),
            "Quaternion" => Some(TrackType::Quaternion),
            "EulerAngles" => Some(TrackType::EulerAngles),
            "Facs" => Some(TrackType::Facs),
            "IkData" => Some(TrackType::IkData),
            "IkTarget" => Some(TrackType::IkTarget),
            "Folder" => Some(TrackType::Folder),
            "Other" => Some(TrackType::Other),
            _ => None,
        }
    }

    /// Standard ordered child component keys (`Constants.COMPONENT_TRACK_TYPES[t]._Order`).
    pub fn component_order(self) -> &'static [&'static str] {
        match self {
            TrackType::CFrame => &[PROP_POSITION, PROP_ROTATION],
            TrackType::Position | TrackType::EulerAngles => &[PROP_X, PROP_Y, PROP_Z],
            TrackType::IkTarget => &[
                PROP_OFFSET_POS,
                PROP_OFFSET_ROT,
                PROP_WEIGHT,
                PROP_SURFACE_POINT,
            ],
            _ => &[],
        }
    }

    /// Optional child component keys (`Constants.COMPONENT_TRACK_TYPES[t]._OptionalOrder`).
    pub fn optional_component_order(self) -> &'static [&'static str] {
        match self {
            TrackType::CFrame => &[PROP_IK_DATA],
            _ => &[],
        }
    }
}

/// Exact Roblox `Enum.RotationOrder` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[repr(u32)]
pub enum RotationOrder {
    #[default]
    XYZ = 0,
    XZY = 1,
    YZX = 2,
    YXZ = 3,
    ZXY = 4,
    ZYX = 5,
}

impl RotationOrder {
    pub fn to_u32(self) -> u32 {
        self as u32
    }

    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(RotationOrder::XYZ),
            1 => Some(RotationOrder::XZY),
            2 => Some(RotationOrder::YZX),
            3 => Some(RotationOrder::YXZ),
            4 => Some(RotationOrder::ZXY),
            5 => Some(RotationOrder::ZYX),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RotationOrder::XYZ => "XYZ",
            RotationOrder::XZY => "XZY",
            RotationOrder::YZX => "YZX",
            RotationOrder::YXZ => "YXZ",
            RotationOrder::ZXY => "ZXY",
            RotationOrder::ZYX => "ZYX",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "XYZ" => Some(RotationOrder::XYZ),
            "XZY" => Some(RotationOrder::XZY),
            "YZX" => Some(RotationOrder::YZX),
            "YXZ" => Some(RotationOrder::YXZ),
            "ZXY" => Some(RotationOrder::ZXY),
            "ZYX" => Some(RotationOrder::ZYX),
            _ => None,
        }
    }

    pub fn to_glam(self) -> EulerRot {
        match self {
            RotationOrder::XYZ => EulerRot::XYZ,
            RotationOrder::XZY => EulerRot::XZY,
            RotationOrder::YZX => EulerRot::YZX,
            RotationOrder::YXZ => EulerRot::YXZ,
            RotationOrder::ZXY => EulerRot::ZXY,
            RotationOrder::ZYX => EulerRot::ZYX,
        }
    }

    /// Constructs a rotation matrix from `(x, y, z)` radians using Roblox's `CFrame.fromEulerAngles(x, y, z, order)`.
    pub fn from_euler_angles(self, angles: Vec3) -> Mat3 {
        match self {
            RotationOrder::XYZ => Mat3::from_euler(EulerRot::XYZ, angles.x, angles.y, angles.z),
            RotationOrder::XZY => Mat3::from_euler(EulerRot::XZY, angles.x, angles.z, angles.y),
            RotationOrder::YZX => Mat3::from_euler(EulerRot::YZX, angles.y, angles.z, angles.x),
            RotationOrder::YXZ => Mat3::from_euler(EulerRot::YXZ, angles.y, angles.x, angles.z),
            RotationOrder::ZXY => Mat3::from_euler(EulerRot::ZXY, angles.z, angles.x, angles.y),
            RotationOrder::ZYX => Mat3::from_euler(EulerRot::ZYX, angles.z, angles.y, angles.x),
        }
    }

    /// Extracts `(x, y, z)` radians from a rotation matrix matching Roblox's `CFrame:ToEulerAngles(order)`.
    pub fn to_euler_angles(self, rot: Mat3) -> Vec3 {
        match self {
            RotationOrder::XYZ => {
                let (x, y, z) = rot.to_euler(EulerRot::XYZ);
                Vec3::new(x, y, z)
            }
            RotationOrder::XZY => {
                let (x, z, y) = rot.to_euler(EulerRot::XZY);
                Vec3::new(x, y, z)
            }
            RotationOrder::YZX => {
                let (y, z, x) = rot.to_euler(EulerRot::YZX);
                Vec3::new(x, y, z)
            }
            RotationOrder::YXZ => {
                let (y, x, z) = rot.to_euler(EulerRot::YXZ);
                Vec3::new(x, y, z)
            }
            RotationOrder::ZXY => {
                let (z, x, y) = rot.to_euler(EulerRot::ZXY);
                Vec3::new(x, y, z)
            }
            RotationOrder::ZYX => {
                let (z, y, x) = rot.to_euler(EulerRot::ZYX);
                Vec3::new(x, y, z)
            }
        }
    }
}

/// Exact Roblox `Enum.PoseEasingStyle` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[repr(u32)]
pub enum EasingStyle {
    #[default]
    Linear = 0,
    Constant = 1,
    Elastic = 2,
    Cubic = 3,
    Bounce = 4,
    CubicV2 = 5,
}

impl EasingStyle {
    pub fn to_u32(self) -> u32 {
        self as u32
    }

    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(EasingStyle::Linear),
            1 => Some(EasingStyle::Constant),
            2 => Some(EasingStyle::Elastic),
            3 => Some(EasingStyle::Cubic),
            4 => Some(EasingStyle::Bounce),
            5 => Some(EasingStyle::CubicV2),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EasingStyle::Linear => "Linear",
            EasingStyle::Constant => "Constant",
            EasingStyle::Elastic => "Elastic",
            EasingStyle::Cubic => "Cubic",
            EasingStyle::Bounce => "Bounce",
            EasingStyle::CubicV2 => "CubicV2",
        }
    }

    /// Exact `Constants.EASING_STYLE_DISPLAY_TEXT`.
    pub fn display_text(self) -> &'static str {
        match self {
            EasingStyle::Linear => "Linear",
            EasingStyle::Constant => "Constant",
            EasingStyle::Elastic => "Elastic",
            EasingStyle::Cubic => "Cubic (deprecated)",
            EasingStyle::Bounce => "Bounce",
            EasingStyle::CubicV2 => "CubicV2",
        }
    }

    /// Exact `Constants.POSE_EASING_STYLE_TO_KEY_INTERPOLATION`.
    pub fn to_key_interpolation(self) -> KeyInterpolationMode {
        match self {
            EasingStyle::Linear => KeyInterpolationMode::Linear,
            EasingStyle::Constant => KeyInterpolationMode::Constant,
            EasingStyle::Cubic
            | EasingStyle::CubicV2
            | EasingStyle::Bounce
            | EasingStyle::Elastic => KeyInterpolationMode::Cubic,
        }
    }
}

/// Exact Roblox `Enum.PoseEasingDirection` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[repr(u32)]
pub enum EasingDirection {
    #[default]
    In = 0,
    Out = 1,
    InOut = 2,
}

impl EasingDirection {
    pub fn to_u32(self) -> u32 {
        self as u32
    }

    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(EasingDirection::In),
            1 => Some(EasingDirection::Out),
            2 => Some(EasingDirection::InOut),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EasingDirection::In => "In",
            EasingDirection::Out => "Out",
            EasingDirection::InOut => "InOut",
        }
    }
}

/// Exact Roblox `Enum.KeyInterpolationMode` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[repr(u32)]
pub enum KeyInterpolationMode {
    #[default]
    Linear = 0,
    Constant = 1,
    Cubic = 2,
}

impl KeyInterpolationMode {
    pub fn to_u32(self) -> u32 {
        self as u32
    }

    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(KeyInterpolationMode::Linear),
            1 => Some(KeyInterpolationMode::Constant),
            2 => Some(KeyInterpolationMode::Cubic),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            KeyInterpolationMode::Linear => "Linear",
            KeyInterpolationMode::Constant => "Constant",
            KeyInterpolationMode::Cubic => "Cubic",
        }
    }
}

/// Exact Roblox `Enum.AnimationPriority` u32 values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[repr(u32)]
pub enum AnimationPriority {
    Idle = 0,
    Movement = 1,
    Action = 2,
    Action2 = 3,
    Action3 = 4,
    Action4 = 5,
    #[default]
    Core = 1000,
}

impl AnimationPriority {
    /// Exact menu display order from `AnimationClipMenu`.
    pub const MENU_ORDER: &'static [AnimationPriority] = &[
        AnimationPriority::Core,
        AnimationPriority::Idle,
        AnimationPriority::Movement,
        AnimationPriority::Action,
        AnimationPriority::Action2,
        AnimationPriority::Action3,
        AnimationPriority::Action4,
    ];

    pub fn to_u32(self) -> u32 {
        self as u32
    }

    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(AnimationPriority::Idle),
            1 => Some(AnimationPriority::Movement),
            2 => Some(AnimationPriority::Action),
            3 => Some(AnimationPriority::Action2),
            4 => Some(AnimationPriority::Action3),
            5 => Some(AnimationPriority::Action4),
            1000 => Some(AnimationPriority::Core),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AnimationPriority::Core => "Core",
            AnimationPriority::Idle => "Idle",
            AnimationPriority::Movement => "Movement",
            AnimationPriority::Action => "Action",
            AnimationPriority::Action2 => "Action2",
            AnimationPriority::Action3 => "Action3",
            AnimationPriority::Action4 => "Action4",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum InstanceType {
    #[default]
    Rig,
    Other,
}

// ============================================================================
// Keyframe Value & Keyframe
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyframeValue {
    CFrame(Xform),
    Position(Vec3),
    Number(f32),
    Angle(f32),
    Quaternion(Quat),
    Facs(f32),
}

impl KeyframeValue {
    /// Exact `KeyframeUtils.getDefaultValue(trackType)`.
    pub fn default_for(track_type: TrackType) -> Self {
        match track_type {
            TrackType::CFrame => KeyframeValue::CFrame(Xform {
                pos: Vec3::ZERO,
                rot: Mat3::IDENTITY,
            }),
            TrackType::Position | TrackType::EulerAngles => KeyframeValue::Position(Vec3::ZERO),
            TrackType::Quaternion => KeyframeValue::Quaternion(Quat::IDENTITY),
            TrackType::Angle => KeyframeValue::Angle(0.0),
            TrackType::Facs => KeyframeValue::Facs(0.0),
            TrackType::Number
            | TrackType::IkData
            | TrackType::IkTarget
            | TrackType::Folder
            | TrackType::Other => KeyframeValue::Number(0.0),
        }
    }

    pub fn as_cframe(self) -> Option<Xform> {
        match self {
            KeyframeValue::CFrame(xf) => Some(xf),
            KeyframeValue::Quaternion(q) => Some(Xform {
                pos: Vec3::ZERO,
                rot: Mat3::from_quat(q.normalize()),
            }),
            _ => None,
        }
    }

    pub fn as_position(self) -> Option<Vec3> {
        match self {
            KeyframeValue::Position(v) => Some(v),
            KeyframeValue::CFrame(xf) => Some(xf.pos),
            _ => None,
        }
    }

    pub fn as_quat(self) -> Option<Quat> {
        match self {
            KeyframeValue::Quaternion(q) => Some(q),
            KeyframeValue::CFrame(xf) => Some(Quat::from_mat3(&xf.rot).normalize()),
            _ => None,
        }
    }

    pub fn as_f32(self) -> Option<f32> {
        match self {
            KeyframeValue::Number(v) | KeyframeValue::Angle(v) | KeyframeValue::Facs(v) => Some(v),
            _ => None,
        }
    }

    /// Approximate equality matching `fuzzyCFrameEq` / `Math.fuzzyEq`.
    pub fn is_approximately_equal(&self, other: &Self) -> bool {
        match (self, other) {
            (KeyframeValue::CFrame(a), KeyframeValue::CFrame(b)) => fuzzy_cframe_eq(a, b),
            (KeyframeValue::Position(a), KeyframeValue::Position(b)) => {
                fuzzy_eq(a.x, b.x) && fuzzy_eq(a.y, b.y) && fuzzy_eq(a.z, b.z)
            }
            (KeyframeValue::Quaternion(a), KeyframeValue::Quaternion(b)) => {
                let ma = Xform {
                    pos: Vec3::ZERO,
                    rot: Mat3::from_quat(a.normalize()),
                };
                let mb = Xform {
                    pos: Vec3::ZERO,
                    rot: Mat3::from_quat(b.normalize()),
                };
                fuzzy_cframe_eq(&ma, &mb)
            }
            (KeyframeValue::Number(a), KeyframeValue::Number(b))
            | (KeyframeValue::Angle(a), KeyframeValue::Angle(b))
            | (KeyframeValue::Facs(a), KeyframeValue::Facs(b)) => fuzzy_eq(*a, *b),
            _ => false,
        }
    }

    /// Linear interpolation matching `KeyframeUtils.interpolate(a, b, alpha)`.
    pub fn lerp(self, other: Self, alpha: f32) -> Self {
        match (self, other) {
            (KeyframeValue::CFrame(a), KeyframeValue::CFrame(b)) => {
                let pos = a.pos.lerp(b.pos, alpha);
                let qa = Quat::from_mat3(&a.rot).normalize();
                let qb = Quat::from_mat3(&b.rot).normalize();
                let q = qa.slerp(qb, alpha);
                KeyframeValue::CFrame(Xform {
                    pos,
                    rot: Mat3::from_quat(q),
                })
            }
            (KeyframeValue::Position(a), KeyframeValue::Position(b)) => {
                KeyframeValue::Position(a.lerp(b, alpha))
            }
            (KeyframeValue::Quaternion(a), KeyframeValue::Quaternion(b)) => {
                KeyframeValue::Quaternion(a.normalize().slerp(b.normalize(), alpha))
            }
            (KeyframeValue::Number(a), KeyframeValue::Number(b)) => {
                KeyframeValue::Number(a + (b - a) * alpha)
            }
            (KeyframeValue::Angle(a), KeyframeValue::Angle(b)) => {
                KeyframeValue::Angle(a + (b - a) * alpha)
            }
            (KeyframeValue::Facs(a), KeyframeValue::Facs(b)) => {
                KeyframeValue::Facs(a + (b - a) * alpha)
            }
            _ => self,
        }
    }
}

/// Exact `Templates.keyframe()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Keyframe {
    pub value: KeyframeValue,
    pub easing_style: Option<EasingStyle>,
    pub easing_direction: Option<EasingDirection>,
    pub interpolation_mode: Option<KeyInterpolationMode>,
    pub left_slope: Option<f32>,
    pub right_slope: Option<f32>,
}

impl Keyframe {
    pub fn new_pose(value: KeyframeValue) -> Self {
        Self {
            value,
            easing_style: Some(EasingStyle::Linear),
            easing_direction: Some(EasingDirection::In),
            interpolation_mode: None,
            left_slope: None,
            right_slope: None,
        }
    }

    pub fn new_curve(value: KeyframeValue) -> Self {
        Self {
            value,
            easing_style: None,
            easing_direction: None,
            interpolation_mode: Some(KeyInterpolationMode::Cubic),
            left_slope: None,
            right_slope: None,
        }
    }

    pub fn default_for(track_type: TrackType, is_curve: bool) -> Self {
        let value = KeyframeValue::default_for(track_type);
        if is_curve {
            Self::new_curve(value)
        } else {
            Self::new_pose(value)
        }
    }
}

/// Partial update patch for `AnimationData::set_keyframe_data`.
#[derive(Debug, Clone, Default)]
pub struct KeyframePatch {
    pub value: Option<KeyframeValue>,
    pub easing_style: Option<Option<EasingStyle>>,
    pub easing_direction: Option<Option<EasingDirection>>,
    pub interpolation_mode: Option<Option<KeyInterpolationMode>>,
    pub left_slope: Option<Option<f32>>,
    pub right_slope: Option<Option<f32>>,
}

impl KeyframePatch {
    pub fn apply(&self, kf: &mut Keyframe) {
        if let Some(v) = self.value {
            kf.value = v;
        }
        if let Some(s) = self.easing_style {
            kf.easing_style = s;
        }
        if let Some(d) = self.easing_direction {
            kf.easing_direction = d;
        }
        if let Some(m) = self.interpolation_mode {
            kf.interpolation_mode = m;
        }
        if let Some(l) = self.left_slope {
            kf.left_slope = l;
        }
        if let Some(r) = self.right_slope {
            kf.right_slope = r;
        }
    }
}

// ============================================================================
// Track
// ============================================================================

/// Exact `Templates.track(trackType)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub track_type: TrackType,
    pub is_curve_track: bool,
    pub euler_angles_order: Option<RotationOrder>,
    /// Always kept sorted ascending.
    pub keyframes: Vec<Tick>,
    pub data: BTreeMap<Tick, Keyframe>,
    /// Composite sub-tracks (e.g. CFrame -> Position/Rotation, Position -> X/Y/Z, EulerAngles -> X/Y/Z).
    pub components: BTreeMap<String, Track>,
}

impl Track {
    pub fn new(track_type: TrackType) -> Self {
        Self {
            track_type,
            is_curve_track: false,
            euler_angles_order: None,
            keyframes: Vec::new(),
            data: BTreeMap::new(),
            components: BTreeMap::new(),
        }
    }

    pub fn new_curve(track_type: TrackType) -> Self {
        Self {
            track_type,
            is_curve_track: true,
            euler_angles_order: None,
            keyframes: Vec::new(),
            data: BTreeMap::new(),
            components: BTreeMap::new(),
        }
    }

    /// Builds a channel-animation track with split components matching `TrackUtils.splitTrackComponents`.
    pub fn new_channel(
        track_type: TrackType,
        rotation_type: TrackType,
        euler_order: RotationOrder,
    ) -> Self {
        let mut track = Self::new(track_type);
        match track_type {
            TrackType::CFrame => {
                let pos = Self::new_channel(TrackType::Position, rotation_type, euler_order);
                let rot = if rotation_type == TrackType::Quaternion {
                    Self::new_curve(TrackType::Quaternion)
                } else {
                    Self::new_channel(TrackType::EulerAngles, rotation_type, euler_order)
                };
                track.components.insert(PROP_POSITION.to_string(), pos);
                track.components.insert(PROP_ROTATION.to_string(), rot);
            }
            TrackType::Position => {
                for axis in [PROP_X, PROP_Y, PROP_Z] {
                    track
                        .components
                        .insert(axis.to_string(), Self::new_curve(TrackType::Number));
                }
            }
            TrackType::EulerAngles => {
                track.euler_angles_order = Some(euler_order);
                for axis in [PROP_X, PROP_Y, PROP_Z] {
                    track
                        .components
                        .insert(axis.to_string(), Self::new_curve(TrackType::Angle));
                }
            }
            TrackType::IkTarget => {
                track.components.insert(
                    PROP_OFFSET_POS.to_string(),
                    Self::new_channel(TrackType::Position, rotation_type, euler_order),
                );
                track.components.insert(
                    PROP_OFFSET_ROT.to_string(),
                    Self::new_curve(TrackType::Quaternion),
                );
                track
                    .components
                    .insert(PROP_WEIGHT.to_string(), Self::new_curve(TrackType::Number));
                track.components.insert(
                    PROP_SURFACE_POINT.to_string(),
                    Self::new_channel(TrackType::Position, rotation_type, euler_order),
                );
            }
            TrackType::Facs | TrackType::Number | TrackType::Angle | TrackType::Quaternion => {
                track.is_curve_track = true;
            }
            _ => {}
        }
        track
    }

    pub fn is_leaf(&self) -> bool {
        self.components.is_empty()
    }

    pub fn is_empty(&self) -> bool {
        if !self.data.is_empty() {
            return false;
        }
        self.components.values().all(Track::is_empty)
    }

    pub fn count_keyframes(&self) -> usize {
        if self.components.is_empty() {
            self.keyframes.len()
        } else {
            self.components.values().map(Track::count_keyframes).sum()
        }
    }

    pub fn get_rotation_type(&self) -> Option<TrackType> {
        if self.track_type == TrackType::CFrame {
            self.components.get(PROP_ROTATION).map(|r| r.track_type)
        } else if matches!(
            self.track_type,
            TrackType::EulerAngles | TrackType::Quaternion
        ) {
            Some(self.track_type)
        } else {
            None
        }
    }

    pub fn get_euler_angles_order(&self) -> Option<RotationOrder> {
        if self.track_type == TrackType::CFrame {
            self.components
                .get(PROP_ROTATION)
                .and_then(|r| r.euler_angles_order)
        } else {
            self.euler_angles_order
        }
    }

    /// Inserts or replaces a keyframe at `tick`, keeping `self.keyframes` sorted and deduplicated.
    pub fn add_keyframe(&mut self, tick: Tick, keyframe: Keyframe) {
        if let Err(idx) = self.keyframes.binary_search(&tick) {
            self.keyframes.insert(idx, tick);
        }
        self.data.insert(tick, keyframe);
    }

    /// Inserts a default keyframe at `tick` matching `AnimationData.addDefaultKeyframe`.
    pub fn add_default_keyframe(&mut self, tick: Tick) {
        let kf = Keyframe::default_for(self.track_type, self.is_curve_track);
        self.add_keyframe(tick, kf);
    }

    /// Moves a keyframe from `old_tick` to `new_tick` matching `AnimationData.moveKeyframe`.
    pub fn move_keyframe(&mut self, old_tick: Tick, new_tick: Tick) -> bool {
        if old_tick == new_tick {
            return false;
        }
        let Some(kf) = self.data.remove(&old_tick) else {
            return false;
        };
        if let Ok(idx) = self.keyframes.binary_search(&old_tick) {
            self.keyframes.remove(idx);
        }
        self.add_keyframe(new_tick, kf);
        true
    }

    /// Deletes the keyframe at `tick` matching `AnimationData.deleteKeyframe`.
    pub fn delete_keyframe(&mut self, tick: Tick) -> bool {
        let removed = self.data.remove(&tick).is_some();
        if let Ok(idx) = self.keyframes.binary_search(&tick) {
            self.keyframes.remove(idx);
        }
        removed
    }

    /// Patches an existing keyframe at `tick` matching `AnimationData.setKeyframeData`.
    pub fn set_keyframe_data(&mut self, tick: Tick, patch: &KeyframePatch) -> bool {
        if let Some(kf) = self.data.get_mut(&tick) {
            patch.apply(kf);
            true
        } else {
            false
        }
    }

    /// Clears all keyframes on this track and all descendant component tracks.
    pub fn clear(&mut self) {
        self.keyframes.clear();
        self.data.clear();
        for comp in self.components.values_mut() {
            comp.clear();
        }
    }

    pub fn collect_ticks(&self, out: &mut BTreeSet<Tick>) {
        for &t in &self.keyframes {
            out.insert(t);
        }
        for comp in self.components.values() {
            comp.collect_ticks(out);
        }
    }

    pub fn max_tick(&self) -> Tick {
        let mut max_t = self.keyframes.last().copied().unwrap_or(0);
        for comp in self.components.values() {
            max_t = max_t.max(comp.max_tick());
        }
        max_t
    }

    /// Removes keyframes beyond `MAX_ANIMATION_LENGTH`. Returns true if any were removed.
    pub fn remove_extra_keyframes(&mut self) -> bool {
        let mut clipped = false;
        if !self.keyframes.is_empty() {
            let before = self.keyframes.len();
            self.keyframes.retain(|&t| t <= MAX_ANIMATION_LENGTH);
            if self.keyframes.len() != before {
                clipped = true;
                self.data.retain(|&t, _| t <= MAX_ANIMATION_LENGTH);
            }
        }
        for comp in self.components.values_mut() {
            if comp.remove_extra_keyframes() {
                clipped = true;
            }
        }
        clipped
    }

    /// Removes redundant interior keyframes in constant runs (`AnimationData.clearTrackSequence`).
    pub fn clear_redundant_sequence(&mut self) -> bool {
        if !self.components.is_empty() {
            let mut changed = false;
            for comp in self.components.values_mut() {
                if comp.clear_redundant_sequence() {
                    changed = true;
                }
            }
            return changed;
        }
        if self.keyframes.len() < 2 {
            return false;
        }
        let mut changed = false;
        let mut keep = Vec::with_capacity(self.keyframes.len());
        let ticks = &self.keyframes;
        keep.push(ticks[0]);
        for i in 1..ticks.len() - 1 {
            let prev = self.data.get(&ticks[i - 1]);
            let cur = self.data.get(&ticks[i]);
            let next = self.data.get(&ticks[i + 1]);
            let redundant = match (prev, cur, next) {
                (Some(p), Some(c), Some(n)) => {
                    p.value.is_approximately_equal(&c.value)
                        && c.value.is_approximately_equal(&n.value)
                }
                _ => false,
            };
            if redundant {
                self.data.remove(&ticks[i]);
                changed = true;
            } else {
                keep.push(ticks[i]);
            }
        }
        keep.push(ticks[ticks.len() - 1]);
        self.keyframes = keep;

        // If exactly 2 keyframes remain, equal to each other and equal to default, clear track.
        if self.keyframes.len() == 2 {
            let default_val = KeyframeValue::default_for(self.track_type);
            let clear_all = match (
                self.data.get(&self.keyframes[0]),
                self.data.get(&self.keyframes[1]),
            ) {
                (Some(a), Some(b)) => {
                    a.value.is_approximately_equal(&b.value)
                        && a.value.is_approximately_equal(&default_val)
                }
                _ => false,
            };
            if clear_all {
                self.keyframes.clear();
                self.data.clear();
                changed = true;
            }
        }
        changed
    }

    /// Visits every leaf track under `self` with its full path.
    pub fn traverse_leaves<'a, F>(&'a self, base_path: &mut TrackPath, f: &mut F)
    where
        F: FnMut(&[String], &'a Track),
    {
        if self.components.is_empty() {
            f(base_path, self);
        } else {
            for (name, comp) in &self.components {
                base_path.push(name.clone());
                comp.traverse_leaves(base_path, f);
                base_path.pop();
            }
        }
    }

    /// Mutable visit of every leaf track under `self`.
    pub fn traverse_leaves_mut<F>(&mut self, base_path: &mut TrackPath, f: &mut F)
    where
        F: FnMut(&[String], &mut Track),
    {
        if self.components.is_empty() {
            f(base_path, self);
        } else {
            for (name, comp) in &mut self.components {
                base_path.push(name.clone());
                comp.traverse_leaves_mut(base_path, f);
                base_path.pop();
            }
        }
    }
}

// ============================================================================
// InstanceData, Events, Metadata & AnimationData
// ============================================================================

#[derive(Debug, Clone, PartialEq, Default)]
pub struct InstanceData {
    pub instance_type: InstanceType,
    pub tracks: BTreeMap<String, Track>,
}

/// Exact `Templates.animationData().Events`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Events {
    /// Sorted ascending list of ticks that have >= 1 event.
    pub keyframes: Vec<Tick>,
    /// `tick -> { event_name -> parameter_string }`.
    pub data: BTreeMap<Tick, BTreeMap<String, String>>,
    /// `tick -> custom keyframe name` (omitted when `"Keyframe"`).
    pub named_keyframes: BTreeMap<Tick, String>,
}

impl Events {
    /// Exact `AnimationData.addEvent(events, tick, name, value)`.
    pub fn add_event(&mut self, tick: Tick, name: impl Into<String>, value: impl Into<String>) {
        if !self.data.contains_key(&tick) {
            if let Err(idx) = self.keyframes.binary_search(&tick) {
                self.keyframes.insert(idx, tick);
            }
            self.data.insert(tick, BTreeMap::new());
        }
        let map = self.data.get_mut(&tick).unwrap();
        let name = name.into();
        map.entry(name).or_insert_with(|| value.into());
    }

    /// Exact `AnimationData.moveEvents(events, oldTick, newTick)`.
    pub fn move_events(&mut self, old_tick: Tick, new_tick: Tick) {
        if old_tick == new_tick {
            return;
        }
        let Some(moved) = self.data.remove(&old_tick) else {
            return;
        };
        if let Ok(idx) = self.keyframes.binary_search(&old_tick) {
            self.keyframes.remove(idx);
        }
        if let Err(idx) = self.keyframes.binary_search(&new_tick) {
            self.keyframes.insert(idx, new_tick);
        }
        self.data.insert(new_tick, moved);
    }

    /// Exact `AnimationData.deleteEvents(events, tick)`.
    pub fn delete_events(&mut self, tick: Tick) {
        if self.data.remove(&tick).is_some() {
            if let Ok(idx) = self.keyframes.binary_search(&tick) {
                self.keyframes.remove(idx);
            }
        }
    }

    /// Exact `AnimationData.setEventValue(events, tick, name, value)`.
    pub fn set_event_value(&mut self, tick: Tick, name: &str, value: impl Into<String>) {
        if let Some(map) = self.data.get_mut(&tick) {
            if let Some(slot) = map.get_mut(name) {
                *slot = value.into();
            }
        }
    }

    /// Exact `AnimationData.removeEvent(events, tick, name)`.
    pub fn remove_event(&mut self, tick: Tick, name: &str) {
        let remove_tick = if let Some(map) = self.data.get_mut(&tick) {
            map.remove(name);
            map.is_empty()
        } else {
            false
        };
        if remove_tick {
            self.data.remove(&tick);
            if let Ok(idx) = self.keyframes.binary_search(&tick) {
                self.keyframes.remove(idx);
            }
        }
    }

    /// Replaces all events at `tick` (deletes `tick` if `events` is empty).
    pub fn set_events_at_tick(&mut self, tick: Tick, events: BTreeMap<String, String>) {
        if events.is_empty() {
            self.delete_events(tick);
        } else {
            if let Err(idx) = self.keyframes.binary_search(&tick) {
                self.keyframes.insert(idx, tick);
            }
            self.data.insert(tick, events);
        }
    }

    /// Exact `AnimationData.setKeyframeName(data, tick, name)`.
    pub fn set_keyframe_name(&mut self, tick: Tick, name: Option<String>) {
        match name {
            Some(n) if !n.is_empty() && n != DEFAULT_KEYFRAME_NAME => {
                self.named_keyframes.insert(tick, n);
            }
            _ => {
                self.named_keyframes.remove(&tick);
            }
        }
    }

    pub fn remove_extra_events(&mut self) -> bool {
        let before = self.keyframes.len();
        self.keyframes.retain(|&t| t <= MAX_ANIMATION_LENGTH);
        if self.keyframes.len() != before {
            self.data.retain(|&t, _| t <= MAX_ANIMATION_LENGTH);
            true
        } else {
            false
        }
    }

    /// Sorted list of all distinct event names present across all ticks.
    pub fn all_event_names(&self) -> Vec<String> {
        let mut names = BTreeSet::new();
        for map in self.data.values() {
            for k in map.keys() {
                names.insert(k.clone());
            }
        }
        names.into_iter().collect()
    }

    /// Exact `AnimationData.getEventBounds(data, selectedEvents)`.
    pub fn event_bounds(selected_events: &BTreeSet<Tick>) -> Option<(Tick, Tick)> {
        let first = *selected_events.iter().next()?;
        let last = *selected_events.iter().next_back()?;
        Some((first.min(MAX_ANIMATION_LENGTH), last.max(0)))
    }
}

/// Exact `Templates.animationData().Metadata`.
#[derive(Debug, Clone, PartialEq)]
pub struct Metadata {
    pub name: String,
    pub start_tick: Tick,
    pub end_tick: Tick,
    pub frame_rate: u32,
    pub looping: bool,
    pub priority: AnimationPriority,
    pub is_channel_animation: bool,
    pub guid: String,
    pub ik_targets: BTreeMap<String, bool>,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            name: String::new(),
            start_tick: 0,
            end_tick: 0,
            frame_rate: DEFAULT_FRAMERATE,
            looping: false,
            priority: AnimationPriority::Core,
            is_channel_animation: false,
            guid: generate_guid(),
            ik_targets: BTreeMap::new(),
        }
    }
}

/// Complete animation clip document (`Templates.animationData()` & `Util/AnimationData`).
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationData {
    pub metadata: Metadata,
    pub events: Events,
    pub instances: BTreeMap<String, InstanceData>,
}

impl AnimationData {
    /// Exact `AnimationData.new(name, guid?)`.
    pub fn new(name: impl Into<String>) -> Self {
        Self::new_with_guid(name, None)
    }

    pub fn new_with_guid(name: impl Into<String>, guid: Option<String>) -> Self {
        let mut metadata = Metadata {
            name: name.into(),
            ..Metadata::default()
        };
        if let Some(g) = guid {
            if !g.is_empty() {
                metadata.guid = g;
            }
        }
        let mut instances = BTreeMap::new();
        instances.insert("Root".to_string(), InstanceData::default());
        Self {
            metadata,
            events: Events::default(),
            instances,
        }
    }

    /// Convenience constructor for a new rig animation with a `Root` rig instance.
    pub fn new_rig_animation(name: impl Into<String>) -> Self {
        let mut anim = Self::new(name);
        if let Some(root) = anim.instances.get_mut("Root") {
            root.instance_type = InstanceType::Rig;
        }
        anim
    }

    /// Exact `AnimationData.isChannelAnimation(data)`.
    pub fn is_channel_animation(&self) -> bool {
        self.metadata.is_channel_animation
    }

    /// Returns `true` if every keyframe tick across all tracks is aligned to `metadata.frame_rate`.
    pub fn is_quantized(&self) -> bool {
        let fps = self.metadata.frame_rate.max(1);
        for inst in self.instances.values() {
            for track in inst.tracks.values() {
                let mut ticks = BTreeSet::new();
                track.collect_ticks(&mut ticks);
                for t in ticks {
                    if nearest_frame(t, fps) != t {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Exact `AnimationData.getTrack(data, instanceName, path)`.
    pub fn get_track(&self, instance_name: &str, path: &[String]) -> Option<&Track> {
        if path.is_empty() {
            return None;
        }
        let inst = self.instances.get(instance_name)?;
        let mut track = inst.tracks.get(&path[0])?;
        for seg in &path[1..] {
            track = track.components.get(seg)?;
        }
        Some(track)
    }

    pub fn get_track_mut(&mut self, instance_name: &str, path: &[String]) -> Option<&mut Track> {
        if path.is_empty() {
            return None;
        }
        let inst = self.instances.get_mut(instance_name)?;
        let mut track = inst.tracks.get_mut(&path[0])?;
        for seg in &path[1..] {
            track = track.components.get_mut(seg)?;
        }
        Some(track)
    }

    /// Exact `AnimationData.addTrack(tracks, trackName, trackType, isChannelAnimation, rotationType, eulerAnglesOrder)`.
    pub fn add_track(
        &mut self,
        instance_name: &str,
        track_name: impl Into<String>,
        track_type: TrackType,
        rotation_type: TrackType,
        euler_order: RotationOrder,
    ) -> &mut Track {
        let is_channel = self.metadata.is_channel_animation;
        let track = if is_channel {
            Track::new_channel(track_type, rotation_type, euler_order)
        } else {
            let mut t = Track::new(track_type);
            if track_type == TrackType::EulerAngles {
                t.euler_angles_order = Some(euler_order);
            }
            t
        };
        let inst = self.instances.entry(instance_name.to_string()).or_default();
        let name = track_name.into();
        inst.tracks.insert(name.clone(), track);
        inst.tracks.get_mut(&name).unwrap()
    }

    pub fn remove_track(&mut self, instance_name: &str, track_name: &str) -> Option<Track> {
        self.instances
            .get_mut(instance_name)
            .and_then(|inst| inst.tracks.remove(track_name))
    }

    pub fn clear_track(&mut self, instance_name: &str, path: &[String]) {
        if let Some(track) = self.get_track_mut(instance_name, path) {
            track.clear();
        }
    }

    /// Exact `AnimationData.addKeyframe`.
    pub fn add_keyframe(
        &mut self,
        instance_name: &str,
        path: &[String],
        tick: Tick,
        keyframe: Keyframe,
    ) -> bool {
        if let Some(track) = self.get_track_mut(instance_name, path) {
            track.add_keyframe(tick, keyframe);
            true
        } else {
            false
        }
    }

    /// Exact `AnimationData.addDefaultKeyframe`.
    pub fn add_default_keyframe(
        &mut self,
        instance_name: &str,
        path: &[String],
        tick: Tick,
    ) -> bool {
        if let Some(track) = self.get_track_mut(instance_name, path) {
            track.add_default_keyframe(tick);
            true
        } else {
            false
        }
    }

    /// Exact `AnimationData.moveKeyframe`.
    pub fn move_keyframe(
        &mut self,
        instance_name: &str,
        path: &[String],
        old_tick: Tick,
        new_tick: Tick,
    ) -> bool {
        if let Some(track) = self.get_track_mut(instance_name, path) {
            track.move_keyframe(old_tick, new_tick)
        } else {
            false
        }
    }

    /// Exact `AnimationData.moveNamedKeyframe(data, oldTick, newTick)`:
    /// moves a named keyframe only if no other keyframe remains at `old_tick`.
    pub fn move_named_keyframe(&mut self, old_tick: Tick, new_tick: Tick) {
        if old_tick == new_tick {
            return;
        }
        let Some(name) = self.events.named_keyframes.get(&old_tick).cloned() else {
            return;
        };
        let still_present = self.get_summary_ticks().binary_search(&old_tick).is_ok();
        if !still_present {
            self.events.named_keyframes.remove(&old_tick);
            self.events.set_keyframe_name(new_tick, Some(name));
        }
    }

    /// Exact `AnimationData.deleteKeyframe`.
    pub fn delete_keyframe(&mut self, instance_name: &str, path: &[String], tick: Tick) -> bool {
        if let Some(track) = self.get_track_mut(instance_name, path) {
            track.delete_keyframe(tick)
        } else {
            false
        }
    }

    /// Exact `AnimationData.setKeyframeData`.
    pub fn set_keyframe_data(
        &mut self,
        instance_name: &str,
        path: &[String],
        tick: Tick,
        patch: &KeyframePatch,
    ) -> bool {
        if let Some(track) = self.get_track_mut(instance_name, path) {
            track.set_keyframe_data(tick, patch)
        } else {
            false
        }
    }

    pub fn add_event(&mut self, tick: Tick, name: impl Into<String>, value: impl Into<String>) {
        self.events.add_event(tick, name, value);
    }

    pub fn move_event(&mut self, old_tick: Tick, new_tick: Tick) {
        self.events.move_events(old_tick, new_tick);
    }

    pub fn delete_event(&mut self, tick: Tick) {
        self.events.delete_events(tick);
    }

    pub fn remove_event(&mut self, tick: Tick, name: &str) {
        self.events.remove_event(tick, name);
    }

    pub fn set_event_value(&mut self, tick: Tick, name: &str, value: impl Into<String>) {
        self.events.set_event_value(tick, name, value);
    }

    pub fn set_events_at_tick(&mut self, tick: Tick, events: BTreeMap<String, String>) {
        self.events.set_events_at_tick(tick, events);
    }

    pub fn rename_keyframe(&mut self, tick: Tick, name: impl Into<String>) {
        self.events.set_keyframe_name(tick, Some(name.into()));
    }

    pub fn set_keyframe_name(&mut self, tick: Tick, name: Option<String>) {
        self.events.set_keyframe_name(tick, name);
    }

    /// Exact `AnimationData.validateKeyframeNames(data)`.
    pub fn validate_keyframe_names(&mut self) {
        let valid: BTreeSet<Tick> = self.get_summary_ticks().into_iter().collect();
        self.events
            .named_keyframes
            .retain(|tick, _| valid.contains(tick));
    }

    /// Exact `AnimationData.getMaximumLength(fps)`.
    pub fn get_maximum_length(fps: u32) -> i32 {
        (fps as i32) * MAX_TIME
    }

    /// Exact `AnimationData.setEndTick(data)` (also covers events).
    pub fn set_end_tick(&mut self) {
        let mut max_end = 0;
        for inst in self.instances.values() {
            for track in inst.tracks.values() {
                max_end = max_end.max(track.max_tick());
            }
        }
        if let Some(&last_ev) = self.events.keyframes.last() {
            max_end = max_end.max(last_ev);
        }
        self.metadata.end_tick = max_end;
    }

    /// Exact `AnimationData.setLooping(data, looping)`.
    pub fn set_looping(&mut self, looping: bool) {
        self.metadata.looping = looping;
    }

    /// Exact `AnimationData.removeExtraKeyframes(data)`.
    pub fn remove_extra_keyframes(&mut self) -> bool {
        let mut clipped = false;
        for inst in self.instances.values_mut() {
            for track in inst.tracks.values_mut() {
                if track.remove_extra_keyframes() {
                    clipped = true;
                }
            }
        }
        if self.events.remove_extra_events() {
            clipped = true;
        }
        clipped
    }

    pub fn has_keyframes(&self) -> bool {
        self.instances
            .values()
            .any(|inst| inst.tracks.values().any(|t| !t.is_empty()))
    }

    pub fn has_any_tracks(&self) -> bool {
        self.instances.values().any(|inst| !inst.tracks.is_empty())
    }

    /// Exact `AnimationData.hasFacsData(data)`.
    pub fn has_facs_data(&self) -> bool {
        self.instances.values().any(|inst| {
            inst.tracks
                .values()
                .any(|t| t.track_type == TrackType::Facs)
        })
    }

    /// Exact `AnimationData.hasIkTarget(data, trackName)`.
    pub fn has_ik_target(&self, track_name: &str) -> bool {
        self.metadata
            .ik_targets
            .get(track_name)
            .copied()
            .unwrap_or(false)
    }

    /// Sorted union of all keyframe ticks across all instances and tracks.
    pub fn get_summary_ticks(&self) -> Vec<Tick> {
        let mut ticks = BTreeSet::new();
        for inst in self.instances.values() {
            for track in inst.tracks.values() {
                track.collect_ticks(&mut ticks);
            }
        }
        ticks.into_iter().collect()
    }

    /// Sorted union of all keyframe ticks in `[start, end]`.
    pub fn get_summary_ticks_in_range(&self, start: Tick, end: Tick) -> Vec<Tick> {
        self.get_summary_ticks()
            .into_iter()
            .filter(|&t| t >= start && t <= end)
            .collect()
    }

    /// Exact `AnimationData.clearTrackSequences(data)`.
    pub fn clear_track_sequences(&mut self) -> bool {
        let mut changed = false;
        for inst in self.instances.values_mut() {
            let mut empty_top_tracks = Vec::new();
            for (name, track) in &mut inst.tracks {
                if track.clear_redundant_sequence() {
                    changed = true;
                }
                if matches!(track.track_type, TrackType::CFrame | TrackType::Facs)
                    && track.is_empty()
                {
                    empty_top_tracks.push(name.clone());
                }
            }
            for name in empty_top_tracks {
                inst.tracks.remove(&name);
            }
        }
        changed
    }

    /// Promotes a `KeyframeSequence`-style animation to channel/curve tracks
    /// (`AnimationData.promoteToChannels`).
    ///
    /// Splits `CFrame` tracks into `Position.{X,Y,Z}` and `Rotation` (either
    /// `EulerAngles.{X,Y,Z}` with continuous Euler unwrapping or `Quaternion`),
    /// marks leaf tracks as `is_curve_track = true`, appends `" [CHANNELS]"` to
    /// `metadata.name`, sets `metadata.is_channel_animation = true`, and returns
    /// `(num_tracks, num_keyframes)`.
    pub fn promote_to_curves(
        &mut self,
        rotation_type: TrackType,
        euler_order: RotationOrder,
    ) -> (usize, usize) {
        if self.metadata.is_channel_animation {
            return (0, 0);
        }

        let mut num_tracks = 0usize;
        let mut num_keyframes = 0usize;

        for inst in self.instances.values_mut() {
            for track in inst.tracks.values_mut() {
                match track.track_type {
                    TrackType::CFrame => {
                        let mut pos_x = Track::new_curve(TrackType::Number);
                        let mut pos_y = Track::new_curve(TrackType::Number);
                        let mut pos_z = Track::new_curve(TrackType::Number);

                        let mut rot_track = if rotation_type == TrackType::Quaternion {
                            Track::new_curve(TrackType::Quaternion)
                        } else {
                            let mut ea = Track::new(TrackType::EulerAngles);
                            ea.euler_angles_order = Some(euler_order);
                            ea.components
                                .insert(PROP_X.to_string(), Track::new_curve(TrackType::Angle));
                            ea.components
                                .insert(PROP_Y.to_string(), Track::new_curve(TrackType::Angle));
                            ea.components
                                .insert(PROP_Z.to_string(), Track::new_curve(TrackType::Angle));
                            ea
                        };

                        let mut prev_euler: Option<Vec3> = None;

                        for &tick in &track.keyframes {
                            let Some(kf) = track.data.get(&tick) else {
                                continue;
                            };
                            let xf = kf.value.as_cframe().unwrap_or(Xform {
                                pos: Vec3::ZERO,
                                rot: Mat3::IDENTITY,
                            });
                            let interp = kf
                                .easing_style
                                .map(EasingStyle::to_key_interpolation)
                                .or(kf.interpolation_mode)
                                .unwrap_or(KeyInterpolationMode::Linear);

                            let make_scalar = |v: KeyframeValue| Keyframe {
                                value: v,
                                easing_style: None,
                                easing_direction: None,
                                interpolation_mode: Some(interp),
                                left_slope: None,
                                right_slope: None,
                            };

                            pos_x.add_keyframe(tick, make_scalar(KeyframeValue::Number(xf.pos.x)));
                            pos_y.add_keyframe(tick, make_scalar(KeyframeValue::Number(xf.pos.y)));
                            pos_z.add_keyframe(tick, make_scalar(KeyframeValue::Number(xf.pos.z)));

                            if rotation_type == TrackType::Quaternion {
                                let q = Quat::from_mat3(&xf.rot).normalize();
                                rot_track
                                    .add_keyframe(tick, make_scalar(KeyframeValue::Quaternion(q)));
                            } else {
                                let raw = euler_order.to_euler_angles(xf.rot);
                                let unwrapped = unwrap_euler(raw, prev_euler, euler_order);
                                prev_euler = Some(unwrapped);
                                if let Some(rx) = rot_track.components.get_mut(PROP_X) {
                                    rx.add_keyframe(
                                        tick,
                                        make_scalar(KeyframeValue::Angle(unwrapped.x)),
                                    );
                                }
                                if let Some(ry) = rot_track.components.get_mut(PROP_Y) {
                                    ry.add_keyframe(
                                        tick,
                                        make_scalar(KeyframeValue::Angle(unwrapped.y)),
                                    );
                                }
                                if let Some(rz) = rot_track.components.get_mut(PROP_Z) {
                                    rz.add_keyframe(
                                        tick,
                                        make_scalar(KeyframeValue::Angle(unwrapped.z)),
                                    );
                                }
                            }
                        }

                        let mut pos_track = Track::new(TrackType::Position);
                        pos_track.components.insert(PROP_X.to_string(), pos_x);
                        pos_track.components.insert(PROP_Y.to_string(), pos_y);
                        pos_track.components.insert(PROP_Z.to_string(), pos_z);

                        track.keyframes.clear();
                        track.data.clear();
                        track.components.clear();
                        track
                            .components
                            .insert(PROP_POSITION.to_string(), pos_track);
                        track
                            .components
                            .insert(PROP_ROTATION.to_string(), rot_track);
                    }
                    TrackType::Facs | TrackType::Number | TrackType::Angle => {
                        track.is_curve_track = true;
                        for kf in track.data.values_mut() {
                            if kf.interpolation_mode.is_none() {
                                kf.interpolation_mode = Some(
                                    kf.easing_style
                                        .map(EasingStyle::to_key_interpolation)
                                        .unwrap_or(KeyInterpolationMode::Linear),
                                );
                            }
                            kf.easing_style = None;
                            kf.easing_direction = None;
                        }
                    }
                    _ => {}
                }
            }
        }

        self.clear_track_sequences();
        self.metadata.is_channel_animation = true;
        self.metadata.name.push_str(" [CHANNELS]");

        for inst in self.instances.values() {
            num_tracks += inst.tracks.len();
            for track in inst.tracks.values() {
                num_keyframes += track.count_keyframes();
            }
        }
        (num_tracks, num_keyframes)
    }

    /// Alias for `promote_to_curves` matching the Luau name `promoteToChannels`.
    pub fn promote_to_channels(
        &mut self,
        rotation_type: TrackType,
        euler_order: RotationOrder,
    ) -> (usize, usize) {
        self.promote_to_curves(rotation_type, euler_order)
    }
}

/// Continuous Euler unwrapping matching `TrackUtils.convertTrackToEulerAngles` (`PROTO_52`–`PROTO_65`).
pub fn unwrap_euler(cur: Vec3, prev: Option<Vec3>, order: RotationOrder) -> Vec3 {
    let Some(p) = prev else {
        return cur;
    };
    let extract = |v: Vec3| -> (f32, f32, f32) {
        match order {
            RotationOrder::XYZ => (v.z, v.y, v.x),
            RotationOrder::XZY => (v.y, v.z, v.x),
            RotationOrder::YXZ => (v.z, v.x, v.y),
            RotationOrder::YZX => (v.x, v.z, v.y),
            RotationOrder::ZXY => (v.y, v.x, v.z),
            RotationOrder::ZYX => (v.x, v.y, v.z),
        }
    };
    let reconstruct = |(a, b, c): (f32, f32, f32)| -> Vec3 {
        match order {
            RotationOrder::XYZ => Vec3::new(c, b, a),
            RotationOrder::XZY => Vec3::new(c, a, b),
            RotationOrder::YXZ => Vec3::new(b, c, a),
            RotationOrder::YZX => Vec3::new(a, c, b),
            RotationOrder::ZXY => Vec3::new(b, a, c),
            RotationOrder::ZYX => Vec3::new(a, b, c),
        }
    };
    let unwrap_angle = |prev_a: f32, x: f32| -> f32 {
        let tau = std::f32::consts::TAU;
        x + tau * ((prev_a - x) / tau + 0.5).floor()
    };

    let (pa, pb, pc) = extract(p);
    let (a, b, c) = extract(cur);

    let c1 = (
        unwrap_angle(pa, a),
        unwrap_angle(pb, b),
        unwrap_angle(pc, c),
    );
    let pi = std::f32::consts::PI;
    let c2 = (
        unwrap_angle(pa, a + pi),
        unwrap_angle(pb, pi - b),
        unwrap_angle(pc, c + pi),
    );

    let d1 = (pa - c1.0).powi(2) + (pb - c1.1).powi(2) + (pc - c1.2).powi(2);
    let d2 = (pa - c2.0).powi(2) + (pb - c2.1).powi(2) + (pc - c2.2).powi(2);

    if d1 <= d2 {
        reconstruct(c1)
    } else {
        reconstruct(c2)
    }
}

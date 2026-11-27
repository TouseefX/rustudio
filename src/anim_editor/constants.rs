//! Studio-matched constants for RusTudio's native port of Roblox Studio's
//! `AnimationClipEditor` (derived from `Util/Constants`).

use crate::anim_editor::model::{
    EasingDirection, EasingStyle, KeyInterpolationMode, RotationOrder,
};

// ============================================================================
// Plugin Identity, Settings Keys & Folders
// ============================================================================

pub const PLUGIN_NAME: &str = "AnimationClipEditor";
pub const PLUGIN_ICON: &str = "";
pub const CHECK_IMAGE: &str = "rbxasset://textures/ui/LuaChat/icons/ic-check@2x.png";
pub const ANIMATION_UPLOAD_SUCCESS_EVENT_NAME: &str = "animationUploadSuccess";

/// Persistent settings keys (`Constants.SETTINGS`).
pub const SETTING_EULER_ANGLES_ORDER: &str = "ACE_EulerAnglesOrder";
pub const SETTING_ROTATION_TYPE: &str = "ACE_RotationType";
pub const SETTING_SHOW_AS_SECONDS: &str = "ACE_ShowAsSeconds";
pub const SETTING_SNAP_MODE: &str = "ACE_SnapMode";
pub const SETTING_TIMELINE_UNIT: &str = "ACE_TimelineUnit";

/// Folder & instance names (`Constants.FOLDERS` and related).
pub const ANIMSAVES_NAME: &str = "AnimSaves";
pub const RBX_ANIMSAVES_NAME: &str = "RBX_ANIMSAVES";
pub const MICROBONES_FOLDER: &str = "RBX_MICROBONE_NODES";
pub const IK_DATA_FOLDER: &str = "IkData";
pub const IK_TARGETS_FOLDER: &str = "IkTargets";
pub const FACE_CONTROLS_FOLDER: &str = "FaceControls";
pub const NAMED_KEYFRAMES_CHANNEL: &str = "NamedKeyframeEvent";
pub const TEMPORARY_CONSTRAINTS: &str = "ROBLOX_R15_IK_CONSTRAINTS";
pub const TEMPORARY_CONTROL_RIG_ANIMATION: &str = "RBX_TMP_CONTROL_RIG";

/// Default animation & keyframe names.
pub const DEFAULT_ANIMATION_NAME: &str = "Untitled";
pub const DEFAULT_ANIMATION_CLIP_NAME: &str = "Untitled Animation Clip";
pub const DEFAULT_IMPORTED_NAME: &str = "Imported Animation Clip";
pub const AUTOMATIC_SAVE_NAME: &str = "Automatic Save";
pub const DEFAULT_AUTOSAVE_NAME: &str = AUTOMATIC_SAVE_NAME;
pub const DEFAULT_KEYFRAME_NAME: &str = "Keyframe";

// ============================================================================
// Timing, Tick Frequency, Framerate & Limits
// ============================================================================

/// Internal ticks per second (divisible by 24, 30, 60, and 120).
pub const TICK_FREQUENCY: i32 = 2400;
pub const DEFAULT_TICK_FREQUENCY: i32 = 2400;

/// Maximum animation duration in seconds (30 minutes).
pub const MAX_TIME: i32 = 1800;
/// Maximum animation length in ticks (`MAX_TIME * TICK_FREQUENCY` = 4,320,000).
pub const MAX_ANIMATION_LENGTH: i32 = MAX_TIME * TICK_FREQUENCY;

pub const DEFAULT_FRAMERATE: u32 = 30;
pub const MIN_FRAMERATE: u32 = 1;
pub const MAX_FRAMERATE: u32 = 120;

pub const MIN_PLAYBACK_SPEED: f32 = 0.01;
pub const MAX_PLAYBACK_SPEED: f32 = 10.0;

pub const MAX_DISPLAYED_TIME: i32 = 30;
pub const MIN_TIMELINE_RANGE: i32 = 30;

/// Undo / redo history stack limit.
pub const MAX_WAYPOINT_SIZE: usize = 20;

/// Threshold above which the dopesheet collapses keyframes into clusters.
pub const MAX_VISIBLE_KEYFRAMES: usize = 1000;

pub const LENGTH_PADDING: f32 = 1.0;
pub const ZOOM_INCREMENT: f32 = 0.04;
pub const CANVAS_ZOOM_INCREMENT: f32 = 0.1;

/// Pixel threshold for snapping the scrubber to the nearest keyframe.
pub const SNAP_TO_KEYFRAME_THRESHOLD: f32 = 30.0;
pub const KEYFRAME_SNAP_THRESHOLD_PX: f32 = SNAP_TO_KEYFRAME_THRESHOLD;

/// Double-click detection window in seconds (`DoubleClickDetector`).
pub const DOUBLE_CLICK_THRESHOLD_SECS: f64 = 0.2;

// ============================================================================
// UI Layout, Timeline, Curve Canvas & Prompt Dimensions (px)
// ============================================================================

pub const MAIN_FLOATING_SIZE: (f32, f32) = (640.0, 200.0);
pub const MAIN_TEST_SIZE: (f32, f32) = (1024.0, 200.0);
pub const TRACK_LIST_START_WIDTH: f32 = 445.0;
pub const TRACK_LIST_MIN_WIDTH: f32 = 440.0;
pub const TIMELINE_MIN_WIDTH: f32 = 190.0;
pub const MAIN_MINIMUM_SIZE: (f32, f32) = (TRACK_LIST_MIN_WIDTH + TIMELINE_MIN_WIDTH + 10.0, 200.0);

pub const TIMELINE_HEIGHT: f32 = 24.0;
pub const SUMMARY_TRACK_HEIGHT: f32 = 22.0;
pub const TRACK_HEIGHT: f32 = 22.0;

pub const TRACK_PADDING_SMALL: f32 = 72.0;
pub const TRACK_PADDING_MEDIUM: f32 = 84.0;
pub const TRACK_PADDING_LARGE: f32 = 96.0;

pub const CONTROLS_WIDTH: f32 = 200.0;
pub const TIME_DISPLAY_WIDTH: f32 = 100.0;

pub const SCROLL_BAR_SIZE: f32 = 17.0;
pub const SCROLL_BAR_PADDING: f32 = 2.0;

pub const INDENT_PADDING: f32 = 8.0;
pub const ARROW_SIZE: f32 = 10.0;
pub const TRACKLIST_BUTTON_SIZE: f32 = 16.0;
pub const TRACKLIST_RIGHT_PADDING: f32 = 8.0;

pub const NUMBERBOX_WIDTH: f32 = 80.0;
pub const NUMBERBOX_PADDING: f32 = 4.0;
pub const NUMBERBOX_DRAG_MULTIPLIER: f32 = 0.05;
pub const NUMBERBOX_FACS_DRAG_MULTIPLIER: f32 = 0.01;
pub const NUMBERTRACK_PADDING: f32 = 5.0;
pub const NUMBER_PRECISION: f32 = 1000.0;
pub const NUMBER_FACS_PRECISION: f32 = 100.0;

pub const MIN_SPACE_BETWEEN_KEYS: f32 = 8.0;
pub const KEYFRAME_WIDTH: f32 = 11.0;
pub const KEYFRAME_BORDER_SIZE: f32 = 1.25;
pub const KEYFRAME_BORDER_SIZE_SELECTED: f32 = 2.0;
pub const SUMMARY_KEYFRAME_WIDTH: f32 = 13.0;

pub const EVENT_MARKER_SIZE: (f32, f32) = (6.0, 20.0);
pub const EVENT_MARKER_BORDER_SIZE: (f32, f32) = (8.0, 22.0);

pub const NUM_TICKS: u32 = 5;
pub const TICK_SPACING: f32 = 125.0;
pub const SCALE_NUM_TICKS: u32 = 5;
pub const SCALE_TICK_SPACING: f32 = 300.0;
pub const TICK_LABEL_SIZE: (f32, f32) = (25.0, 15.0);

pub const TOGGLE_EDITOR_BUTTON_WIDTH: f32 = 17.0;
pub const TOGGLE_EDITOR_BUTTON_HEIGHT: f32 = 16.0;

pub const CURVE_WIDTH: f32 = 1.0;
pub const CURVE_WIDTH_SELECTED: f32 = 2.0;
pub const CURVE_RESOLUTION: usize = 25;
pub const CURVE_REFINEMENT: usize = 5;
pub const CURVE_INTERVAL: f32 = 5.0;
pub const CURVE_CANVAS_PADDING: f32 = 0.1;
pub const CURVE_CANVAS_MIN_RANGE: f32 = 0.2;

pub const TANGENT_CONTROL_LENGTH: f32 = 0.09;
pub const TANGENT_CONTROL_WIDTH: f32 = 1.0;
pub const TANGENT_CONTROL_SIZE: f32 = 9.0;
pub const SCRUBBER_MARKER_WIDTH: f32 = 3.0;
pub const CLAMPED_AUTO_TANGENT_THRESHOLD: f32 = 0.25;

pub const SCALE_CONTROLS_PADDING: f32 = 12.0;
pub const SCALE_HANDLE_HIT_WIDTH: f32 = 6.0;
pub const SCALE_HANDLE_BAR_WIDTH: f32 = 2.0;
pub const TIME_TAG_SIZE: (f32, f32) = (30.0, 18.0);

pub const PROMPT_SIZE: (f32, f32) = (320.0, 130.0);
pub const PROMPT_BUTTON_SIZE: (f32, f32) = (100.0, 25.0);
pub const PROMPT_BUTTON_PADDING: f32 = 10.0;
pub const PROMPT_VERTICAL_PADDING: f32 = 15.0;
pub const PROMPT_HORIZONTAL_PADDING: f32 = 20.0;

/// Fractional path weight base scale used for deterministic track sorting.
pub const COMPONENT_PATH_BASE_SCALE: f64 = 0.1;

/// Face Controls Editor layout metrics.
pub const FACE_CONTROLS_EDITOR_ORIGINAL_WIDTH: f32 = 240.0;
pub const FACE_CONTROLS_EDITOR_FRONT_DIAGRAM_WIDTH: f32 = 220.0;
pub const FACE_CONTROLS_EDITOR_FRONT_DIAGRAM_HEIGHT: f32 = 310.0;
pub const FACE_CONTROLS_EDITOR_SIDE_DIAGRAM_WIDTH: f32 = 220.0;
pub const FACE_CONTROLS_EDITOR_SIDE_DIAGRAM_HEIGHT: f32 = 190.0;
pub const FACE_CONTROLS_EDITOR_DIAGRAM_PADDING: f32 = 10.0;
pub const FACE_CONTROLS_EDITOR_SPACING_BETWEEN_DIAGRAMS: f32 = 40.0;
pub const FACE_CONTROLS_EDITOR_TOGGLES_CONTAINER_HEIGHT: f32 = 95.0;

// ============================================================================
// IK & Microbone Constants
// ============================================================================

pub const TRANSLATION_STIFFNESS: f32 = 0.7;
pub const ROTATION_STIFFNESS: f32 = 0.7;
pub const MIN_TRANSLATION_STIFFNESS: f32 = 0.01;
pub const MIN_ROTATION_STIFFNESS: f32 = 0.01;
pub const MIN_EFFECTOR_DISTANCE: f32 = 10.0;
pub const LENGTH_TO_RADIUS_RATIO: f32 = 15.0;
pub const LEAF_BONE_LENGTH_RATIO: f32 = 0.75;

pub const BONE_NODE_TRANSPARENCY: f32 = 0.15;
pub const BONE_LINK_TRANSPARENCY: f32 = 0.45;
pub const BONE_TRANSPARENCY_DEFAULT: f32 = 0.3;
pub const BONE_TRANSPARENCY_HOVER: f32 = 0.0;

// ============================================================================
// Property Keys & Component Path Ordering
// ============================================================================

pub const PROP_POSITION: &str = "Position";
pub const PROP_ROTATION: &str = "Rotation";
pub const PROP_X: &str = "X";
pub const PROP_Y: &str = "Y";
pub const PROP_Z: &str = "Z";
pub const PROP_IK_DATA: &str = "IkData";
pub const PROP_OFFSET_POS: &str = "OffsetPos";
pub const PROP_OFFSET_ROT: &str = "OffsetRot";
pub const PROP_WEIGHT: &str = "Weight";
pub const PROP_SURFACE_POINT: &str = "SurfacePoint";

/// Numeric ordering weight for component path segments (`Constants.COMPONENT_PATH_VALUE`).
pub fn component_path_value(key: &str) -> Option<u32> {
    match key {
        PROP_POSITION | PROP_X | PROP_OFFSET_POS => Some(1),
        PROP_ROTATION | PROP_Y | PROP_OFFSET_ROT => Some(2),
        PROP_IK_DATA | PROP_Z | PROP_WEIGHT => Some(3),
        PROP_SURFACE_POINT => Some(4),
        _ => None,
    }
}

// ============================================================================
// Preset Arrays & Enumerations
// ============================================================================

/// Frame rate presets in menu order (`Constants.FRAMERATES`).
pub const FRAMERATES: &[u32] = &[24, 30, 60, 120];

/// Playback speed presets in menu order (`Constants.PLAYBACK_SPEEDS`).
pub const PLAYBACK_SPEEDS: &[f32] = &[0.25, 0.5, 1.0, 2.0, 4.0];

/// Floor grid speed presets (`Constants.GRIDSPEEDS`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GridSpeedPreset {
    Static,
    Default,
    FromRig,
    Custom,
}

pub const GRID_SPEEDS: &[GridSpeedPreset] = &[
    GridSpeedPreset::Static,
    GridSpeedPreset::Default,
    GridSpeedPreset::FromRig,
    GridSpeedPreset::Custom,
];

pub const GRID_SPEED_STATIC: f32 = 0.0;
pub const GRID_SPEED_DEFAULT: f32 = 16.0;

/// Import scale unit presets (`MultipleOptionPrompt`, Stud = 1.0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleUnitPreset {
    pub name: &'static str,
    pub factor: f64,
}

pub const SCALE_UNITS: &[ScaleUnitPreset] = &[
    ScaleUnitPreset {
        name: "Stud",
        factor: 1.0,
    },
    ScaleUnitPreset {
        name: "Meter",
        factor: 3.571_428_571_428_571_6, // 1 / 0.28
    },
    ScaleUnitPreset {
        name: "Centimeter",
        factor: 0.035_714_285_714_285_71,
    },
    ScaleUnitPreset {
        name: "Millimeter",
        factor: 0.003_571_428_571_428_571_3,
    },
    ScaleUnitPreset {
        name: "Foot",
        factor: 1.088_571_428_571_428_5,
    },
    ScaleUnitPreset {
        name: "Inch",
        factor: 0.090_714_285_714_285_72,
    },
];

/// Exact `Constants.EASING_STYLE_ORDER`.
pub const EASING_STYLE_ORDER: &[EasingStyle] = &[
    EasingStyle::Linear,
    EasingStyle::Constant,
    EasingStyle::CubicV2,
    EasingStyle::Elastic,
    EasingStyle::Bounce,
    EasingStyle::Cubic,
];

/// Exact `Constants.EASING_DIRECTION_ORDER`.
pub const EASING_DIRECTION_ORDER: &[EasingDirection] = &[
    EasingDirection::In,
    EasingDirection::Out,
    EasingDirection::InOut,
];

/// Exact `Constants.KEY_INTERPOLATION_MODE_ORDER`.
pub const KEY_INTERPOLATION_MODE_ORDER: &[KeyInterpolationMode] = &[
    KeyInterpolationMode::Linear,
    KeyInterpolationMode::Constant,
    KeyInterpolationMode::Cubic,
];

/// Exact `Constants.EULER_ANGLES_ORDER`.
pub const EULER_ANGLES_ORDER: &[RotationOrder] = &[
    RotationOrder::XYZ,
    RotationOrder::XZY,
    RotationOrder::YXZ,
    RotationOrder::YZX,
    RotationOrder::ZXY,
    RotationOrder::ZYX,
];

// ============================================================================
// R15 Hierarchy & IK Constraint Config Tables
// ============================================================================

/// All 16 standard R15 body parts (`Constants.R15_PARTS`).
pub const R15_PARTS: &[&str] = &[
    "UpperTorso",
    "LowerTorso",
    "LeftFoot",
    "RightFoot",
    "HumanoidRootPart",
    "Head",
    "RightUpperLeg",
    "RightLowerLeg",
    "RightHand",
    "LeftUpperLeg",
    "LeftLowerLeg",
    "LeftHand",
    "RightUpperArm",
    "RightLowerArm",
    "LeftUpperArm",
    "LeftLowerArm",
];

/// R15 child -> parent part links (`Constants.R15links`).
pub const R15_LINKS: &[(&str, &str)] = &[
    ("LowerTorso", "HumanoidRootPart"),
    ("UpperTorso", "LowerTorso"),
    ("RightUpperLeg", "LowerTorso"),
    ("LeftUpperLeg", "LowerTorso"),
    ("RightUpperArm", "UpperTorso"),
    ("LeftUpperArm", "UpperTorso"),
    ("Head", "UpperTorso"),
    ("LeftLowerArm", "LeftUpperArm"),
    ("LeftHand", "LeftLowerArm"),
    ("RightLowerArm", "RightUpperArm"),
    ("RightHand", "RightLowerArm"),
    ("LeftLowerLeg", "LeftUpperLeg"),
    ("LeftFoot", "LeftLowerLeg"),
    ("RightLowerLeg", "RightUpperLeg"),
    ("RightFoot", "RightLowerLeg"),
];

/// R15 Part1 -> standard Motor6D joint name (`Constants.PART1_TO_JOINT`).
pub const PART1_TO_JOINT: &[(&str, &str)] = &[
    ("LowerTorso", "Root"),
    ("UpperTorso", "Waist"),
    ("LeftFoot", "LeftAnkle"),
    ("RightFoot", "RightAnkle"),
    ("LeftHand", "LeftWrist"),
    ("RightHand", "RightWrist"),
    ("LeftLowerArm", "LeftElbow"),
    ("RightLowerArm", "RightElbow"),
    ("LeftLowerLeg", "LeftKnee"),
    ("RightLowerLeg", "RightKnee"),
    ("LeftUpperArm", "LeftShoulder"),
    ("RightUpperArm", "RightShoulder"),
    ("LeftUpperLeg", "LeftHip"),
    ("RightUpperLeg", "RightHip"),
    ("Head", "Neck"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IkConstraintKind {
    BallSocket,
    Hinge,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IkConstraintConfig {
    pub kind: IkConstraintKind,
    pub limits_enabled: bool,
    pub restitution: f32,
    pub lower_angle: f32,
    pub upper_angle: f32,
    pub twist_limits_enabled: bool,
    pub twist_lower_angle: f32,
    pub twist_upper_angle: f32,
    pub axis: Option<[f32; 3]>,
    pub secondary_axis: Option<[f32; 3]>,
}

/// R15 IK constraint definitions (`Constants.CONSTRAINTS_CONFIG`).
pub const IK_CONSTRAINTS_CONFIG: &[(&str, IkConstraintConfig)] = &[
    (
        "Ankle",
        IkConstraintConfig {
            kind: IkConstraintKind::BallSocket,
            limits_enabled: false,
            restitution: 0.0,
            lower_angle: 0.0,
            upper_angle: 20.0,
            twist_limits_enabled: false,
            twist_lower_angle: -20.0,
            twist_upper_angle: 20.0,
            axis: None,
            secondary_axis: None,
        },
    ),
    (
        "Knee",
        IkConstraintConfig {
            kind: IkConstraintKind::Hinge,
            limits_enabled: true,
            restitution: 0.0,
            lower_angle: -135.0,
            upper_angle: -10.0,
            twist_limits_enabled: false,
            twist_lower_angle: 0.0,
            twist_upper_angle: 0.0,
            axis: None,
            secondary_axis: None,
        },
    ),
    (
        "Hip",
        IkConstraintConfig {
            kind: IkConstraintKind::BallSocket,
            limits_enabled: true,
            restitution: 0.0,
            lower_angle: 0.0,
            upper_angle: 80.0,
            twist_limits_enabled: true,
            twist_lower_angle: -3.0,
            twist_upper_angle: 3.0,
            axis: Some([0.0, -1.0, 0.0]),
            secondary_axis: Some([0.0, 0.0, 1.0]),
        },
    ),
    (
        "Waist",
        IkConstraintConfig {
            kind: IkConstraintKind::Hinge,
            limits_enabled: true,
            restitution: 0.0,
            lower_angle: -90.0,
            upper_angle: 90.0,
            twist_limits_enabled: false,
            twist_lower_angle: 0.0,
            twist_upper_angle: 0.0,
            axis: None,
            secondary_axis: None,
        },
    ),
    (
        "Shoulder",
        IkConstraintConfig {
            kind: IkConstraintKind::BallSocket,
            limits_enabled: true,
            restitution: 0.0,
            lower_angle: 0.0,
            upper_angle: 170.0,
            twist_limits_enabled: false,
            twist_lower_angle: -90.0,
            twist_upper_angle: 135.0,
            axis: None,
            secondary_axis: None,
        },
    ),
    (
        "Elbow",
        IkConstraintConfig {
            kind: IkConstraintKind::Hinge,
            limits_enabled: true,
            restitution: 0.0,
            lower_angle: 0.0,
            upper_angle: 135.0,
            twist_limits_enabled: false,
            twist_lower_angle: 0.0,
            twist_upper_angle: 0.0,
            axis: None,
            secondary_axis: None,
        },
    ),
    (
        "Wrist",
        IkConstraintConfig {
            kind: IkConstraintKind::BallSocket,
            limits_enabled: true,
            restitution: 0.0,
            lower_angle: 0.0,
            upper_angle: 20.0,
            twist_limits_enabled: true,
            twist_lower_angle: 0.0,
            twist_upper_angle: 0.0,
            axis: None,
            secondary_axis: None,
        },
    ),
    (
        "Neck",
        IkConstraintConfig {
            kind: IkConstraintKind::BallSocket,
            limits_enabled: true,
            restitution: 0.0,
            lower_angle: 0.0,
            upper_angle: 30.0,
            twist_limits_enabled: false,
            twist_lower_angle: -70.0,
            twist_upper_angle: 70.0,
            axis: None,
            secondary_axis: None,
        },
    ),
];

/// R15 Part -> IK constraint config name (`Constants.PartToConstraintConfigMap`).
pub const PART_TO_CONSTRAINT_CONFIG: &[(&str, &str)] = &[
    ("UpperTorso", "Waist"),
    ("RightUpperLeg", "Hip"),
    ("LeftUpperLeg", "Hip"),
    ("RightUpperArm", "Shoulder"),
    ("LeftUpperArm", "Shoulder"),
    ("Head", "Neck"),
    ("LeftLowerArm", "Elbow"),
    ("RightLowerArm", "Elbow"),
    ("LeftHand", "Wrist"),
    ("RightHand", "Wrist"),
    ("LeftLowerLeg", "Knee"),
    ("RightLowerLeg", "Knee"),
    ("LeftFoot", "Ankle"),
    ("RightFoot", "Ankle"),
];

pub fn r15_parent(part: &str) -> Option<&'static str> {
    R15_LINKS
        .iter()
        .find_map(|&(child, parent)| (child == part).then_some(parent))
}

pub fn part1_to_joint(part: &str) -> Option<&'static str> {
    PART1_TO_JOINT
        .iter()
        .find_map(|&(p, j)| (p == part).then_some(j))
}

pub fn part_ik_constraint(part: &str) -> Option<IkConstraintConfig> {
    let config_name = PART_TO_CONSTRAINT_CONFIG
        .iter()
        .find_map(|&(p, c)| (p == part).then_some(c))?;
    IK_CONSTRAINTS_CONFIG
        .iter()
        .find_map(|&(name, cfg)| (name == config_name).then_some(cfg))
}

// ============================================================================
// FACS Regions, Controls, Symmetry & Dual-Slider Tables
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FacsRegion {
    Mouth,
    Jaw,
    Brows,
    Eyes,
    Tongue,
    Other,
}

impl FacsRegion {
    pub const ALL: &'static [FacsRegion] = &[
        FacsRegion::Mouth,
        FacsRegion::Jaw,
        FacsRegion::Brows,
        FacsRegion::Eyes,
        FacsRegion::Tongue,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            FacsRegion::Mouth => "Mouth",
            FacsRegion::Jaw => "Jaw",
            FacsRegion::Brows => "Brows",
            FacsRegion::Eyes => "Eyes",
            FacsRegion::Tongue => "Tongue",
            FacsRegion::Other => "Other",
        }
    }
}

/// All 50 FACS controls paired with their `FacsRegion` (`Constants.FacsControlToRegionMap`).
pub const FACS_CONTROLS: &[(&str, FacsRegion)] = &[
    // Mouth (25)
    ("ChinRaiserUpperLip", FacsRegion::Mouth),
    ("ChinRaiser", FacsRegion::Mouth),
    ("FlatPucker", FacsRegion::Mouth),
    ("Funneler", FacsRegion::Mouth),
    ("LowerLipSuck", FacsRegion::Mouth),
    ("LipPresser", FacsRegion::Mouth),
    ("LipsTogether", FacsRegion::Mouth),
    ("MouthLeft", FacsRegion::Mouth),
    ("MouthRight", FacsRegion::Mouth),
    ("Pucker", FacsRegion::Mouth),
    ("UpperLipSuck", FacsRegion::Mouth),
    ("LeftCheekPuff", FacsRegion::Mouth),
    ("LeftDimpler", FacsRegion::Mouth),
    ("LeftLipCornerDown", FacsRegion::Mouth),
    ("LeftLowerLipDepressor", FacsRegion::Mouth),
    ("LeftLipCornerPuller", FacsRegion::Mouth),
    ("LeftLipStretcher", FacsRegion::Mouth),
    ("LeftUpperLipRaiser", FacsRegion::Mouth),
    ("RightCheekPuff", FacsRegion::Mouth),
    ("RightDimpler", FacsRegion::Mouth),
    ("RightLipCornerDown", FacsRegion::Mouth),
    ("RightLowerLipDepressor", FacsRegion::Mouth),
    ("RightLipCornerPuller", FacsRegion::Mouth),
    ("RightLipStretcher", FacsRegion::Mouth),
    ("RightUpperLipRaiser", FacsRegion::Mouth),
    // Jaw (3)
    ("JawDrop", FacsRegion::Jaw),
    ("JawLeft", FacsRegion::Jaw),
    ("JawRight", FacsRegion::Jaw),
    // Brows (9)
    ("Corrugator", FacsRegion::Brows),
    ("LeftBrowLowerer", FacsRegion::Brows),
    ("LeftOuterBrowRaiser", FacsRegion::Brows),
    ("LeftNoseWrinkler", FacsRegion::Brows),
    ("LeftInnerBrowRaiser", FacsRegion::Brows),
    ("RightBrowLowerer", FacsRegion::Brows),
    ("RightOuterBrowRaiser", FacsRegion::Brows),
    ("RightInnerBrowRaiser", FacsRegion::Brows),
    ("RightNoseWrinkler", FacsRegion::Brows),
    // Eyes (10)
    ("EyesLookDown", FacsRegion::Eyes),
    ("EyesLookLeft", FacsRegion::Eyes),
    ("EyesLookUp", FacsRegion::Eyes),
    ("EyesLookRight", FacsRegion::Eyes),
    ("LeftCheekRaiser", FacsRegion::Eyes),
    ("LeftEyeUpperLidRaiser", FacsRegion::Eyes),
    ("LeftEyeClosed", FacsRegion::Eyes),
    ("RightCheekRaiser", FacsRegion::Eyes),
    ("RightEyeUpperLidRaiser", FacsRegion::Eyes),
    ("RightEyeClosed", FacsRegion::Eyes),
    // Tongue (3)
    ("TongueDown", FacsRegion::Tongue),
    ("TongueOut", FacsRegion::Tongue),
    ("TongueUp", FacsRegion::Tongue),
];

/// Dual-slider opposing FACS pairs (`Constants.FacsCrossMappings.*.sliderGroup`).
/// Setting one > 0 zeroes out its opposing partner.
pub const FACS_SLIDER_GROUPS: &[(&str, &str)] = &[
    ("MouthRight", "MouthLeft"),
    ("JawRight", "JawLeft"),
    ("LeftEyeClosed", "LeftEyeUpperLidRaiser"),
    ("RightEyeClosed", "RightEyeUpperLidRaiser"),
    ("TongueUp", "TongueDown"),
];

/// Left/Right symmetric FACS control pairs (`Constants.FacsCrossMappings.*.symmetryPartner`).
pub const FACS_SYMMETRY_PAIRS: &[(&str, &str)] = &[
    // Mouth (7)
    ("LeftCheekPuff", "RightCheekPuff"),
    ("LeftDimpler", "RightDimpler"),
    ("LeftLipCornerDown", "RightLipCornerDown"),
    ("LeftLowerLipDepressor", "RightLowerLipDepressor"),
    ("LeftLipCornerPuller", "RightLipCornerPuller"),
    ("LeftLipStretcher", "RightLipStretcher"),
    ("LeftUpperLipRaiser", "RightUpperLipRaiser"),
    // Brows (4)
    ("LeftBrowLowerer", "RightBrowLowerer"),
    ("LeftOuterBrowRaiser", "RightOuterBrowRaiser"),
    ("LeftNoseWrinkler", "RightNoseWrinkler"),
    ("LeftInnerBrowRaiser", "RightInnerBrowRaiser"),
    // Eyes (3)
    ("LeftCheekRaiser", "RightCheekRaiser"),
    ("LeftEyeUpperLidRaiser", "RightEyeUpperLidRaiser"),
    ("LeftEyeClosed", "RightEyeClosed"),
];

pub fn facs_region(name: &str) -> Option<FacsRegion> {
    FACS_CONTROLS
        .iter()
        .find_map(|&(n, r)| (n == name).then_some(r))
}

pub fn is_facs_name(name: &str) -> bool {
    facs_region(name).is_some()
}

pub fn facs_symmetry_partner(name: &str) -> Option<&'static str> {
    for &(l, r) in FACS_SYMMETRY_PAIRS {
        if l == name {
            return Some(r);
        }
        if r == name {
            return Some(l);
        }
    }
    None
}

pub fn facs_slider_partner(name: &str) -> Option<&'static str> {
    for &(a, b) in FACS_SLIDER_GROUPS {
        if a == name {
            return Some(b);
        }
        if b == name {
            return Some(a);
        }
    }
    None
}

pub fn facs_controls_in_region(region: FacsRegion) -> impl Iterator<Item = &'static str> {
    FACS_CONTROLS
        .iter()
        .filter(move |&&(_, r)| r == region)
        .map(|&(name, _)| name)
}

// ============================================================================
// Animation-from-Video Status Codes (`Constants.ANIMATION_FROM_VIDEO_STATUS`)
// ============================================================================

pub mod video_status {
    pub const INITIALIZING: i32 = 0;
    pub const PENDING: i32 = 1;
    pub const PROCESSING: i32 = 2;
    pub const REQUIRES_MODERATION: i32 = 3;
    pub const ERROR: i32 = 4;
    pub const MODERATION_REJECTED: i32 = 5;
    pub const SUCCESS: i32 = 6;
    pub const INPUT_TOO_LONG: i32 = 7;
    pub const INPUT_HAS_NO_PERSON: i32 = 8;
    pub const INPUT_UNSTABLE: i32 = 9;
    pub const TIMED_OUT: i32 = 10;
    pub const CANCELLED: i32 = 11;
    pub const MULTIPLE_PEOPLE_IN_FRAME: i32 = 12;
    pub const UPLOADING_DATA: i32 = 1001;
    pub const ERROR_UPLOADING_DATA: i32 = 2001;
}

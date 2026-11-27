//! Complete editor state for `AnimationClipEditor`
//! (covers all 56+ `Status` reducer fields, `History`, `Notifications`, `Dialogs`,
//! `Menus`, `SelectionNode`, `DragContext`, `Clipboard`, `PlaybackState`, `IkState`,
//! `FacsEditorState`, `CurveEditorState`, `SettingsState`, and `RigCache`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use glam::Vec3;
use rbx_dom_weak::types::Ref;
use rustudio_core::math::Xform;
use serde::{Deserialize, Serialize};

use crate::anim_editor::constants::{
    video_status, FacsRegion, CURVE_CANVAS_MIN_RANGE, DEFAULT_FRAMERATE, GRID_SPEED_DEFAULT,
    GRID_SPEED_STATIC, MAX_ANIMATION_LENGTH, MAX_WAYPOINT_SIZE, TRACK_LIST_START_WIDTH,
};
use crate::anim_editor::model::{
    AnimationData, Events, Keyframe, RotationOrder, Tick, TrackPath, TrackType,
};

// ============================================================================
// Core State Enums
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum EditorMode {
    #[default]
    DopeSheet,
    Curves,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum PlayState {
    #[default]
    Pause,
    Play,
    Reverse,
    Record,
}

/// Maps to `Constants.TIMELINE_UNITS`:
/// - `Seconds` -> `SecondsDecimal` ("1.25")
/// - `SecondsFrames` -> `Seconds` ("1:08")
/// - `Frames` -> `Frames` ("38")
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TimelineUnit {
    #[default]
    Seconds,
    SecondsFrames,
    Frames,
}

impl TimelineUnit {
    pub const ORDER: &'static [TimelineUnit] = &[
        TimelineUnit::Seconds,
        TimelineUnit::SecondsFrames,
        TimelineUnit::Frames,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TimelineUnit::Seconds => "Seconds",
            TimelineUnit::SecondsFrames => "Seconds:Frames",
            TimelineUnit::Frames => "Frames",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SnapMode {
    Keyframes,
    #[default]
    Frames,
    None,
}

impl SnapMode {
    pub fn label(self) -> &'static str {
        match self {
            SnapMode::Keyframes => "Keyframes",
            SnapMode::Frames => "Frames",
            SnapMode::None => "None",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum IkMode {
    BodyPart,
    #[default]
    FullBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ToolMode {
    #[default]
    Rotate,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DragMode {
    Keyframe,
    Tangent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TangentSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipboardType {
    Events,
    Keyframes,
    Channels,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum GridSpeedSetting {
    #[default]
    Static,
    Default,
    FromRig,
    Custom(f32),
}

impl GridSpeedSetting {
    pub fn resolve_studs_per_sec(self, humanoid_walkspeed: Option<f32>) -> f32 {
        match self {
            GridSpeedSetting::Static => GRID_SPEED_STATIC,
            GridSpeedSetting::Default => GRID_SPEED_DEFAULT,
            GridSpeedSetting::FromRig => humanoid_walkspeed.unwrap_or(0.0),
            GridSpeedSetting::Custom(v) => v,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ReduceKeyframesDialogMode {
    #[default]
    Hidden,
    FromImportFbx,
    FromImportLiveAnimationCreator,
    FromImportFaceRecorder,
    FromMenu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RigTypeKind {
    R15,
    R6,
    #[default]
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RigJointKind {
    Motor6D,
    Bone,
    AnimationConstraint,
}

// ============================================================================
// Hierarchical Keyframe Selection (`Status.SelectedKeyframes`)
// ============================================================================

/// Recursive selection tree mirroring the track/component hierarchy.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SelectionNode {
    /// Union of ticks selected at this node and all descendant components.
    pub selection: BTreeSet<Tick>,
    pub components: BTreeMap<String, SelectionNode>,
}

impl SelectionNode {
    pub fn is_empty(&self) -> bool {
        self.selection.is_empty() && self.components.values().all(SelectionNode::is_empty)
    }

    pub fn contains_tick(&self, tick: Tick) -> bool {
        self.selection.contains(&tick)
    }

    pub fn prune_empty(&mut self) {
        self.components.retain(|_, child| {
            child.prune_empty();
            !child.is_empty()
        });
    }

    pub fn collect_bounds(&self, min_t: &mut Tick, max_t: &mut Tick, found: &mut bool) {
        if let Some(&first) = self.selection.iter().next() {
            *min_t = (*min_t).min(first);
            *found = true;
        }
        if let Some(&last) = self.selection.iter().next_back() {
            *max_t = (*max_t).max(last);
            *found = true;
        }
        for child in self.components.values() {
            child.collect_bounds(min_t, max_t, found);
        }
    }
}

/// `instance_name -> { top_track_name -> SelectionNode }`.
pub type SelectedKeyframes = BTreeMap<String, BTreeMap<String, SelectionNode>>;

/// Selected event ticks (`Status.SelectedEvents`).
pub type SelectedEvents = BTreeSet<Tick>;

/// Exact `AnimationData.getSelectionBounds(data, selectedKeyframes)`.
pub fn selection_bounds(selected: &SelectedKeyframes) -> Option<(Tick, Tick)> {
    let mut min_t = MAX_ANIMATION_LENGTH;
    let mut max_t = 0;
    let mut found = false;
    for tracks in selected.values() {
        for node in tracks.values() {
            node.collect_bounds(&mut min_t, &mut max_t, &mut found);
        }
    }
    if found {
        Some((min_t, max_t))
    } else {
        None
    }
}

pub fn is_selection_empty(selected: &SelectedKeyframes) -> bool {
    selected
        .values()
        .all(|tracks| tracks.values().all(SelectionNode::is_empty))
}

pub fn has_multiple_selected_ticks(selected: &SelectedKeyframes) -> bool {
    selection_bounds(selected).is_some_and(|(min_t, max_t)| min_t < max_t)
}

// ============================================================================
// Clipboard (`Status.Clipboard` & `Status.ClipboardType`)
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct ClipboardKeyframeEntry {
    pub top_track_name: String,
    pub top_track_type: TrackType,
    pub rel_path: TrackPath,
    pub track_type: TrackType,
    pub rotation_type: Option<TrackType>,
    pub euler_angles_order: Option<RotationOrder>,
    pub data: BTreeMap<Tick, Keyframe>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Clipboard {
    #[default]
    Empty,
    Keyframes(BTreeMap<String, Vec<ClipboardKeyframeEntry>>),
    Channels(BTreeMap<String, Vec<ClipboardKeyframeEntry>>),
    Events(BTreeMap<Tick, BTreeMap<String, String>>),
}

impl Clipboard {
    pub fn is_empty(&self) -> bool {
        match self {
            Clipboard::Empty => true,
            Clipboard::Keyframes(m) | Clipboard::Channels(m) => m.values().all(|v| v.is_empty()),
            Clipboard::Events(m) => m.is_empty(),
        }
    }

    pub fn clipboard_type(&self) -> Option<ClipboardType> {
        match self {
            Clipboard::Empty => None,
            Clipboard::Keyframes(_) => Some(ClipboardType::Keyframes),
            Clipboard::Channels(_) => Some(ClipboardType::Channels),
            Clipboard::Events(_) => Some(ClipboardType::Events),
        }
    }
}

// ============================================================================
// DragContext (`Util/DragContext`)
// ============================================================================

#[derive(Debug, Clone)]
pub enum DragContext {
    Keyframes {
        animation_data: AnimationData,
        selected_keyframes: SelectedKeyframes,
        pivot_tick: Tick,
        new_tick: Tick,
        pivot_value: Option<f32>,
        new_value: Option<f32>,
        scale: f64,
        earliest_tick: Tick,
        latest_tick: Tick,
        has_waypoint: bool,
    },
    Tangent {
        instance_name: String,
        path: TrackPath,
        tick: Tick,
        value: f32,
        side: TangentSide,
        track_type: TrackType,
        has_waypoint: bool,
    },
    Events {
        animation_data: AnimationData,
        selected_events: SelectedEvents,
        event_filters: BTreeSet<String>,
        pivot_tick: Tick,
        new_tick: Tick,
        has_waypoint: bool,
    },
}

impl DragContext {
    pub fn new_keyframes(
        animation_data: AnimationData,
        selected_keyframes: SelectedKeyframes,
        pivot_tick: Tick,
        pivot_value: Option<f32>,
    ) -> Self {
        let (earliest_tick, latest_tick) =
            selection_bounds(&selected_keyframes).unwrap_or((pivot_tick, pivot_tick));
        DragContext::Keyframes {
            animation_data,
            selected_keyframes,
            pivot_tick,
            new_tick: pivot_tick,
            pivot_value,
            new_value: pivot_value,
            scale: 1.0,
            earliest_tick,
            latest_tick,
            has_waypoint: false,
        }
    }

    pub fn new_tangent(
        instance_name: impl Into<String>,
        path: TrackPath,
        tick: Tick,
        value: f32,
        side: TangentSide,
        track_type: TrackType,
    ) -> Self {
        DragContext::Tangent {
            instance_name: instance_name.into(),
            path,
            tick,
            value,
            side,
            track_type,
            has_waypoint: false,
        }
    }

    pub fn new_events(
        animation_data: AnimationData,
        selected_events: SelectedEvents,
        event_filters: BTreeSet<String>,
        pivot_tick: Tick,
    ) -> Self {
        DragContext::Events {
            animation_data,
            selected_events,
            event_filters,
            pivot_tick,
            new_tick: pivot_tick,
            has_waypoint: false,
        }
    }

    pub fn move_keyframes(&mut self, tick: Tick, value: Option<f32>) {
        if let DragContext::Keyframes {
            new_tick,
            new_value,
            ..
        } = self
        {
            *new_tick = tick;
            *new_value = value;
        }
    }

    pub fn scale_keyframes(&mut self, tick: Tick) {
        if let DragContext::Keyframes {
            pivot_tick,
            new_tick,
            scale,
            earliest_tick,
            latest_tick,
            ..
        } = self
        {
            *new_tick = tick;
            let span = (*latest_tick - *earliest_tick) as f64;
            if span.abs() > f64::EPSILON {
                let delta = if *pivot_tick == *latest_tick {
                    (*pivot_tick - tick) as f64
                } else {
                    (tick - *pivot_tick) as f64
                };
                *scale = delta / span;
            }
        }
    }

    pub fn move_events(&mut self, tick: Tick) {
        if let DragContext::Events { new_tick, .. } = self {
            *new_tick = tick;
        }
    }
}

// ============================================================================
// Track List Entry (`Templates.trackListEntry`)
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct TrackListEntry {
    pub name: String,
    pub instance: String,
    pub depth: usize,
    pub expanded: bool,
    pub selected: bool,
    pub track_type: TrackType,
    pub euler_angles_order: Option<RotationOrder>,
    pub components: BTreeMap<String, TrackListEntry>,
    pub has_error: bool,
}

impl TrackListEntry {
    pub fn new(
        name: impl Into<String>,
        instance: impl Into<String>,
        track_type: TrackType,
        depth: usize,
    ) -> Self {
        Self {
            name: name.into(),
            instance: instance.into(),
            depth,
            expanded: false,
            selected: false,
            track_type,
            euler_angles_order: None,
            components: BTreeMap::new(),
            has_error: false,
        }
    }

    /// Exact `TrackUtils.getExpandedSize(track)`.
    pub fn expanded_row_count(&self) -> usize {
        if !self.components.is_empty() {
            if self.expanded {
                1 + self
                    .components
                    .values()
                    .map(TrackListEntry::expanded_row_count)
                    .sum::<usize>()
            } else {
                1
            }
        } else if self.expanded {
            if self.track_type == TrackType::CFrame {
                3
            } else {
                2
            }
        } else {
            1
        }
    }
}

// ============================================================================
// History (`Reducers/History`)
// ============================================================================

#[derive(Debug, Clone)]
pub struct HistoryWaypoint {
    pub animation_data: AnimationData,
    pub tracks: Vec<TrackListEntry>,
}

#[derive(Debug, Clone)]
pub struct HistoryState {
    /// Newest waypoint at index 0 (matching Luau `Past[1]`).
    pub past: Vec<HistoryWaypoint>,
    /// Newest redo waypoint at index 0 (matching Luau `Future[1]`).
    pub future: Vec<HistoryWaypoint>,
    pub max_waypoint_size: usize,
}

impl Default for HistoryState {
    fn default() -> Self {
        Self {
            past: Vec::new(),
            future: Vec::new(),
            max_waypoint_size: MAX_WAYPOINT_SIZE,
        }
    }
}

impl HistoryState {
    pub fn push_waypoint(&mut self, animation_data: AnimationData, tracks: Vec<TrackListEntry>) {
        self.past.insert(
            0,
            HistoryWaypoint {
                animation_data,
                tracks,
            },
        );
        self.past.truncate(self.max_waypoint_size);
        self.future.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
    }
}

// ============================================================================
// Notifications & Toasts (`Reducers/Notifications`, `Components/Toast/*`)
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct NotificationsState {
    pub quantize_warning: bool,
    pub saved: Option<String>,
    pub loaded: Option<String>,
    pub clipped_warning: bool,
    pub invalid_animation: bool,
    pub cannot_paste_error: bool,
}

impl NotificationsState {
    pub fn set_loaded(&mut self, name: Option<String>) {
        if name.is_some() {
            // Studio side effect: setting Loaded clears Saved.
            self.saved = None;
        }
        self.loaded = name;
    }
}

#[derive(Debug, Clone)]
pub enum ToastKind {
    Notice {
        text: String,
        started_at_secs: f64,
        show_secs: f64,
        fade_secs: f64,
    },
    Action {
        text: String,
        primary_label: String,
        secondary_label: String,
        action_id: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct ToastState {
    pub active: Option<ToastKind>,
}

impl ToastState {
    pub fn show_notice(&mut self, text: impl Into<String>, now_secs: f64) {
        self.active = Some(ToastKind::Notice {
            text: text.into(),
            started_at_secs: now_secs,
            show_secs: 3.0,
            fade_secs: 0.5,
        });
    }

    pub fn clear(&mut self) {
        self.active = None;
    }
}

// ============================================================================
// Dialogs & Menus (`07-menus-dialogs-io-l10n.md`)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationSeverity {
    Warning = 1,
    Error = 2,
}

#[derive(Debug, Clone)]
pub struct RigValidationIssue {
    pub id: String,
    pub message: String,
    pub severity: ValidationSeverity,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationChoice {
    Delete,
    Migrate,
    Ignore,
}

#[derive(Debug, Clone)]
pub struct MigrationItem {
    pub anim_ref: Ref,
    pub name: String,
    pub choice: MigrationChoice,
}

#[derive(Debug, Clone)]
pub struct ImportSettingsDialogState {
    pub fbx_path: Option<PathBuf>,
    pub rest_pose_source: String,
    pub scale_unit_index: usize,
    pub scale_factor_text: String,
    pub rig_type_index: usize,
    pub track_selection_items: Vec<String>,
    pub selected_track_index: usize,
}

impl Default for ImportSettingsDialogState {
    fn default() -> Self {
        Self {
            fbx_path: None,
            rest_pose_source: "AnimationEditorRig".to_string(),
            scale_unit_index: 0,
            scale_factor_text: "1.0".to_string(),
            rig_type_index: 0,
            track_selection_items: Vec::new(),
            selected_track_index: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EditEventsDialogState {
    pub tick: Tick,
    pub draft_events: Events,
    pub editing_event_name: Option<String>,
    pub editing_name_buffer: String,
    pub rename_all_prompt: Option<(String, String)>,
    pub delete_all_prompt: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ReduceKeyframesState {
    pub mode: ReduceKeyframesDialogMode,
    pub animation_backup: Option<AnimationData>,
    pub source_data: Option<AnimationData>,
    pub prioritized_ticks: Vec<Tick>,
    pub initial_keyframes: usize,
    pub target_keyframes: usize,
    pub has_moved: bool,
}

#[derive(Debug, Clone, Default)]
pub enum ActiveDialog {
    #[default]
    None,
    CreateNewAnimation {
        name: String,
        error: Option<String>,
    },
    SaveAs {
        name: String,
        error: Option<String>,
    },
    OverwriteConfirm {
        name: String,
        is_legacy: bool,
    },
    RenameKeyframe {
        tick: Tick,
        name: String,
    },
    ChangeDuration {
        text: String,
        current_duration_frames: i32,
        error: Option<String>,
    },
    ChangeFps {
        text: String,
        error: Option<String>,
    },
    ChangePlaybackSpeed {
        text: String,
        error: Option<String>,
    },
    ChangeGridSpeed {
        text: String,
        error: Option<String>,
    },
    EditEvents(EditEventsDialogState),
    PromoteToCurves,
    ReduceKeyframes(ReduceKeyframesState),
    RigErrors {
        title: String,
        issues: Vec<RigValidationIssue>,
    },
    Migration {
        items: Vec<MigrationItem>,
    },
    ImportSettings(ImportSettingsDialogState),
    ImportFromRoblox {
        asset_id_text: String,
        error: Option<String>,
    },
    InProgress {
        text: String,
        cancellable: bool,
    },
}

#[derive(Debug, Clone, Default)]
pub struct MenuState {
    pub show_clip_menu: bool,
    pub show_settings_menu: bool,
    pub show_track_menu: bool,
    pub show_timeline_menu: bool,
    pub show_event_menu: bool,
    pub show_tangent_menu: bool,
    pub show_add_track_menu: bool,
    pub show_ik_menu: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RightClickContextInfo {
    pub tick: Option<Tick>,
    pub on_keyframe: bool,
    pub on_event: bool,
    pub summary_keyframe: Option<Tick>,
    pub instance_name: Option<String>,
    pub path: Option<TrackPath>,
    pub tangent_side: Option<TangentSide>,
}

// ============================================================================
// Sub-states: Playback, IK, FACS, Curve Editor, Viewport Dragger
// ============================================================================

#[derive(Debug, Clone)]
pub struct PlaybackState {
    pub play_state: PlayState,
    pub playhead: Tick,
    pub playback_speed: f32,
    pub start_time_secs: Option<f64>,
    pub start_playhead: Tick,
    pub editing_length: Tick,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            play_state: PlayState::Pause,
            playhead: 0,
            playback_speed: 1.0,
            start_time_secs: None,
            start_playhead: 0,
            editing_length: 0,
        }
    }
}

impl PlaybackState {
    pub fn set_play_state(&mut self, state: PlayState) {
        if self.play_state != state {
            self.play_state = state;
            self.start_time_secs = None;
        }
    }
}

#[derive(Debug, Clone)]
pub struct IkState {
    pub enabled: bool,
    pub mode: IkMode,
    pub show_tree: bool,
    pub pinned_parts: HashSet<String>,
    pub opened_timestamp: Option<f64>,
    pub is_selecting_target: bool,
}

impl Default for IkState {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: IkMode::FullBody,
            show_tree: false,
            pinned_parts: HashSet::new(),
            opened_timestamp: None,
            is_selecting_target: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FacsEditorState {
    pub show_panel: bool,
    pub symmetry_enabled: bool,
    pub auto_focus_face_enabled: bool,
    pub opened_timestamp: Option<f64>,
    pub active_region: FacsRegion,
}

impl Default for FacsEditorState {
    fn default() -> Self {
        Self {
            show_panel: false,
            symmetry_enabled: false,
            auto_focus_face_enabled: false,
            opened_timestamp: None,
            active_region: FacsRegion::Mouth,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveEditorView {
    pub min_value: f32,
    pub max_value: f32,
    pub vertical_scroll: f32,
    pub vertical_zoom: f32,
    pub horizontal_scroll: f32,
    pub horizontal_zoom: f32,
}

impl Default for CurveEditorView {
    fn default() -> Self {
        Self {
            min_value: 0.0,
            max_value: CURVE_CANVAS_MIN_RANGE,
            vertical_scroll: 0.0,
            vertical_zoom: 0.0,
            horizontal_scroll: 0.0,
            horizontal_zoom: 0.0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CurveEditorState {
    pub view: CurveEditorView,
    pub marquee_start: Option<(f32, f32)>,
    pub marquee_end: Option<(f32, f32)>,
    pub is_dragging_marquee: bool,
    pub test_scroll_min: f32,
    pub test_scroll_max: f32,
}

#[derive(Debug, Clone)]
pub struct ViewportDraggerState {
    pub tool: ToolMode,
    pub world_space: bool,
    pub dragging: bool,
    pub drag_start_transforms: HashMap<String, Xform>,
    pub has_drag_waypoint: bool,
    pub hovered_part: Option<String>,
}

impl Default for ViewportDraggerState {
    fn default() -> Self {
        Self {
            tool: ToolMode::Rotate,
            world_space: false,
            dragging: false,
            drag_start_transforms: HashMap::new(),
            has_drag_waypoint: false,
            hovered_part: None,
        }
    }
}

// ============================================================================
// Persistent Settings (`SettingsState` -> JSON in user config dir)
// ============================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsState {
    pub timeline_unit: TimelineUnit,
    pub snap_mode: SnapMode,
    pub default_rotation_type: TrackType,
    pub default_euler_angles_order: RotationOrder,
    pub show_events: bool,
    pub visualize_bones: bool,
    pub visualize_grid: bool,
    pub grid_speed: GridSpeedSetting,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            timeline_unit: TimelineUnit::Seconds,
            snap_mode: SnapMode::Frames,
            default_rotation_type: TrackType::EulerAngles,
            default_euler_angles_order: RotationOrder::XYZ,
            show_events: true,
            visualize_bones: true,
            visualize_grid: true,
            grid_speed: GridSpeedSetting::Static,
        }
    }
}

impl SettingsState {
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("rustudio").join("anim_editor_settings.json"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return Self::default();
        };
        serde_json::from_slice(&bytes).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::config_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }
}

// ============================================================================
// Rig Cache (`RigInfo` / `RigUtils` runtime cache)
// ============================================================================

#[derive(Debug, Clone)]
pub struct RigJointInfo {
    pub name: String,
    pub kind: RigJointKind,
    pub joint_ref: Ref,
    pub part0_name: Option<String>,
    pub part1_name: String,
    pub part0_ref: Option<Ref>,
    pub part1_ref: Option<Ref>,
    pub c0: Xform,
    pub c1: Xform,
}

#[derive(Debug, Clone)]
pub struct RigAnimSaveEntry {
    pub anim_ref: Ref,
    pub name: String,
    pub is_legacy_anim_saves: bool,
    pub timestamp_tag: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct RigCache {
    pub root_ref: Option<Ref>,
    pub root_name: String,
    pub root_part_ref: Option<Ref>,
    pub root_part_name: Option<String>,
    pub rig_type: RigTypeKind,
    pub root_cframe: Option<Xform>,
    pub rig_scale: f32,
    pub motor6d_map: HashMap<String, Ref>,
    pub bone_map: HashMap<String, Ref>,
    pub anim_constraint_map: HashMap<String, Ref>,
    pub joints_by_part: HashMap<String, RigJointInfo>,
    pub part_refs: HashMap<String, Ref>,
    pub face_controls_ref: Option<Ref>,
    /// `child_part_name -> parent_part_name`
    pub parent_map: HashMap<String, String>,
    /// `parent_part_name -> ordered children`
    pub children_map: HashMap<String, Vec<String>>,
    /// Depth-first traversal order starting from root part.
    pub hierarchy_order: Vec<String>,
    /// Saved joint transforms captured when attaching to the rig (`Status.StartingPose`).
    pub starting_pose: HashMap<String, Xform>,
    pub anim_saves: Vec<RigAnimSaveEntry>,
    pub legacy_anim_saves: Vec<RigAnimSaveEntry>,
    pub has_humanoid: bool,
    pub humanoid_walkspeed: Option<f32>,
    pub validation_issues: Vec<RigValidationIssue>,
}

impl RigCache {
    pub fn clear(&mut self) {
        *self = Self::default();
        self.rig_scale = 1.0;
    }

    pub fn has_errors(&self) -> bool {
        self.validation_issues
            .iter()
            .any(|i| i.severity == ValidationSeverity::Error)
    }

    pub fn has_face_controls(&self) -> bool {
        self.face_controls_ref.is_some()
    }

    pub fn root_position(&self) -> Vec3 {
        self.root_cframe.map(|cf| cf.pos).unwrap_or(Vec3::ZERO)
    }
}

// ============================================================================
// Top-Level EditorState
// ============================================================================

/// Complete runtime state for RusTudio's native `AnimationClipEditor`.
#[derive(Debug, Clone)]
pub struct EditorState {
    // Lifecycle & editing status
    pub active: bool,
    pub read_only: bool,
    pub is_dirty: bool,
    pub is_legacy_anim_saves: bool,
    pub editor_mode: EditorMode,
    pub editor_mode_switch_time: Option<f64>,
    pub frame_rate: u32,

    // Layout & scrolling
    pub track_list_width: f32,
    pub top_track_index: usize,

    // Track list & selection
    pub tracks: Vec<TrackListEntry>,
    pub unused_tracks: Vec<String>,
    pub unused_facs: Vec<String>,
    pub selected_tracks: Vec<TrackPath>,
    pub selected_track_instances: Vec<Ref>,
    pub last_selected_path: Option<TrackPath>,
    pub selected_keyframes: SelectedKeyframes,
    pub selected_events: SelectedEvents,

    // Event filtering & editing
    pub available_events: Vec<String>,
    pub event_filters: BTreeSet<String>,
    pub event_editing_tick: Option<Tick>,

    // Clipboard, drag & context menus
    pub clipboard: Clipboard,
    pub clipboard_type: Option<ClipboardType>,
    pub drag_context: Option<DragContext>,
    pub right_click_context: RightClickContextInfo,

    // Document, undo/redo, notifications, dialogs, menus
    pub animation_data: Option<AnimationData>,
    pub history: HistoryState,
    pub notifications: NotificationsState,
    pub toast: ToastState,
    pub active_dialog: ActiveDialog,
    pub menus: MenuState,

    // Subsystem states
    pub playback: PlaybackState,
    pub ik: IkState,
    pub facs: FacsEditorState,
    pub curve_editor: CurveEditorState,
    pub viewport_dragger: ViewportDraggerState,
    pub settings: SettingsState,
    pub rig_cache: RigCache,

    // Misc Status reducer fields
    pub last_synced_selection: Vec<Ref>,
    pub in_review_state: bool,
    pub have_to_set_back_to_not_looping: bool,
    pub ignore_root_cframe_signal: bool,
    pub creating_animation_from_video: bool,
    pub animation_from_video_errored_out: bool,
    pub animation_import_progress: f32,
    pub animation_import_status: i32,
}

pub type AnimationEditor = EditorState;

impl Default for EditorState {
    fn default() -> Self {
        let mut rig_cache = RigCache::default();
        rig_cache.rig_scale = 1.0;

        Self {
            active: false,
            read_only: false,
            is_dirty: false,
            is_legacy_anim_saves: false,
            editor_mode: EditorMode::DopeSheet,
            editor_mode_switch_time: None,
            frame_rate: DEFAULT_FRAMERATE,

            track_list_width: TRACK_LIST_START_WIDTH,
            top_track_index: 1,

            tracks: Vec::new(),
            unused_tracks: Vec::new(),
            unused_facs: Vec::new(),
            selected_tracks: Vec::new(),
            selected_track_instances: Vec::new(),
            last_selected_path: None,
            selected_keyframes: SelectedKeyframes::new(),
            selected_events: SelectedEvents::new(),

            available_events: Vec::new(),
            event_filters: BTreeSet::new(),
            event_editing_tick: None,

            clipboard: Clipboard::Empty,
            clipboard_type: None,
            drag_context: None,
            right_click_context: RightClickContextInfo::default(),

            animation_data: None,
            history: HistoryState::default(),
            notifications: NotificationsState::default(),
            toast: ToastState::default(),
            active_dialog: ActiveDialog::None,
            menus: MenuState::default(),

            playback: PlaybackState::default(),
            ik: IkState::default(),
            facs: FacsEditorState::default(),
            curve_editor: CurveEditorState::default(),
            viewport_dragger: ViewportDraggerState::default(),
            settings: SettingsState::load(),
            rig_cache,

            last_synced_selection: Vec::new(),
            in_review_state: false,
            have_to_set_back_to_not_looping: false,
            ignore_root_cframe_signal: false,
            creating_animation_from_video: false,
            animation_from_video_errored_out: false,
            animation_import_progress: 0.0,
            animation_import_status: video_status::INITIALIZING,
        }
    }
}

impl EditorState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_channel_animation(&self) -> bool {
        self.animation_data
            .as_ref()
            .is_some_and(AnimationData::is_channel_animation)
    }

    pub fn add_waypoint(&mut self) {
        if let Some(anim) = &self.animation_data {
            self.history
                .push_waypoint(anim.clone(), self.tracks.clone());
            self.is_dirty = true;
        }
    }

    pub fn clear_selection(&mut self) {
        self.selected_keyframes.clear();
        self.selected_events.clear();
    }

    /// Rebuilds `selected_track_instances` from `selected_tracks` using
    /// `rig_cache` so TrackList / Summary / IK selection and 3D viewport
    /// SelectionBoxes / gizmos always stay in sync.
    pub fn sync_selected_track_instances(&mut self) {
        self.selected_track_instances.clear();
        for path in &self.selected_tracks {
            let Some(joint_name) = path.first() else {
                continue;
            };
            let inst_ref = self
                .rig_cache
                .joints_by_part
                .get(joint_name)
                .and_then(|j| match j.kind {
                    RigJointKind::Bone => Some(j.joint_ref),
                    RigJointKind::Motor6D | RigJointKind::AnimationConstraint => j.part1_ref,
                })
                .or_else(|| self.rig_cache.bone_map.get(joint_name).copied())
                .or_else(|| self.rig_cache.part_refs.get(joint_name).copied());
            if let Some(r) = inst_ref {
                if !self.selected_track_instances.contains(&r) {
                    self.selected_track_instances.push(r);
                }
            }
        }
    }
}

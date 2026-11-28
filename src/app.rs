//! The editor: state shared by every panel, menus, toolbar, shortcuts and
//! the docked layout.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use egui::{Key, KeyboardShortcut, Modifiers, RichText};
use egui_dock::{DockArea, DockState, TabViewer};
use glam::Vec3;
use rbx_dom_weak::types::Ref;
use rbx_dom_weak::InstanceBuilder;

use rustudio_core::doc::{self, DocKind, Document};
use crate::explorer::ExplorerState;
use rustudio_core::mech;
use crate::properties::PropertiesState;
use rustudio_render::render::MeshKind;
use crate::selftest::SelfTest;
use rustudio_engine::theme;
use crate::viewport::GizmoDrag;

pub use rustudio_core::log::{Level, LogLine, tee_log};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    None,
    Transform,
    Select,
    Move,
    Scale,
    Rotate,
}

impl Tool {
    pub fn toggle(self, requested: Self) -> Self {
        if self == requested { Self::None } else { requested }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::None => "None",
            Tool::Transform => "Transform",
            Tool::Select => "Select",
            Tool::Move => "Move",
            Tool::Scale => "Scale",
            Tool::Rotate => "Rotate",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Viewport,
    Explorer,
    Properties,
    Output,
    /// Studio's Avatar Settings (File > Avatar Settings).
    AvatarSettings,
    /// Studio's Terrain Editor (Home / View > Terrain Editor).
    TerrainEditor,
    /// Studio's Animation Editor (Avatar > Clip Editor): src/anim_editor.
    AnimationEditor,
    /// Studio's Toolbox (Home / View > Toolbox): src/toolbox.
    Toolbox,
    /// Studio's Asset Manager (Home > Assets, Window > Asset Manager): src/asset_manager.
    AssetManager,
    /// File Sync (src/file_sync): the project attached to the place.
    FileSync,
    /// View > MicroProfiler (Ctrl+F6): the profiler window
    /// (crates/rustudio-profiler-ui), its own OS window by default.
    Profiler,
    /// A script open in the code editor.
    Script(Ref),
    /// A plugin's DockWidgetPluginGui (src/plugin_host/widgets_ui.rs: the
    /// slot number of its `edit_<plugin>_<id>` key).
    Plugin(u32),
}

impl Tab {
    fn title(self) -> &'static str {
        match self {
            Tab::Viewport => "Viewport",
            Tab::Explorer => "Explorer",
            Tab::Properties => "Properties",
            Tab::Output => "Output",
            Tab::AvatarSettings => "Avatar Settings",
            Tab::TerrainEditor => "Terrain Editor",
            Tab::AnimationEditor => "Animation Editor",
            Tab::Toolbox => "Toolbox",
            Tab::AssetManager => "Asset Manager",
            Tab::FileSync => "File Sync",
            Tab::Profiler => "MicroProfiler",
            Tab::Script(_) => "Script",
            Tab::Plugin(_) => "Plugin",
        }
    }
}

/// Counters the self-test reads to know each panel was actually drawn.
#[derive(Default, Debug, Clone)]
pub struct HudStats {
    pub explorer_rows: usize,
    pub property_rows: usize,
    pub output_lines: usize,
    pub viewport_drawn: bool,
    pub toolbar_buttons: usize,
    pub status_drawn: bool,
}

pub struct Harness {
    pub run: RunHarness,
    pub selftest: Option<SelfTest>,
    /// `--ui-shots DIR`: pictures of the chrome (src/ui_shots.rs).
    pub ui_shots: Option<crate::ui_shots::UiShots>,
    /// `--open-roblox ID|URL`: open that place from Roblox once the session
    /// is checked.
    pub open_roblox: Option<(u64, bool)>,
    pub team_create: Option<(u64, u64)>,
    /// `--roblox-picker TAB`: show File > Open from Roblox on that tab.
    pub roblox_picker: Option<crate::cloud_ui::PickerTab>,
    pub roblox_settings: bool,
    /// `--action NAME@S` menu actions now due (run where the egui context is).
    pub ui_due: Vec<String>,
    /// `--overlays`: with --no-ui, keep the editor's view overlays (grid,
    /// selection, gizmos, hover, view cube, readouts) in Edit state.
    pub overlays: bool,
    /// `--plugin FILE` (repeatable) / `--plugins-dir DIR`: plugins loaded
    /// once the place is open; `--no-plugins`: none at all.
    pub plugin_files: Vec<PathBuf>,
    /// --installed-plugin ASSETID:FILE (loaded as cloud_<id>).
    pub installed_plugins: Vec<(u64, PathBuf)>,
    pub plugin_dirs: Vec<PathBuf>,
    pub no_plugins: bool,
    pub plugins_started: bool,
}

#[cfg(test)]
impl Harness {
    pub fn test_empty() -> Self {
        Self {
            run: RunHarness {
                shot_after: None,
                log_out: None,
                input: crate::run_harness::HarnessInput {
                    actions: Vec::new(),
                    pointer: None,
                    shift: false,
                },
                started: rustudio_core::sim_clock::now(),
                held: 0.0,
                screenshot: None,
                screenshot_requested: false,
                start_play: false,
                camera: None,
                shots: Vec::new(),
                shot_next: 0,
                shot_lines: Vec::new(),
                from_play: false,
                play_started: None,
                play_game_time: None,
                frame_ms: Vec::new(),
                last_frame: None,
                no_ui: true,
                no_render: true,
                show_core_gui: false,
                show_core_gui_explicit: false,
                end_after: None,
                fail_on_error: false,
                result: None,
                end_value: None,
                run_frames: Vec::new(),
                run_last: None,
                run_playing: false,
                finished: false,
                bench: None,
                profile: None,
            },
            selftest: None,
            ui_shots: None,
            open_roblox: None,
            team_create: None,
            roblox_picker: None,
            roblox_settings: false,
            ui_due: Vec::new(),
            overlays: false,
            plugin_files: Vec::new(),
            installed_plugins: Vec::new(),
            plugin_dirs: Vec::new(),
            no_plugins: true,
            plugins_started: false,
        }
    }
}

use crate::game::Game;
use crate::run_harness::{RunHarness, frame_grid};
pub use crate::run_harness::{EXIT_CODE, HarnessAction, save_png};
use rustudio_render::{camera::Camera, scene::Scene};

impl Harness {
    /// A plain interactive run: no screenshots, tests or timed actions (the
    /// Android app, which has no command line; `run` fills the fields from
    /// its options).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn interactive() -> Self {
        Harness {
            run: RunHarness {
                input: Default::default(),
                shot_after: None,
                log_out: None,
                started: rustudio_core::sim_clock::now(),
                screenshot: None,
                screenshot_requested: false,
                start_play: false,
                bench: None,
                profile: None,
                camera: None,
                shots: Vec::new(),
                shot_next: 0,
                shot_lines: Vec::new(),
                from_play: false,
                play_started: None,
                play_game_time: None,
                frame_ms: Vec::new(),
                last_frame: None,
                no_ui: false,
                no_render: false,
                // (the Core UI shows, as in a windowed desktop run)
                show_core_gui: true,
                show_core_gui_explicit: false,
                end_after: None,
                fail_on_error: false,
                result: None,
                end_value: None,
                run_frames: Vec::new(),
                run_last: None,
                run_playing: false,
                finished: false,
                held: 0.0,
            },
            selftest: None,
            ui_shots: None,
            open_roblox: None,
            team_create: None,
            roblox_picker: None,
            roblox_settings: false,
            ui_due: Vec::new(),
            overlays: false,
            plugin_files: Vec::new(),
            installed_plugins: Vec::new(),
            plugin_dirs: Vec::new(),
            no_plugins: false,
            plugins_started: false,
        }
    }
}

pub struct Editor {
    pub game: Game,
    pub tool: Tool,
    /// Model tab Collisions: handle moves / scales stop where parts would
    /// overlap (TransformHandlesImplementation._safelyTransformParts).
    pub collisions: bool,
    /// Model tab Join Surfaces: drops weld touching faces (JointPairs).
    pub join_surfaces: bool,
    /// Dragger modes: Physical (DraggerMovementMode.Physical) drags in the
    /// camera plane like Studio's IK dragger; Geometric rests on surfaces.
    pub physical_drag: bool,
    /// Snap to Parts (DraggerService.PartSnapEnabled): free drags snap the
    /// selection's edges / centre to the target face's (soft snaps).
    pub part_snap: bool,
    /// Use Bounding Boxes (DraggerService.UseBoundingBoxes): the part under
    /// the mouse is treated as its box when building the drop surface.
    pub use_bounding_boxes: bool,
    /// Align Dragged Objects (StudioService.AlignDraggedObjects): dropped
    /// selections turn to the nearest axis alignment of the target surface.
    pub align_dragged: bool,
    pub show_grid: bool,
    /// The scene alone in the 3D view, without the editor's own drawing (the
    /// ground grid, selection and hover boxes, handles, the drag box, the
    /// tool readout and axis compass): comparison captures (src/control.rs).
    pub clean_view: bool,
    pub clipboard: Option<Vec<u8>>,
    pub hover: Option<Ref>,
    pub(crate) hover_pick: crate::viewport::HoverPick,
    pub drag: Option<GizmoDrag>,
    pub marquee: Option<egui::Pos2>,
    /// The selection when the box started (Shift / Ctrl add to it).
    pub marquee_base: Vec<Ref>,
    pub(crate) marquee_centres: crate::viewport::MarqueeCentres,
    /// The Output panel's toolbar choices (src/output_panel.rs).
    pub output_view: crate::output_panel::OutputState,
    /// Dragging parts with the Select / Move tool (src/part_drag.rs).
    pub part_drag: Option<crate::part_drag::PartDrag>,
    pub explorer: ExplorerState,
    pub props: PropertiesState,
    pub(crate) panel_cache: crate::panel_cache::Panels,
    /// The character builder window (Home / Avatar → Character).
    pub chars: crate::char_builder::CharBuilder,
    /// Places stored on Roblox: File > Open from Roblox, Save to Roblox.
    pub cloud: crate::cloud_ui::CloudUi,
    pub(crate) account_destination: Option<crate::account_ui::Picker>,
    /// Scripts to open (or focus) as editor tabs at the end of the frame.
    pub open_script: Vec<Ref>,
    /// Script tabs a plugin closed (ScriptDocument:CloseAsync).
    pub close_script: Vec<Ref>,
    document_tabs_reset: bool,
    pub scripts: crate::script_editor::ScriptTabs,
    /// Studio plugins in the edit DataModel (src/plugin_host).
    pub plugins: crate::plugin_host::PluginHost,
    /// Latched built-in plugin toggle and per-tool native/Roblox choices.
    pub builtin_tools: rustudio_core::rustudio_settings::BuiltinToolChoices,
    /// Plugin toolbars / menus / shortcuts as the editor draws them
    /// (src/plugin_host/ui_ribbon.rs).
    pub plugin_bar: crate::plugin_host::ui_ribbon::PluginUi,
    /// Plugin dock widgets' panels, CoreGui over the edit viewport, and the
    /// edit-mode input plugins hear (src/plugin_host/widgets_ui.rs).
    pub plugin_ui: crate::plugin_host::widgets_ui::PluginUi,
    /// RUSTUDIO_CONTROL: the comparison-capture socket (src/control.rs).
    #[cfg(unix)]
    pub control: Option<crate::control::Control>,
    /// The built-in MCP server and its panel (src/mcp).
    pub mcp: crate::mcp::Mcp,
    /// File Sync: a project folder attached to the open place (src/file_sync).
    pub file_sync: crate::file_sync::FileSync,
    /// The Terrain Editor's tools and settings.
    pub terrain_ed: crate::terrain_editor::TerrainEditor,
    /// The Animation Editor's state (src/anim_editor).
    pub anim_editor: crate::anim_editor::AnimationEditor,
    /// The Toolbox panel's state (src/toolbox), kept while it is hidden.
    pub toolbox: crate::toolbox::Toolbox,
    /// The Asset Manager panel's state (src/asset_manager), kept while hidden.
    pub asset_manager: crate::asset_manager::AssetManager,
    /// The MicroProfiler window while its tab is open (recording runs while
    /// it exists; dropping it stops recording).
    pub profiler: Option<rustudio_profiler_ui::ProfilerWindow>,
    /// This frame's Toolbox preview ViewportFrames (drawn with the GUI's).
    pub toolbox_vf: std::collections::HashMap<rbx_dom_weak::types::Ref, rustudio_render::render::vf::Request>,
    /// Physical key positions held (movement keys; src/keys.rs).
    /// Seconds the fly keys have been held / released (speed ramp).
    pub fly_held: f32,
    pub fly_idle: f32,
    /// Last wheel zoom: (cursor ray direction, hit distance) for the self-test.
    pub last_zoom: Option<(Vec3, Option<f32>)>,
    pub hud: HudStats,
    pub ui_rects: HashMap<&'static str, egui::Rect>,
    /// Screen positions of gizmo handles last frame (axis, sign) -> point.
    pub gizmo_points: Vec<(crate::viewport::Handle, egui::Pos2)>,
    /// Selection / hover outlines kept while nothing they depend on changes.
    pub outline_cache: std::cell::RefCell<crate::viewport::OutlineCache>,
    /// The selected parts and what the ribbon asks of them, for one
    /// document state and selection (SelectionInfo).
    pub selection_info: std::cell::RefCell<SelectionInfo>,
    /// The view selector's meshes for the current view.
    pub selector_cache: crate::viewport::SelectorCache,
    /// The hovered instance's outline parts, for (hover, journal end).
    pub hover_outline_refs: (Option<(rbx_dom_weak::types::Ref, u64, u64)>, Vec<rbx_dom_weak::types::Ref>),
    /// Android: the points of the window's bottom the soft keyboard lies
    /// over while a game TextBox is typed into (`RustudioApp::frame`; the
    /// panels are not laid out above it then: `Editor::keyboard_pan`).
    /// 0 otherwise, and on a desktop.
    pub keyboard_cover: f32,
    /// How far the 3D view, its GUIs with it, is slid up to show the
    /// TextBox typed into above the soft keyboard (`keyboard_pan`).
    pub view_pan: f32,
    pub harness: Harness,
    pub last_frame: Instant,
    pub fps: f32,
    pub confirm_discard: Option<PendingAction>,
    /// Selection as of the last Explorer sync.
    pub last_selection: Vec<Ref>,
    /// The first frame picks the UI zoom from the screen (Android, a fresh
    /// layout: android::touch::auto_zoom).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub auto_zoom: bool,
    /// CPU time of the last frame's update + draw submission (ms).
    pub last_work_ms: f32,
    /// Play Fills the Screen's Stop button is pressed (`play_stop_button`:
    /// the release over it stops the game, however long the press).
    pub stop_pressed: bool,
    /// … and its Studio button (`play_full_button`).
    pub full_pressed: bool,
    /// View > Play Fills the Screen is on, but this Play's game was put
    /// back in the dock's 3D view by the Studio button at its top right
    /// (Output, Explorer, plugins and the ribbon's Stop around it); the
    /// view's Fill the Screen button undoes it. Not saved: the next Play
    /// fills the screen again.
    pub play_full_exit: bool,
    /// The open project folder (files are the truth; src/project_sync.rs).
    pub project: Option<crate::project_sync::ProjectState>,
    /// A folder with several project files: (folder, the files) to choose from.
    pub project_choice: Option<(PathBuf, Vec<PathBuf>)>,
    /// File > Save as Project Folder… / Project Settings… (src/project_ui.rs).
    pub project_export: Option<crate::project_ui::ExportDialog>,
    pub project_settings: Option<crate::project_ui::SettingsDialog>,
    /// Studio plugins: the local / installed store, Manage Plugins, and the
    /// runtime they're handed to (src/plugin_store).
    pub plugin_store: crate::plugin_store::manager::PluginManager,
    /// Open / save file dialogs, run off the UI thread (src/file_dialog.rs).
    pub dialogs: crate::file_dialog::Dialogs,
    /// Studio's chrome: ribbon tab, run type, panel toggles, popups (src/studio_ui.rs).
    pub ui: crate::studio_ui::UiState,
    /// View > Screenshot waiting for the frame.
    pub pending_user_shot: Option<PathBuf>,
    /// The Start Page (src/start_page.rs): shown when nothing is open.
    pub start: crate::start_page::StartPage,
    /// Requests switching to the main viewport tab.
    pub switch_to_viewport: bool,
}

#[derive(Debug, Clone)]
pub enum PendingAction {
    New,
    Open(Option<PathBuf>),
    OpenFolder,
    /// A place from Roblox: place id, a version (else the latest), and
    /// whether to take a local copy of a Team Create place.
    OpenCloud(u64, Option<u64>, bool),
    /// File > Close Place: back to the Start Page.
    Close,
    Quit,
    /// File > Restart rustudio: a fresh process on the same place.
    Restart,
}

pub struct RustudioApp {
    watchdog: crate::frame_watchdog::Watchdog,
    updater: crate::update::Updater,
    pub(crate) last_ui_frame: Option<Instant>,
    last_plugin_tick: Instant,
    plugins_ticked_between_frames: bool,
    /// The window's egui context, for the ticks run between frames.
    egui_ctx: Option<egui::Context>,
    last_hidden_tick: Instant,
    pub dock: DockState<Tab>,
    pub ed: Editor,
    /// Keeps the dock, window and view settings (src/layout_store.rs).
    layout: Option<crate::layout_store::Saver>,
    /// Panels popped out into their own windows, and where each panel was
    /// last docked (src/panels.rs).
    pub panels: crate::panels::Panels,
    /// Room for the soft keyboard (src/android/keyboard.rs).
    #[cfg(target_os = "android")]
    keyboard: crate::android::keyboard::Keyboard,
    /// The display cutout's insets, which the frame is laid out short of
    /// (src/android/cutout.rs).
    #[cfg(target_os = "android")]
    cutout: crate::android::cutout::Cutout,
    /// The text fields' long-press menu (src/android/text_menu.rs).
    #[cfg(target_os = "android")]
    text_menu: crate::android::text_menu::TextMenu,
    /// Cut / Copy / Paste / Select all above the soft keyboard
    /// (src/android/text_bar.rs).
    #[cfg(target_os = "android")]
    text_bar: crate::android::text_bar::TextBar,
    /// The display's refresh rate in Hz (0 unknown): the frame budget the
    /// CPU governor is told (src/android/perf_hint.rs).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    refresh_hz: f32,
    /// eframe's CPU time of the frame before (`eframe::Frame::info`, the
    /// wait for the display left out), kept by `ui` for `frame`: the
    /// governor's hint. None offscreen (src/offscreen.rs) and before the
    /// first frame.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    cpu_usage: Option<f32>,
    /// When the next frame may start under the Editor frame limit, and
    /// how long the last frame slept for it (`pace_frame`, Android: the
    /// desktop's pacing is native_loop.rs's).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pace_next: Option<Instant>,
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    paced_last: f32,
    /// The rate last voted for the window (src/android/frame_rate.rs; 0:
    /// any) and when; the display's rate is read again on the same clock.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    rate_voted: Option<(f32, Instant)>,
    /// The frame report (perf_report.rs): a line to the log every few
    /// seconds — on by default on a phone, `RUSTUDIO_PERF_LOG=1` on a
    /// computer, `=0` off.
    perf_report: Option<rustudio_player::perf_report::PerfReport>,
}

/// Studio's starting layout, from the placement table (src/panels.rs):
/// Explorer right, Properties under it, Output under the 3D view.
pub fn default_dock() -> DockState<Tab> {
    default_dock_for(rustudio_core::rustudio_settings::BuiltinToolChoices::default())
}

pub fn default_dock_for(choices: rustudio_core::rustudio_settings::BuiltinToolChoices) -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Viewport]);
    let none = HashMap::new();
    for t in [Tab::Explorer, Tab::Properties, Tab::Output] {
        crate::panels::place(&mut dock, t, &none);
    }
    if choices.use_native(rustudio_core::rustudio_settings::BuiltinTool::Toolbox) {
        crate::panels::place(&mut dock, Tab::Toolbox, &none);
    }
    dock
}

fn native_tab_enabled(tab: Tab, choices: rustudio_core::rustudio_settings::BuiltinToolChoices) -> bool {
    use rustudio_core::rustudio_settings::BuiltinTool;
    match tab {
        Tab::Toolbox => choices.use_native(BuiltinTool::Toolbox),
        Tab::TerrainEditor => choices.use_native(BuiltinTool::TerrainEditor),
        Tab::AnimationEditor => choices.use_native(BuiltinTool::AnimationEditor),
        _ => true,
    }
}

fn strip_disabled_native_tabs(
    dock: &mut DockState<Tab>,
    panels: &mut crate::panels::Panels,
    choices: rustudio_core::rustudio_settings::BuiltinToolChoices,
) {
    for tab in [Tab::Toolbox, Tab::TerrainEditor, Tab::AnimationEditor] {
        if !native_tab_enabled(tab, choices) {
            panels.take(dock, &tab);
        }
    }
}

impl RustudioApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        open: Option<PathBuf>,
        harness: Harness,
        demo: bool,
    ) -> Self {
        // (whether the cursor can truly lock: mouse_lock::platform_locks)
        use eframe::egui_wgpu::wgpu::rwh::{HasWindowHandle, RawWindowHandle};
        let wayland = cc.window_handle().ok().map(|h| matches!(h.as_raw(), RawWindowHandle::Wayland(_)));
        let mut app = Self::new_in(&cc.egui_ctx, cc.wgpu_render_state.clone(), wayland, open, harness, demo);
        app.ed.plugins.set_repaint_context(cc.egui_ctx.clone());
        rustudio_core::load_trace::mark("app created");
        app
    }

    /// The app on an egui context and (optional) wgpu device of its own:
    /// eframe's window (`new`) or the offscreen runner (src/offscreen.rs,
    /// `--headless`, no window; `--no-render`, no GPU).
    pub fn new_in(
        egui_ctx: &egui::Context,
        render_state: Option<egui_wgpu::RenderState>,
        wayland: Option<bool>,
        open: Option<PathBuf>,
        harness: Harness,
        demo: bool,
    ) -> Self {
        crate::studio_hooks::install();
        rustudio_engine::ti::install(egui_ctx);
        theme::apply(egui_ctx);
        rustudio_engine::mouse_lock::note_window(wayland);
        let builtin_tools = rustudio_core::rustudio_settings::BuiltinToolChoices::from_settings();
        let initial_tool = if builtin_tools.use_roblox(rustudio_core::rustudio_settings::BuiltinTool::Draggers) {
            Tool::None
        } else {
            Tool::Select
        };
        let mut ed = Editor {
            game: Game::new(render_state),
            dialogs: crate::file_dialog::Dialogs::new(egui_ctx.clone()),
            tool: initial_tool,
            collisions: mech::COLLISIONS_ON_BY_DEFAULT,
            join_surfaces: mech::JOIN_SURFACES_ON_BY_DEFAULT,
            physical_drag: mech::PHYSICAL_DRAG_BY_DEFAULT,
            part_snap: mech::PART_SNAP_ON_BY_DEFAULT,
            use_bounding_boxes: mech::USE_BOUNDING_BOXES_BY_DEFAULT,
            align_dragged: mech::ALIGN_DRAGGED_BY_DEFAULT,
            show_grid: true,
            clean_view: false,
            clipboard: None,
            hover: None,
            hover_pick: Default::default(),
            drag: None,
            marquee: None,
            marquee_base: Vec::new(),
            marquee_centres: Default::default(),
            output_view: Default::default(),
            part_drag: None,
            explorer: ExplorerState::default(),
            props: PropertiesState::default(),
            panel_cache: Default::default(),
            chars: Default::default(),
            cloud: crate::cloud_ui::CloudUi::new(),
            account_destination: None,
            open_script: Vec::new(),
            close_script: Vec::new(),
            document_tabs_reset: false,
            scripts: Default::default(),
            plugins: Default::default(),
            builtin_tools,
            plugin_ui: Default::default(),
            plugin_bar: Default::default(),
            #[cfg(unix)]
            control: None,
            mcp: Default::default(),
            file_sync: Default::default(),
            terrain_ed: Default::default(),
            anim_editor: Default::default(),
            toolbox: Default::default(),
            asset_manager: Default::default(),
            profiler: None,
            toolbox_vf: Default::default(),
            fly_held: 0.0,
            fly_idle: 0.0,
            last_zoom: None,
            hud: HudStats::default(),
            ui_rects: HashMap::new(),
            gizmo_points: Vec::new(),
            outline_cache: Default::default(),
            hover_outline_refs: (None, Vec::new()),
            selector_cache: Default::default(),
            selection_info: Default::default(),
            keyboard_cover: 0.0,
            view_pan: 0.0,
            harness,
            last_frame: Instant::now(),
            fps: 0.0,
            confirm_discard: None,
            last_selection: Vec::new(),
            auto_zoom: false,
            last_work_ms: 0.0,
            stop_pressed: false,
            full_pressed: false,
            play_full_exit: false,
            project: None,
            project_choice: None,
            project_export: None,
            project_settings: None,
            plugin_store: crate::plugin_store::manager::PluginManager::new(
                crate::plugin_store::Store::open(crate::plugin_store::default_root(), crate::plugin_store::default_plugins_dir(), 0),
                Box::new(crate::plugin_store::runtime::HostBridge::default()),
            ),
            ui: crate::studio_ui::UiState::default(),
            pending_user_shot: None,
            start: Default::default(),
            switch_to_viewport: false,
        };
        ed.info(format!(
            "rustudio {} ready. Reflection database has {} classes.",
            env!("CARGO_PKG_VERSION"),
            doc::db().classes.len()
        ));
        {
            use rustudio_core::studio_target as target;
            let content = rustudio_core::default_scripts::studio_dir()
                .and_then(|root| rustudio_core::studio_content::content_deployment(&root));
            ed.info(format!(
                "Targets Studio {} ({}); Studio content: {}",
                target::VERSION,
                target::DEPLOYMENT,
                match content.as_deref() {
                    Some(d) if d == target::DEPLOYMENT => "matches".to_string(),
                    Some(d) => format!("{d} (does not match)"),
                    None => "not installed or unknown version".to_string(),
                },
            ));
        }
        let reports = rustudio_core::crash::unseen_reports();
        for line in &reports {
            // (and in logcat, where a device run is read)
            #[cfg(target_os = "android")]
            log::error!("{line}");
            ed.error(line.clone());
        }
        // On a phone the report's folder is closed to the Files apps
        // (android/logs.rs): the report and the logs go to Download.
        #[cfg(target_os = "android")]
        if !reports.is_empty() && ed.harness.run.log_out.is_none() {
            match crate::android::logs::export("after a crash") {
                Ok(path) => ed.error(format!("The crash report and the logs were copied to {path} (a Files app: Download > rustudio); please send that file with a bug report")),
                Err(e) => ed.error(format!("The logs couldn't be copied to Download: {e}")),
            }
        }
        // A place the activity was started for but left alone (the last
        // start ended while opening it: android/storage.rs).
        #[cfg(target_os = "android")]
        for line in crate::android::storage::take_notices() {
            ed.error(line);
        }
        if ed.game.renderer.is_none() && !ed.harness.run.no_render {
            ed.error("No wgpu renderer available; the 3D viewport is disabled.");
        }
        let open_none = open.is_none();
        if let Some(path) = open {
            ed.open_path(path);
        } else if demo {
            ed.build_demo();
        }
        if !ed.harness.run.no_ui {
            ed.explorer.expand_defaults(&ed.game.doc);
        }
        // No UI: the scene alone (the comparison captures' clean view)
        // unless --overlays keeps the editor's overlays.
        if ed.harness.run.no_ui && !ed.harness.overlays {
            ed.clean_view = true;
        }
        ed.ui.show_core_gui = ed.harness.run.show_core_gui;
        ed.game.show_core_gui = ed.harness.run.show_core_gui;
        if ed.harness.run.show_core_gui_explicit {
            // A flag for this run only: the layout keeps the default (or,
            // below, what it had saved).
            ed.ui.show_core_gui_saved = Some(true);
        }
        if ed.harness.selftest.is_some() {
            // Deterministic play-session timing for the scripted test.
            ed.game.fixed_dt = Some(1.0 / 60.0);
        }
        // RUSTUDIO_FIXED_DT=HZ (tests only): Play steps 1/HZ s a frame however
        // long frames take (with RUSTUDIO_GAME_CLOCK, probes timed by os.clock
        // see an unloaded machine).
        if let Some(hz) = std::env::var("RUSTUDIO_FIXED_DT").ok().and_then(|v| v.parse::<f32>().ok()).filter(|h| *h > 0.0) {
            ed.game.fixed_dt = Some(1.0 / hz);
        }
        // --fixed-dt (crates/rustudio-core/src/sim_clock.rs): the same fixed step, every clock on it.
        if let Some(dt) = rustudio_core::sim_clock::dt() {
            ed.game.fixed_dt = Some(dt);
        }
        if ((ed.harness.selftest.is_none() && ed.harness.run.bench.is_none())
            || std::env::var_os("RUSTUDIO_SESSION").is_some())
            && std::env::var_os("RUSTUDIO_NO_SESSION").is_none()
        {
            // The session (login keyring) is checked in the background, for
            // real playthroughs too (--screenshot): asset downloads and the
            // avatar. LoadAsset at Play start waits for the check. The
            // self-test and --bench opt in with RUSTUDIO_SESSION=1;
            // RUSTUDIO_NO_SESSION=1 keeps any run signed out.
            ed.game.account.restore();
        }
        if let Some((u, p)) = ed.harness.team_create { ed.open_team_create(u, p); }
        ed.cloud.open_when_ready = ed.harness.open_roblox.map(|(p, copy)| (p, None, copy));
        ed.cloud.picker_when_ready = ed.harness.roblox_picker;
        ed.cloud.settings_open = ed.harness.roblox_settings;
        #[cfg(unix)]
        if let Ok(path) = std::env::var("RUSTUDIO_CONTROL") {
            match crate::control::Control::listen(&path) {
                Ok(c) => {
                    ed.control = Some(c);
                    ed.clean_view = true;
                }
                Err(e) => eprintln!("[control] can't listen on {path}: {e}"),
            }
        }
        if let Some([x, y, z, yaw, pitch]) = ed.harness.run.camera {
            ed.game.cam.pos = Vec3::new(x, y, z);
            ed.game.cam.yaw = yaw.to_radians();
            ed.game.cam.pitch = pitch.to_radians();
            ed.game.cam.focus = ed.game.cam.pos + ed.game.cam.forward() * 50.0;
        } else if ed.harness.run.bench.as_ref().is_some_and(|b| !b.play) {
            // Worst case for drawing: the whole place in view.
            ed.frame_all();
        }
        // Nothing to open: the Start Page, as Studio starts. Test and harness
        // runs (and RUSTUDIO_NO_START_PAGE=1) go straight to the editor.
        let plain = ed.harness.plain();
        // RUSTUDIO_START_PAGE=1 forces it (screenshots of the page itself).
        let forced = std::env::var_os("RUSTUDIO_START_PAGE").is_some();
        if !ed.harness.run.no_ui && (forced || (open_none && !demo && plain && std::env::var_os("RUSTUDIO_NO_START_PAGE").is_none())) {
            ed.show_start_page();
            let page = std::env::var("RUSTUDIO_START_PAGE").unwrap_or_default();
            ed.start.open_page(&page);
        }
        // The last session's layout and view settings (plain runs only).
        let mut dock = default_dock_for(ed.builtin_tools);
        let mut layout = None;
        let mut panels = crate::panels::Panels::default();
        if !ed.harness.run.no_ui && crate::layout_store::enabled(&ed.harness) {
            let saved = crate::layout_store::load();
            if let Some(l) = &saved {
                l.apply(&mut ed);
                if ed.harness.run.show_core_gui_explicit {
                    ed.ui.show_core_gui_saved = Some(ed.ui.show_core_gui);
                    ed.ui.show_core_gui = ed.harness.run.show_core_gui;
                }
                if let Some(d) = l.dock() {
                    dock = d;
                }
                panels = l.panels(&dock);
            }
            layout = Some(crate::layout_store::Saver::new(saved.as_ref()));
            // (plugin widgets' restore state beside layout.json)
            crate::plugin_host::widgets_ui::set_persist_dir(Some(rustudio_core::paths::config().join("rustudio")));
        }
        ed.game.show_core_gui = ed.ui.show_core_gui;
        strip_disabled_native_tabs(&mut dock, &mut panels, ed.builtin_tools);
        // (a GPU validation error is a logcat line, not the end of the
        // process: src/android/mod.rs install_gpu_error_handler. Set after
        // Game::new, so it replaces gpu_health::watch's handlers — the
        // Output warning and the lost-device panic a desktop gets.)
        #[cfg(target_os = "android")]
        if let Some(rs) = ed.game.render_state.as_ref() {
            crate::android::install_gpu_error_handler(&rs.device);
        }
        // Comparison captures (src/control.rs) want the 3D view as big as the
        // other program's: the view alone, no Explorer / Properties / Output.
        #[cfg(unix)]
        if ed.control.is_some() && layout.is_none() {
            dock = DockState::new(vec![Tab::Viewport]);
        }
        // (--no-ui never shows the dock.)
        if ed.harness.run.no_ui {
            dock = DockState::new(vec![Tab::Viewport]);
        }
        // `--play` starts once the 3D view has its size (Camera.ViewportSize).
        RustudioApp {
            watchdog: crate::frame_watchdog::Watchdog::new(egui_ctx.clone()),
            updater: crate::update::Updater::start(),
            last_ui_frame: None,
            last_plugin_tick: Instant::now(),
            plugins_ticked_between_frames: false,
            egui_ctx: None,
            last_hidden_tick: Instant::now(),
            dock,
            ed,
            layout,
            panels,
            #[cfg(target_os = "android")]
            keyboard: Default::default(),
            #[cfg(target_os = "android")]
            cutout: Default::default(),
            #[cfg(target_os = "android")]
            text_menu: Default::default(),
            #[cfg(target_os = "android")]
            text_bar: Default::default(),
            refresh_hz: display_refresh_hz(),
            cpu_usage: None,
            pace_next: None,
            paced_last: 0.0,
            rate_voted: None,
            perf_report: match std::env::var("RUSTUDIO_PERF_LOG").ok().as_deref() {
                Some("0") | Some("") => None,
                Some(_) => Some(Default::default()),
                None => cfg!(target_os = "android").then(Default::default),
            },
        }
    }
}

impl RustudioApp {
    /// Android: Performance > Editor frame limit (60 on a phone by default,
    /// as Roblox's client: rustudio_settings `choice_default_for`). The
    /// limit is first asked of the panel (src/android/frame_rate.rs: a
    /// 60/120 Hz phone on an adaptive setting switches to 60, and vsync
    /// paces the frames); where the panel stays faster, the rest of the
    /// period is slept here before the frame — the desktop's pacing is
    /// native_loop.rs's, which the activity's eframe loop hasn't. Nothing
    /// when the limit is at or over the display's rate (a 60 Hz phone at
    /// the default never sleeps). Returns the seconds slept (left out of
    /// the governor's hint).
    #[cfg(target_os = "android")]
    fn pace_frame(&mut self) -> f32 {
        let limit = rustudio_core::rustudio_settings::fps_limit();
        // The vote, when the limit changes and every few seconds (the
        // window is a new one after a pause); the panel's rate read again
        // on the same clock, since the vote may have changed it.
        let want = limit.unwrap_or(0.0);
        if self.rate_voted.is_none_or(|(hz, at)| hz != want || at.elapsed().as_secs() >= 3) {
            if self.rate_voted.is_none_or(|(hz, _)| hz != want) {
                match crate::android::frame_rate::vote(want) {
                    Some(true) if want > 0.0 => log::info!("display: {want:.0} Hz asked for the frame limit"),
                    Some(true) => log::info!("display: any rate (no frame limit)"),
                    Some(false) => log::info!("display: the frame rate vote was refused"),
                    None => {}
                }
            } else {
                let _ = crate::android::frame_rate::vote(want);
            }
            self.rate_voted = Some((want, Instant::now()));
            if let Ok(hz) = crate::android::jni::display_refresh_rate() {
                if hz > 1.0 && (hz - self.refresh_hz).abs() > 0.5 {
                    log::info!("display: {hz:.0} Hz");
                    self.refresh_hz = hz;
                }
            }
        }
        let Some(cap) = limit else {
            self.pace_next = None;
            return 0.0;
        };
        if self.refresh_hz > 1.0 && cap >= self.refresh_hz * 0.98 {
            self.pace_next = None;
            return 0.0;
        }
        let period = std::time::Duration::from_secs_f32(1.0 / cap.max(1.0));
        let now = Instant::now();
        let mut slept = 0.0;
        let next = match self.pace_next {
            Some(next) if next > now => {
                std::thread::sleep(next - now);
                slept = now.elapsed().as_secs_f32();
                next + period
            }
            // (a little late: the cadence holds; far behind, or the first
            // frame: it restarts from now, no catching up in a burst)
            Some(next) if now.duration_since(next) < period => next + period,
            _ => now + period,
        };
        self.pace_next = Some(next);
        slept
    }
}

/// The display's refresh rate (Android asks the Display; 0 elsewhere:
/// unknown, taken as 60 where a budget is needed).
fn display_refresh_hz() -> f32 {
    #[cfg(target_os = "android")]
    {
        match crate::android::jni::display_refresh_rate() {
            Ok(hz) if hz > 1.0 => {
                log::info!("display: {hz:.0} Hz");
                hz
            }
            Ok(_) => 0.0,
            Err(e) => {
                log::info!("display refresh rate unknown: {e}");
                0.0
            }
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        0.0
    }
}

// ------------------------------------------------------------------ logging

impl Editor {
    pub fn push_log(&mut self, level: Level, text: impl Into<String>) {
        let text = text.into();
        match level {
            Level::Info | Level::System => log::info!(target: "rustudio::editor", "{text}"),
            Level::Warn => log::warn!(target: "rustudio::editor", "{text}"),
            Level::Error => log::error!(target: "rustudio::editor", "{text}"),
        }
        self.game.push_log(level, text);
        self.flush_output_changes();
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.push_log(Level::Info, text);
    }

    /// Studio's purple `MESSAGE_SYSTEM` Output line (save / publish / system notices).
    pub fn system(&mut self, text: impl Into<String>) {
        self.push_log(Level::System, text);
    }

    pub(crate) fn flush_output_changes(&mut self) {
        if std::mem::take(&mut self.game.log_cleared) {
            self.output_view.log_cleared();
        }
        let drained = std::mem::take(&mut self.game.log_drained);
        if drained != 0 {
            self.output_view.log_drained(drained);
        }
    }

    /// The plugin store's frame: startup loads, folder watching, Roblox jobs;
    /// its lines go to Output.
    pub fn plugins_tick(&mut self) {
        let client = self.game.account.shared_client();
        let host_on = self.plugins.enabled();
        if let Some(b) = self.plugin_store.runtime.bridge() {
            b.host_enabled = host_on;
        }
        self.plugin_store.tick(client, self.game.frame_no);
        // The store's loads / unloads / reloads, applied to the host.
        let cmds = self.plugin_store.runtime.bridge().map(|b| std::mem::take(&mut b.cmds)).unwrap_or_default();
        for cmd in cmds {
            use crate::plugin_store::runtime::{host_source, RtCmd};
            match cmd {
                RtCmd::Load(src) | RtCmd::Reload(src) => {
                    let known = self.plugin_store.runtime.bridge().and_then(|b| b.ids.get(&src.id).copied());
                    let host_path = |h: &crate::plugin_host::PluginHost, pid| {
                        h.loaded().into_iter().find(|i| i.id == pid).and_then(|i| i.source.path().map(|p| p.to_path_buf()))
                    };
                    let r = match known {
                        // Same file (an edit): the host reads it again. (A
                        // subfolder .lua is an in-memory tree: no path, so
                        // it is loaded anew below.)
                        Some(pid) if host_path(&self.plugins, pid).as_deref() == Some(src.path.as_path()) => {
                            self.plugins.reload(pid);
                            Ok(pid)
                        }
                        // Another file (an update) or an in-memory tree: unload, load anew.
                        Some(pid) => {
                            self.plugins.unload(pid);
                            host_source(&src).and_then(|h| self.plugins.load(h))
                        }
                        None => host_source(&src).and_then(|h| self.plugins.load(h)),
                    };
                    let b = self.plugin_store.runtime.bridge().unwrap();
                    match r {
                        Ok(pid) => {
                            b.ids.insert(src.id.clone(), pid);
                        }
                        Err(e) => {
                            b.ids.remove(&src.id);
                            b.loaded.retain(|l| *l != src.id);
                            self.push_log(Level::Error, format!("Plugin {} failed to load: {e}", src.id));
                        }
                    }
                }
                RtCmd::Unload(id) => {
                    if let Some(pid) = self.plugin_store.runtime.bridge().and_then(|b| b.ids.remove(&id)) {
                        self.plugins.unload(pid);
                    }
                }
            }
        }
        for l in std::mem::take(&mut self.plugin_store.lines) {
            let level = match l.level {
                crate::plugin_store::watcher::Level::Info => Level::Info,
                crate::plugin_store::watcher::Level::Warn => Level::Warn,
                crate::plugin_store::watcher::Level::Error => Level::Error,
                crate::plugin_store::watcher::Level::System => Level::System,
            };
            self.push_log(level, l.text);
        }
    }

    /// Manage Plugins, permission prompts, Publish as Plugin.
    pub fn plugins_ui(&mut self, ctx: &egui::Context) {
        let client = self.game.account.shared_client();
        crate::plugin_store::ui::show(ctx, &mut self.plugin_store, client);
        // Plugin menus (ShowAsync), Customize Shortcuts, bound plugin keys.
        self.plugin_popups(ctx);
        self.customize_shortcuts_window(ctx);
        self.plugin_shortcut_keys(ctx);
        // Dialogs plugins opened (src/plugin_host/prompts.rs).
        crate::plugin_host::prompts::ui(ctx, &self.game.doc);
    }
    pub fn warn(&mut self, t: impl Into<String>) {
        self.push_log(Level::Warn, t)
    }
    pub fn error(&mut self, t: impl Into<String>) {
        self.push_log(Level::Error, t)
    }
}

// ------------------------------------------------------------------ actions

impl Editor {
    /// A Roblox TextBox has the keyboard: the game's in Play, or a plugin
    /// GUI's (a widget's, CoreGui's). The soft keyboard comes up for it
    /// (src/android/keyboard.rs).
    #[cfg(target_os = "android")]
    pub fn textbox_focused(&self) -> bool {
        (self.game.play.is_some() && self.game.gui.focus.is_some()) || self.plugin_ui.has_text_focus()
    }

    pub fn set_document(&mut self, doc: Document) {
        self.reset_cloud_save_dialogs();
        // Another document closes the open project (open_project sets it again).
        self.project = None;
        crate::project_config::set_active(None);
        self.document_tabs_reset = true;
        self.ui.insert_popup = None;
        self.last_selection.clear();
        self.marquee_base.clear();
        self.ui.dock_moves.clear();
        self.ui.panel_requests.retain(|(tab, _)| !matches!(tab, Tab::Script(_)));
        self.game.doc = doc;
        // The scripts compile for Play in the background from here
        // (bytecode_cache::precompile): a first Play then starts as a second
        // one does, finding its bytecode ready.
        rustudio_engine::bytecode_cache::precompile(
            self.game
                .doc
                .dom
                .descendants()
                .filter(|i| matches!(i.class.as_str(), "Script" | "LocalScript" | "ModuleScript"))
                .filter_map(|i| match i.properties.get(&rbx_dom_weak::ustr("Source")) {
                    Some(rbx_dom_weak::types::Variant::String(s)) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
        );
        self.game.edit_audio.reset();
        self.game.scene = Scene::default();
        self.drag = None;
        self.hover = None;
        self.open_script.clear();
        self.scripts = Default::default();
        self.explorer = ExplorerState::default();
        self.explorer.expand_defaults(&self.game.doc);
        if !self.game.use_saved_camera() {
            self.frame_all();
        }
        // Plugins run in the place that's open: the
        // old place's go, the new place gets them all.
        self.plugin_store.on_place_opened();
        self.builtin_tools = rustudio_core::rustudio_settings::BuiltinToolChoices::from_settings();
        if self.builtin_tools.use_roblox(rustudio_core::rustudio_settings::BuiltinTool::Draggers) {
            if matches!(self.tool, Tool::Select | Tool::Move | Tool::Scale | Tool::Rotate) {
                self.tool = Tool::None;
            }
        } else if self.tool == Tool::None {
            self.tool = Tool::Select;
        }
        if !self.builtin_tools.use_native(rustudio_core::rustudio_settings::BuiltinTool::RigBuilder) {
            self.chars.open = false;
        }
        if !self.builtin_tools.use_native(rustudio_core::rustudio_settings::BuiltinTool::TerrainEditor) {
            self.terrain_ed.tool = None;
        }
        if self.harness.plugins_started && !self.harness.no_plugins {
            self.plugins.replace_builtins(self.builtin_tools);
            if self.builtin_tools.use_roblox(rustudio_core::rustudio_settings::BuiltinTool::ViewSelector) {
                self.plugins.wake_plugin_by_name("ViewSelector");
            }
        }
    }


    pub fn open_path(&mut self, path: PathBuf) {
        // (the start counts as settled 20 s after this: android/storage.rs)
        #[cfg(target_os = "android")]
        crate::android::storage::opening(&path);
        // File > Recent remembers what a person opens (not test runs).
        let harness = self.harness.run.screenshot.is_some()
            || self.harness.selftest.is_some()
            || self.harness.run.bench.is_some()
            || self.harness.ui_shots.is_some();
        if !harness && path.exists() {
            crate::studio_ui::remember_recent(&mut self.ui, &path);
        }
        // A copy downloaded from Roblox (places/<id>/origin.json beside it):
        // assets are asked for in that place's context, as when opened from
        // Roblox. RUSTUDIO_PLACE_CONTEXT (harness) wins.
        if std::env::var_os("RUSTUDIO_PLACE_CONTEXT").is_none() {
            let origin = path.parent().map(|d| d.join("origin.json"));
            let place = origin
                .and_then(|o| std::fs::read_to_string(o).ok())
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .and_then(|v| v.get("place_id")?.as_u64())
                .unwrap_or(0);
            rustudio_account::session::set_place_context(place);
        }
        // A project folder, or a project file itself.
        if path.is_dir() {
            return self.open_folder(path, false);
        }
        if crate::project::name_of(&path).ends_with(".project.json") {
            return self.open_project(&path);
        }
        match Document::open(&path) {
            Ok(d) => {
                let n = d.dom.descendants().count();
                rustudio_core::load_trace::mark("document built");
                self.set_document(d);
                rustudio_core::load_trace::mark(&format!("opened ({n} instances)"));
                self.info(format!("Opened {} ({n} instances)", path.display()));
            }
            Err(e) => self.error(format!("Could not open {}: {e}", path.display())),
        }
    }

    /// A project folder: its project file (asks which one when there are
    /// several and `ask`, else default.project.json or the first).
    pub fn open_folder(&mut self, dir: PathBuf, ask: bool) {
        let files = crate::project::project_files(&dir);
        match files.len() {
            0 => self.error(format!("{} has no *.project.json", dir.display())),
            1 => self.open_project(&files[0].clone()),
            _ if ask => self.project_choice = Some((dir, files)),
            _ => self.open_project(&files[0].clone()),
        }
    }

    pub fn open_project(&mut self, file: &std::path::Path) {
        match crate::project_sync::open(file) {
            Ok((doc, mut st)) => {
                let n = doc.dom.descendants().count();
                let warnings = std::mem::take(&mut st.warnings);
                let place = st
                    .place
                    .as_ref()
                    .map(|p| format!(", place {}", p.display()))
                    .unwrap_or_else(|| ", no place file (an empty DataModel under it)".into());
                let synced = st.map.len();
                self.set_document(doc);
                crate::project_config::set_active(Some(st.config.clone()));
                self.project = Some(st);
                self.info(format!(
                    "Opened project \"{}\" from {} ({n} instances, {synced} from files{place})",
                    self.project.as_ref().map(|p| p.name.as_str()).unwrap_or(""),
                    file.display()
                ));
                for w in warnings.iter().take(20) {
                    self.warn(format!("Project: {w}"));
                }
                if warnings.len() > 20 {
                    self.warn(format!("Project: {} more warnings", warnings.len() - 20));
                }
            }
            Err(e) => self.error(format!("Could not open project {}: {e}", file.display())),
        }
    }

    /// Once a frame: edits to synced instances go to their files, and files
    /// changed on disk come into the place (in edit mode; during Play they
    /// wait until Play stops).
    pub fn project_tick(&mut self) {
        let Some(mut st) = self.project.take() else { return };
        let playing = self.game.play.is_some();
        if !playing && self.game.doc.id == st.doc_id {
            st.write_back(&mut self.game.doc);
        }
        if let Some(changed) = st.poll_disk() {
            if playing {
                st.reapply_after_play = true;
            } else if self.game.doc.id == st.doc_id {
                let n = st.reconcile(&mut self.game.doc);
                if n > 0 {
                    st.sourcemap_dirty = true;
                    let mut names: Vec<String> = changed
                        .iter()
                        .filter_map(|p| p.strip_prefix(&st.root).ok().map(|x| x.display().to_string()))
                        .collect();
                    names.sort();
                    names.truncate(4);
                    self.info(format!("Project: {n} instance(s) updated from disk ({})", names.join(", ")));
                }
            }
        }
        if !playing && st.reapply_after_play && self.game.doc.id == st.doc_id {
            st.reapply_after_play = false;
            let n = st.reconcile(&mut self.game.doc);
            self.info(format!("Project: {n} instance(s) updated from files changed during Play"));
        }
        if !st.log.is_empty() {
            st.sourcemap_dirty = true;
        }
        if st.sourcemap_dirty && !playing && self.game.doc.id == st.doc_id {
            st.update_sourcemap(&self.game.doc);
        }
        for l in std::mem::take(&mut st.log) {
            self.info(format!("Project: {l}"));
        }
        for w in std::mem::take(&mut st.warnings) {
            self.warn(format!("Project: {w}"));
        }
        self.project = Some(st);
    }

    /// Ctrl+S with a project: synced instances are already in their files;
    /// the place file is written only when something outside the mapped
    /// paths changed (so an untouched project saves nothing).
    pub(crate) fn save_project(&mut self) {
        if self.refuse_during_play("save") {
            return;
        }
        let Some(mut st) = self.project.take() else { return };
        if self.game.play.is_none() {
            st.write_back(&mut self.game.doc);
        }
        let dirty = st.place_dirty;
        let place = st.place.clone();
        self.project = Some(st);
        match (dirty, place) {
            (false, _) => self.system("No changes to the place detected. Did not save."),
            (true, Some(p)) => {
                self.save_to(p);
                if let Some(st) = self.project.as_mut() {
                    st.place_dirty = false;
                }
            }
            (true, None) => self.save_as(),
        }
    }

    pub fn open_folder_dialog(&mut self) {
        use crate::file_dialog::{Kind, Purpose};
        self.dialogs.start(Purpose::OpenFolder, crate::file_dialog::Request::new(), Kind::PickFolder);
    }

    /// A file dialog closed: act on its answer.
    pub fn poll_file_dialogs(&mut self) {
        use crate::file_dialog::Purpose;
        if let Some((purpose, paths)) = self.dialogs.poll_multi() {
            if !paths.is_empty() && purpose == Purpose::AssetManagerImport {
                self.asset_manager_import_files(paths);
            }
        }
        let Some((purpose, path)) = self.dialogs.poll() else {
            return;
        };
        let Some(path) = path else {
            return;
        };
        match purpose {
            Purpose::OpenPlace => self.open_path(path),
            Purpose::OpenFolder => self.open_folder(path, true),
            Purpose::SaveAs => self.save_to(path),
            Purpose::InsertModel => self.insert_model_file(path),
            Purpose::SaveSelection(sel) => match self.game.doc.save_selection_to(&sel, &path) {
                Ok(()) => self.system(format!("Saved to '{}'", path.display())),
                Err(e) => self.error(format!("Save failed: {e}")),
            },
            Purpose::SaveLocalPlugin(sel) => {
                self.game.doc.selection = sel;
                self.save_local_plugin_to(path);
            }
            Purpose::TerrainImage { height } => self.load_import_image(path, height),
            Purpose::AnimImportFbx | Purpose::AnimImportRbxm | Purpose::AnimExportRbxm => {
                self.anim_editor_handle_file(&purpose, path)
            }
            Purpose::FileSyncProject => self.file_sync.pick = path.display().to_string(),
            Purpose::ProjectParent => {
                if let Some(d) = self.project_export.as_mut() {
                    d.parent = path.display().to_string();
                }
            }
            Purpose::AssetManagerImport => self.asset_manager_import_files(vec![path]),
            Purpose::TemplateAdd | Purpose::TemplateThumb(_) | Purpose::TemplateReplace(_) => self.start_template_file(&purpose, path),
        }
    }

    /// File > New / Ctrl+N: Studio's Baseplate template as Studio opens it
    /// (start_page/templates.rs), its editor camera at the template's saved
    /// Camera.
    /// While Play or Run is going the shown document is the running game's
    /// copy, not the place: saving would write the game's state (anything a
    /// script made) over the place file, and New / Open would replace it.
    /// Studio disables these during a playtest; refuse them here so every
    /// route (menus, shortcuts, MCP, plugins) is covered.
    fn refuse_during_play(&mut self, what: &str) -> bool {
        if self.game.is_playing() {
            self.warn(format!("Stop the playtest before you {what}."));
            return true;
        }
        false
    }

    pub fn new_place(&mut self) {
        if self.refuse_during_play("create a new place") {
            return;
        }
        self.set_document(crate::start_page::templates::baseplate());
        self.info("New place created");
    }

    pub fn open_dialog(&mut self) {
        if self.refuse_during_play("open a place") {
            return;
        }
        use crate::file_dialog::{Kind, Purpose};
        let d = crate::file_dialog::Request::new().add_filter("Roblox place or model", &["rbxl", "rbxlx", "rbxm", "rbxmx"]);
        self.dialogs.start(Purpose::OpenPlace, d, Kind::PickFile);
    }

    pub fn save(&mut self) {
        if self.refuse_during_play("save") {
            return;
        }
        if self.project.is_some() {
            return self.save_project();
        }
        // A local copy of a Team Create place saves to a file.
        if self.game.doc.cloud.as_ref().is_some_and(|o| o.local_copy) {
            return match self.game.doc.path.clone() {
                Some(p) => self.save_to(p),
                None => self.save_as(),
            };
        }
        // A place opened from Roblox: Ctrl+S is Save to Roblox, as in Studio.
        if self.game.doc.cloud.is_some() || self.live_team_create_ids().is_some() {
            return self.save_cloud(rustudio_account::VersionType::Saved, false);
        }
        match self.game.doc.path.clone() {
            Some(p) => self.save_to(p),
            None => self.save_as(),
        }
    }

    pub fn save_as(&mut self) {
        if self.refuse_during_play("save") {
            return;
        }
        let (name, exts): (&str, &[&str]) = match self.game.doc.kind {
            DocKind::Place => ("Roblox place", &["rbxl", "rbxlx"]),
            DocKind::Model => ("Roblox model", &["rbxm", "rbxmx"]),
        };
        let default = match self.game.doc.kind {
            DocKind::Place => "place.rbxl",
            DocKind::Model => "model.rbxm",
        };
        use crate::file_dialog::{Kind, Purpose};
        let d = crate::file_dialog::Request::new().add_filter(name, exts).set_file_name(default);
        self.dialogs.start(Purpose::SaveAs, d, Kind::SaveFile);
    }

    pub fn save_to(&mut self, p: PathBuf) {
        if self.refuse_during_play("save") {
            return;
        }
        match self.game.doc.save_to(&p) {
            Ok(()) => self.system(format!("Saved to '{}'", p.display())),
            Err(e) => self.error(format!("Save failed: {e}")),
        }
    }

    pub fn insert_from_file(&mut self) {
        use crate::file_dialog::{Kind, Purpose};
        let d = crate::file_dialog::Request::new().add_filter("Roblox model", &["rbxm", "rbxmx"]);
        self.dialogs.start(Purpose::InsertModel, d, Kind::PickFile);
    }

    pub(crate) fn insert_model_file(&mut self, path: PathBuf) {
        match std::fs::read(&path) {
            Ok(bytes) => {
                let parent = self.paste_target(false);
                let refs = self.game.doc.paste(&bytes, parent, "Insert from file");
                self.info(format!(
                    "Inserted {} object(s) from {}",
                    refs.len(),
                    path.display()
                ));
                self.select(refs);
            }
            Err(e) => self.error(format!("{}: {e}", path.display())),
        }
    }

    pub fn save_selection(&mut self) {
        if self.game.doc.selection.is_empty() {
            self.warn("Select something to save as a model");
            return;
        }
        use crate::file_dialog::{Kind, Purpose};
        let d = crate::file_dialog::Request::new().add_filter("Roblox model", &["rbxm", "rbxmx"]).set_file_name("model.rbxm");
        self.dialogs.start(Purpose::SaveSelection(self.game.doc.selection.clone()), d, Kind::SaveFile);
    }

    pub fn select(&mut self, refs: Vec<Ref>) {
        self.game.doc.selection = refs;
    }

    /// Whenever the selection changes (viewport click, undo/redo, paste,
    /// Explorer…), expand the ancestors of every selected object and scroll
    /// the newest one into view.
    fn sync_explorer_to_selection(&mut self) {
        if self.game.doc.selection == self.last_selection {
            return;
        }
        if std::env::var_os("RUSTUDIO_PICK_TRACE").is_some() {
            eprintln!("[pick selection] {:?}", self.game.doc.selection.iter().map(|r| self.game.doc.dom.get_by_ref(*r).map(|i| (&i.name, &i.class))).collect::<Vec<_>>());
        }
        let last_added = self.game.doc.selection.iter().rev().find(|r| !self.last_selection.contains(r));
        for &r in &self.game.doc.selection {
            self.explorer.expand_ancestors(&self.game.doc, r);
        }
        if let Some(&r) = last_added.or(self.game.doc.selection.last()) {
            self.explorer.scroll_to = Some(r);
        }
        self.last_selection = self.game.doc.selection.clone();
    }

    pub fn undo(&mut self) {
        match self.game.doc.undo() {
            Some(l) => self.info(format!("Undo: {l}")),
            None => self.warn("Nothing to undo"),
        }
    }

    pub fn redo(&mut self) {
        match self.game.doc.redo() {
            Some(l) => self.info(format!("Redo: {l}")),
            None => self.warn("Nothing to redo"),
        }
    }

    pub fn copy(&mut self) -> bool {
        match self.game.doc.copy(&self.game.doc.selection.clone()) {
            Some(bytes) => {
                self.clipboard = Some(bytes);
                true
            }
            None => false,
        }
    }

    pub fn cut(&mut self) {
        if self.copy() {
            let sel = self.game.doc.selection.clone();
            self.game.doc.remove(&sel);
        }
    }

    /// Paste goes next to the selection (or into it with `into`), else Workspace.
    /// If the selected instance is a top-level Service (parent is DataModel root,
    /// e.g. StarterGui), paste into that service rather than directly under DataModel.
    fn paste_target(&self, into: bool) -> Ref {
        paste_target_for(&self.game.doc, into)
    }

    /// Whether `StarterGui.ShowDevelopmentGui` is enabled (defaults to true in Studio).
    /// Whether the edit view shows StarterGui. `StarterGui.ShowDevelopmentGui`
    /// decides it, as in Studio (a property of the place: saved with it and
    /// undoable); `ui.show_gui` is only an older editor-wide switch that the
    /// toggles below turn back on, so the two can't disagree.
    pub fn starter_gui_shown(&self) -> bool {
        self.ui.show_gui && starter_gui_shown_in(&self.game.doc)
    }

    /// View > UI and the Explorer eye: sets `StarterGui.ShowDevelopmentGui`
    /// (Studio's UI visibility toggle flips that property).
    pub fn set_starter_gui_shown(&mut self, shown: bool) {
        self.ui.show_gui = true;
        set_starter_gui_shown_in(&mut self.game.doc, shown);
    }

    pub fn paste(&mut self, into: bool) {
        let Some(bytes) = self.clipboard.clone() else {
            self.warn("Clipboard is empty");
            return;
        };
        let parent = self.paste_target(into);
        let refs = self.game.doc.paste(&bytes, parent, "Paste");
        self.select(refs);
    }

    pub fn duplicate(&mut self) {
        let sel = self.game.doc.selection.clone();
        // Commit the motion so far before Duplicate's own undo step. Keep the
        // pointer geometry, but transfer the active targets to the copies.
        let free = self.part_drag.clone();
        let handle = self.drag.clone();
        self.finish_part_drag();
        self.finish_drag();
        let roots: Vec<_> = self.game.doc.top_level(&sel).into_iter()
            .filter(|r| self.game.doc.can_edit_structure(*r)).collect();
        let refs = self.game.doc.duplicate(&sel);
        self.part_drag = free;
        self.drag = handle;
        if !refs.is_empty() {
            let mut map = std::collections::HashMap::new();
            let mut pending: Vec<_> = roots.into_iter().zip(refs.iter().copied()).collect();
            while let Some((old, new)) = pending.pop() {
                map.insert(old, new);
                if let (Some(a), Some(b)) = (self.game.doc.dom.get_by_ref(old), self.game.doc.dom.get_by_ref(new)) {
                    pending.extend(a.children().iter().copied().zip(b.children().iter().copied()));
                }
            }
            self.duplicate_part_drag(&map);
            self.duplicate_handle_drag(&map);
            self.select(refs);
        }
    }

    pub fn delete(&mut self) {
        let sel = self.game.doc.selection.clone();
        let n = self.game.doc.remove(&sel);
        if n == 0 && !sel.is_empty() {
            self.warn("Services and the root cannot be deleted");
        }
    }

    pub fn group(&mut self) {
        let sel = self.game.doc.selection.clone();
        if let Some(m) = self.game.doc.group(&sel) {
            self.select(vec![m]);
        }
    }

    pub fn ungroup(&mut self) {
        let sel = self.game.doc.selection.clone();
        let refs = self.game.doc.ungroup(&sel);
        if !refs.is_empty() {
            self.select(refs);
        }
    }

    /// Inserts a new part in front of the camera, on top of whatever is there.
    pub fn insert_part(&mut self, mesh: MeshKind) {
        let (class, shape) = match mesh {
            MeshKind::Block => ("Part", 1),
            MeshKind::Ball => ("Part", 0),
            MeshKind::Cylinder => ("Part", 2),
            MeshKind::Wedge => ("WedgePart", 1),
            MeshKind::CornerWedge => ("CornerWedgePart", 1),
        };
        let mut size = Vec3::from(mech::NEW_PART_SIZE);
        if mesh == MeshKind::Ball {
            size = Vec3::splat(2.0);
        } else if mesh == MeshKind::Cylinder {
            size = Vec3::new(2.0, 2.0, 2.0);
        }
        let pos = self.spawn_point(size);
        let mut b = doc::new_part(
            if class == "Part" { "Part" } else { class },
            shape,
            pos,
            size,
            mech::NEW_PART_COLOR,
        );
        if class != "Part" {
            b = b.with_class(class);
        }
        let parent = self.game.doc.default_parent();
        let r = self.game.doc.insert(parent, b);
        self.select(vec![r]);
    }

    /// Where a new part lands: the point under the screen centre, snapped.
    pub(crate) fn spawn_point(&mut self, size: Vec3) -> Vec3 {
        self.game.scene.sync(&mut self.game.doc);
        let rect = if self.game.viewport_rect.is_positive() {
            self.game.viewport_rect
        } else {
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))
        };
        let ray = self.game.cam.ray(rect, rect.center());
        let mut best: Option<f32> = None;
        for p in &self.game.scene.parts {
            if let Some(t) = ray.hit_obb(&p.xf, p.size) {
                if best.is_none_or(|b| t < b) {
                    best = Some(t);
                }
            }
        }
        let hit = match best {
            Some(t) if t < 500.0 => ray.at(t),
            _ => ray
                .hit_plane(Vec3::ZERO, Vec3::Y)
                .filter(|t| *t < 500.0)
                .map(|t| ray.at(t))
                .unwrap_or(self.game.cam.pos + self.game.cam.forward() * 20.0),
        };
        let snap = |v: f32| rustudio_core::math::snap(v, if self.game.snap_move { self.game.move_inc } else { 0.0 });
        Vec3::new(snap(hit.x), hit.y + size.y / 2.0, snap(hit.z))
    }

    pub fn insert_class(&mut self, class: &str, parent: Ref) {
        if class == "Part" {
            self.insert_part(MeshKind::Block);
            if parent != self.game.doc.default_parent() {
                let sel = self.game.doc.selection.clone();
                self.game.doc.reparent(&sel, parent);
            }
            return;
        }
        // Studio's Insert Object names it as its constructor does (a
        // SpecialMesh "Mesh", a StringValue "Value").
        let mut b = InstanceBuilder::new(class).with_name(doc::default_instance_name(class));
        if doc::is_a(class, "BasePart") {
            let size = Vec3::from(mech::NEW_PART_SIZE);
            let pos = self.spawn_point(size);
            b = b
                .with_property("Anchored", true)
                .with_property(
                    "Size",
                    rbx_dom_weak::types::Vector3::new(size.x, size.y, size.z),
                )
                .with_property(
                    "CFrame",
                    rbx_dom_weak::types::CFrame::new(
                        rustudio_core::math::rv3(pos),
                        rbx_dom_weak::types::Matrix3::identity(),
                    ),
                );
        }
        b = apply_insert_gui_defaults(class, b);
        if matches!(class, "Script" | "LocalScript" | "ModuleScript") {
            let src = if class == "ModuleScript" {
                "local module = {}\n\nreturn module\n"
            } else {
                "print(\"Hello world!\")\n"
            };
            b = b.with_property("Source", src.to_string());
        }
        let r = self.game.doc.insert(parent, b);
        self.select(vec![r]);
    }

    pub(crate) fn has_selected_parts(&self) -> bool {
        !self.selected_parts_shared().is_empty()
    }

    /// The selection info, brought up to date with the document, the scene
    /// and the selection first.
    fn selection_info_mut(&self) -> std::cell::RefMut<'_, SelectionInfo> {
        let doc = &self.game.doc;
        let scene = (doc.id, self.game.scene.doc_id, self.game.scene.parts.len());
        let journal = doc.journal_end();
        let mut info = self.selection_info.borrow_mut();
        let same_parts = info.scene == Some(scene) && info.selection == doc.selection;
        if !same_parts || (info.journal != journal && !self.only_camera_motion_since(info.journal)) {
            *info = SelectionInfo {
                scene: Some(scene),
                selection: doc.selection.clone(),
                parts: std::rc::Rc::new(self.walk_selected_parts()),
                ..Default::default()
            };
        }
        info.journal = journal;
        info
    }

    /// View-only edits leave selection membership, ribbon values and outline
    /// geometry intact. An expired journal falls back to re-reading the scene.
    pub(crate) fn only_camera_motion_since(&self, journal: u64) -> bool {
        let doc = &self.game.doc;
        doc.marks_since(journal).is_some_and(|marks| {
            marks.iter().all(|mark| match mark {
                doc::Mark::Prop(r, name) if matches!(name.as_str(), "CFrame" | "Focus" | "FieldOfView") => {
                    doc.dom.get_by_ref(*r).is_some_and(|camera| camera.class == "Camera")
                }
                _ => false,
            })
        })
    }

    /// The selected parts (selected_parts) without copying them: the menu,
    /// ribbon and outline ask several times a frame, and walking a big
    /// selection's descendants each time cost ~60M instructions a frame on
    /// a select-all of 5 000 parts.
    pub(crate) fn selected_parts_shared(&self) -> std::rc::Rc<Vec<Ref>> {
        self.selection_info_mut().parts.clone()
    }

    /// Whether every selected part has `prop` true (Lock / Anchor's checks).
    pub(crate) fn selected_parts_all(&self, prop: &'static str) -> bool {
        let mut info = self.selection_info_mut();
        if let Some(&(_, v)) = info.all_true.iter().find(|(p, _)| *p == prop) {
            return v;
        }
        let v = info.parts.iter().all(|r| matches!(self.game.doc.prop(*r, prop), Some(rbx_dom_weak::types::Variant::Bool(true))));
        info.all_true.push((prop, v));
        v
    }

    /// The selection's material when every selected part shares one.
    pub(crate) fn selected_parts_material(&self) -> Option<u32> {
        let mut info = self.selection_info_mut();
        if let Some(m) = info.material {
            return m;
        }
        let mut out = None;
        for r in info.parts.iter() {
            let m = match self.game.doc.prop(*r, "Material") {
                Some(rbx_dom_weak::types::Variant::Enum(e)) => e.to_u32(),
                _ => 256,
            };
            if out.is_some_and(|o| o != m) {
                out = None;
                break;
            }
            out = Some(m);
        }
        info.material = Some(out);
        out
    }

    pub fn selected_parts(&self) -> Vec<Ref> {
        (*self.selected_parts_shared()).clone()
    }

    fn walk_selected_parts(&self) -> Vec<Ref> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for &r in &self.game.doc.selection {
            if self.game.scene.get(r).is_some() && seen.insert(r) {
                out.push(r);
            }
            for d in self.game.doc.dom.descendants_of(r) {
                let dr = d.referent();
                if dr != r && self.game.scene.get(dr).is_some() && seen.insert(dr) {
                    out.push(dr);
                }
            }
        }
        out
    }

    /// Bounding box (min, max) of the selected parts.
    pub fn selection_bounds(&self) -> Option<(Vec3, Vec3)> {
        let parts = self.selected_parts();
        let mut it = parts.iter().filter_map(|r| self.game.scene.get(*r));
        let first = it.next()?;
        let mut b = crate::viewport::part_aabb(first);
        for p in it {
            let (mn, mx) = crate::viewport::part_aabb(p);
            b = (b.0.min(mn), b.1.max(mx));
        }
        Some(b)
    }

    pub fn focus_selection(&mut self) {
        self.game.scene.sync(&mut self.game.doc);
        match self.selection_bounds() {
            Some((mn, mx)) => self.game.cam.frame((mn + mx) * 0.5, (mx - mn).length() * 0.5),
            None => self.warn("Select a part to focus on"),
        }
    }

    /// Frames everything except huge baseplates.
    /// Frames where most of the parts are: the 10th–90th percentile box of
    /// part centres, so a few stray far-away parts don't throw it off.
    pub fn frame_all(&mut self) {
        self.game.frame_all();
    }

    /// The sample scene used for screenshots and `--demo`.
    pub fn build_demo(&mut self) {
        let last = build_demo_parts(&mut self.game.doc);
        self.game.doc.selection = vec![last];
        if self.builtin_tools.use_native(rustudio_core::rustudio_settings::BuiltinTool::Draggers) {
            self.tool = Tool::Move;
        }
        self.game.cam = Camera::default();
        self.game.cam.pos = Vec3::new(14.0, 22.0, 34.0);
        self.game.cam.yaw = 0.32;
        self.game.cam.pitch = -0.5;
        self.game.cam.focus = self.game.cam.pos + self.game.cam.forward() * 40.0;
        // Demo edits should not show up as undoable history.
        self.game.doc.clear_history();
        self.info("Loaded the sample scene");
    }
}

// ----------------------------------------------------------------- shortcuts

impl Editor {
    fn shortcuts(&mut self, ctx: &egui::Context) {
        // Test controls work even while typing in a script.
        let (stop, play, run) = ctx.input_mut(|i| {
            let stop = i.consume_shortcut(&KeyboardShortcut::new(Modifiers::SHIFT, Key::F5));
            let play = i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::F5));
            let run = i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::F8));
            (stop, play, run)
        });
        if stop {
            self.request_stop();
        } else if play {
            self.start_run(crate::studio_ui::RunType::Test);
        } else if run {
            self.start_run(crate::studio_ui::RunType::Run);
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        // While playing, WASD/Space/etc. belong to the game.
        self.game_keys(ctx);
        // When a Play-mode TextBox has keyboard focus, don't let Studio editor
        // shortcuts (Ctrl+C/X/V/A, Delete, etc.) steal its editing events.
        if self.game.play_game_input() && self.game.gui.focus.is_some() {
            return;
        }
        // Studio's menu accelerators (Ctrl+Shift+V, Ctrl+Alt+G, Alt+A, F11…).
        for a in self.menu_shortcut_actions(ctx) {
            self.ui_action(ctx, a);
        }
        for a in std::mem::take(&mut self.harness.ui_due) {
            self.ui_action(ctx, &a);
        }
        let sc = |m: Modifiers, k: Key| KeyboardShortcut::new(m, k);
        let cs = Modifiers::COMMAND | Modifiers::SHIFT;
        let mut actions: Vec<&str> = Vec::new();
        let textbox = (self.game.play.is_some() && self.game.gui.focus.is_some()) || self.plugin_ui.has_text_focus();
        ctx.input_mut(|i| {
            let mut check = |s: KeyboardShortcut, name: &'static str| {
                if i.consume_shortcut(&s) {
                    actions.push(name);
                }
            };
            check(sc(cs, Key::P), "focus_props_filter");
            check(sc(cs, Key::Z), "redo");
            check(sc(cs, Key::S), "save_as");
            check(sc(cs, Key::V), "paste_into");
            check(sc(Modifiers::COMMAND, Key::Z), "undo");
            check(sc(Modifiers::COMMAND, Key::Y), "redo");
            check(sc(Modifiers::COMMAND, Key::D), "duplicate");
            check(sc(Modifiers::COMMAND, Key::G), "group");
            check(sc(Modifiers::COMMAND, Key::U), "ungroup");
            check(sc(Modifiers::COMMAND, Key::S), "save");
            check(sc(Modifiers::COMMAND, Key::O), "open");
            check(sc(Modifiers::COMMAND, Key::N), "new");
            check(sc(Modifiers::COMMAND, Key::I), "insert_object");
            check(sc(Modifiers::COMMAND, Key::L), "toggle_space");
            check(sc(Modifiers::COMMAND, Key::R), "rotate_selection");
            check(sc(Modifiers::COMMAND, Key::T), "tilt_selection");
            check(sc(Modifiers::NONE, Key::Num1), "tool_select");
            check(sc(Modifiers::NONE, Key::Num2), "tool_move");
            check(sc(Modifiers::NONE, Key::Num3), "tool_scale");
            check(sc(Modifiers::NONE, Key::Num4), "tool_rotate");
            check(sc(Modifiers::COMMAND, Key::Num1), "tool_select");
            check(sc(Modifiers::COMMAND, Key::Num2), "tool_move");
            check(sc(Modifiers::COMMAND, Key::Num3), "tool_scale");
            check(sc(Modifiers::COMMAND, Key::Num4), "tool_rotate");
            check(sc(Modifiers::NONE, Key::Delete), "delete");
            // A Mac keyboard's delete key is Backspace (Studio for Mac
            // deletes with it too).
            if cfg!(target_os = "macos") {
                check(sc(Modifiers::NONE, Key::Backspace), "delete");
            }
            check(sc(Modifiers::NONE, Key::F2), "rename");
            check(sc(Modifiers::NONE, Key::F), "focus");
            // egui turns Ctrl+C/X/V into clipboard events — a Roblox
            // TextBox's while one has the keyboard (gui_input.rs
            // `Gui::keyboard` copies its text and types a paste in).
            if !textbox {
                for ev in &i.events {
                    match ev {
                        egui::Event::Copy => actions.push("copy"),
                        egui::Event::Cut => actions.push("cut"),
                        // egui-winit sends Ctrl+V and Ctrl+Shift+V as Paste (only
                        // when the system clipboard holds text: copy puts a note
                        // there); Shift held means Paste Into.
                        egui::Event::Paste(_) if i.modifiers.shift => actions.push("paste_into"),
                        egui::Event::Paste(_) => actions.push("paste"),
                        _ => {}
                    }
                }
            }
        });
        if actions.contains(&"focus_props_filter") {
            ctx.memory_mut(|m| m.request_focus(egui::Id::new(crate::properties::PROPS_FILTER_ID)));
        }
        let copied = actions.iter().any(|a| matches!(*a, "copy" | "cut"));
        for a in actions {
            self.run_action(a);
        }
        if copied {
            self.note_os_clipboard(ctx);
        }
    }

    pub(crate) fn game_keys(&mut self, ctx: &egui::Context) {
        self.game.game_keys(ctx, self.harness.run.no_ui);
    }

    /// Puts a line on the system clipboard after a copy: egui-winit only
    /// delivers Ctrl+V when the system clipboard holds text.
    pub fn note_os_clipboard(&self, ctx: &egui::Context) {
        if self.builtin_tools.use_native(rustudio_core::rustudio_settings::BuiltinTool::TerrainEditor)
            && self.terrain_ed.tool.is_some_and(crate::terrain_editor::Tool::is_region)
        {
            if self.terrain_ed.clipboard.is_some() && self.harness.selftest.is_none() {
                ctx.copy_text("rustudio: terrain region copied".to_string());
            }
            return;
        }
        if self.clipboard.is_some() && self.harness.selftest.is_none() {
            ctx.copy_text(format!("rustudio: {} object(s) copied", self.game.doc.selection.len()));
        }
    }

    pub fn run_action(&mut self, a: &str) {
        // A Terrain Editor Region tool has Copy, Cut, Paste, Duplicate and Delete.
        if self.builtin_tools.use_native(rustudio_core::rustudio_settings::BuiltinTool::TerrainEditor)
            && self.terrain_ed.tool.is_some_and(crate::terrain_editor::Tool::is_region)
            && matches!(a, "copy" | "cut" | "paste" | "duplicate" | "delete")
            && self.terrain_region_action(a)
        {
            return;
        }
        match a {
            "undo" => self.undo(),
            "redo" => self.redo(),
            "copy" => {
                self.copy();
            }
            "cut" => self.cut(),
            "paste" => self.paste(false),
            "paste_into" => self.paste(true),
            "duplicate" => self.duplicate(),
            "delete" => self.delete(),
            "group" => self.group(),
            "ungroup" => self.ungroup(),
            "save" => self.save(),
            "save_as" => self.save_as(),
            "open" => self.request(PendingAction::Open(None)),
            "new" => self.request(PendingAction::New),
            "insert_object" => {
                let parent = self.insert_parent();
                let at = self.game.viewport_rect.center() - egui::vec2(120.0, 200.0);
                self.open_insert_popup(parent, at);
            }
            "toggle_space" => self.game.local_space = !self.game.local_space,
            "rotate_selection" => self.rotate_selection(false),
            "tilt_selection" => self.rotate_selection(true),
            "tool_select" => self.trigger_dragger_or_native(Tool::Select, "SelectDragger"),
            "tool_move" => self.trigger_dragger_or_native(Tool::Move, "MoveDragger"),
            "tool_scale" => self.trigger_dragger_or_native(Tool::Scale, "ScaleDragger"),
            "tool_rotate" => self.trigger_dragger_or_native(Tool::Rotate, "RotateDragger"),
            "rename" => {
                if let Some(&r) = self.game.doc.selection.first() {
                    let name = self.game
                        .doc
                        .dom
                        .get_by_ref(r)
                        .map(|i| i.name.clone())
                        .unwrap_or_default();
                    self.explorer.renaming = Some((r, name));
                }
            }
            "focus" => self.focus_selection(),
            "focus_props_filter" => self.ui.panel_requests.push((Tab::Properties, Some(true))),
            "audio_preview" => {
                if self.game.play.is_none() && self.game.doc.selection.len() == 1 {
                    let r = self.game.doc.selection[0];
                    if matches!(self.game.doc.class_of(r).as_deref(), Some("Sound" | "AudioPlayer")) {
                        self.game.edit_audio_preview_toggle(r);
                    }
                }
            }
            _ => {}
        }
    }

    /// Asks before throwing away unsaved changes.
    pub fn request(&mut self, action: PendingAction) {
        // A live Team Create document asks only for what the server lacks.
        if self.leave_check() != crate::cloud_ui::live_status::Leave::Free && self.harness.selftest.is_none() {
            self.confirm_discard = Some(action);
        } else {
            self.perform(action);
        }
    }

    fn perform(&mut self, action: PendingAction) {
        if !matches!(action, PendingAction::Close) && self.refuse_during_play("open or create another place") {
            return;
        }
        match action {
            PendingAction::New => self.new_place(),
            PendingAction::Open(None) => self.open_dialog(),
            PendingAction::Open(Some(p)) => self.open_path(p),
            PendingAction::OpenFolder => self.open_folder_dialog(),
            PendingAction::OpenCloud(p, v, copy) => self.perform_open_cloud(p, v, copy),
            PendingAction::Close => {
                if self.game.play.is_some() {
                    self.stop_play();
                }
                self.new_place();
                self.show_start_page();
            }
            PendingAction::Quit => {
                crate::studio_settings::flush();
                rustudio_core::rustudio_settings::flush();
                std::process::exit(0)
            }
            PendingAction::Restart => self.restart(),
        }
    }

    /// Starts a new rustudio on what's open now (the live Team Create
    /// session, the file or project folder, or a Roblox place's local copy)
    /// and exits this one.
    fn restart(&mut self) {
        let mut args: Vec<String> = Vec::new();
        if let Some((universe, place)) = self.live_team_create_ids() {
            args = vec!["--team-create".into(), universe.to_string(), place.to_string()];
        } else if let Some(path) = &self.game.doc.path {
            args.push(path.display().to_string());
        } else if let Some(origin) = &self.game.doc.cloud {
            args = vec!["--open-roblox".into(), origin.place_id.to_string(), "--local-copy".into()];
        }
        // After an update the running file was renamed: start the new one.
        let exe = crate::update::installed_exe()
            .or_else(|| std::env::current_exe().ok())
            .unwrap_or_else(|| PathBuf::from("rustudio"));
        match std::process::Command::new(&exe).args(&args).spawn() {
            Ok(_) => std::process::exit(0),
            Err(e) => self.error(format!("Restart failed: couldn't start {}: {e}", exe.display())),
        }
    }
}

// --------------------------------------------------------------- menus & bars

impl Editor {
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        self.hud.status_drawn = true;
        ui.horizontal(|ui| {
            self.live_status_ui(ui);
            let sel = self.game.doc.selection.len();
            if let Some(s) = self.game.play_status() {
                ui.label(RichText::new(format!("● {s}")).color(theme::ERROR).strong());
                ui.separator();
            }
            let tool_label = if self.builtin_tools.use_roblox(rustudio_core::rustudio_settings::BuiltinTool::Draggers)
                && self.tool == Tool::None
            {
                self.active_roblox_dragger_label().unwrap_or(self.tool.label())
            } else {
                self.tool.label()
            };
            ui.label(RichText::new(format!("{tool_label} tool")).color(theme::ACCENT));
            ui.separator();
            ui.label(format!("{sel} selected"));
            ui.separator();
            ui.label(format!("{} parts", self.game.scene.parts.len()));
            ui.separator();
            let p = self.game.cam.pos;
            ui.label(format!("Camera {:.1}, {:.1}, {:.1}", p.x, p.y, p.z));
            ui.separator();
            ui.label(if self.game.snap_move {
                format!("Snap {} st / {}°", self.game.move_inc, self.game.rotate_inc)
            } else {
                "Snap off".into()
            });
            if let Some((done, asked)) = self.game.assets.progress() {
                ui.separator();
                ui.label(RichText::new(format!("Loading assets {done}/{asked}")).color(theme::TEXT_DIM));
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("{:.0} fps", self.fps)).color(theme::TEXT_DIM));
            });
        });
    }

    /// Opens requested scripts as tabs next to the viewport (or focuses
    /// them, in a panel window too).
    fn open_script_tabs(&mut self, dock: &mut DockState<Tab>, panels: &mut crate::panels::Panels) {
        // Open In Editor: File Sync's scripts go to the OS editor instead.
        self.file_sync_open_external();
        // Tabs plugins closed, then the open script tabs as
        // ScriptEditorService documents (a tab the person closed closes its
        // document; the front one is StudioService.ActiveScript).
        for r in std::mem::take(&mut self.close_script) {
            let tab = Tab::Script(r);
            if let Some(at) = dock.find_tab(&tab) {
                dock.remove_tab(at);
            }
            for w in panels.popped.iter_mut() {
                if let Some(at) = w.dock.find_tab(&tab) {
                    w.dock.remove_tab(at);
                }
            }
        }
        let mut tabs: Vec<Ref> = dock.iter_all_tabs().filter_map(|(_, t)| match t { Tab::Script(r) => Some(*r), _ => None }).collect();
        for w in panels.popped.iter() {
            tabs.extend(w.dock.iter_all_tabs().filter_map(|(_, t)| match t { Tab::Script(r) => Some(*r), _ => None }));
        }
        tabs.extend(self.open_script.iter().copied());
        crate::plugin_host::script_docs::sync_tabs(&tabs);
        if let Some((_, Tab::Script(r))) = dock.find_active_focused() {
            crate::plugin_host::script_docs::set_active(Some(*r));
        }
        for r in std::mem::take(&mut self.open_script) {
            let tab = Tab::Script(r);
            if let Some(w) = panels.popped.iter_mut().find(|w| w.dock.find_tab(&tab).is_some()) {
                if let Some(at) = w.dock.find_tab(&tab) {
                    let _ = w.dock.set_active_tab(at);
                }
                w.focus = true;
                continue;
            }
            let tree = dock.main_surface_mut();
            if let Some((node, tab)) = tree.find_tab(&tab) {
                let _ = tree.set_active_tab(node, tab);
                tree.set_focused_node(node);
                continue;
            }
            crate::panels::place(dock, tab, &panels.homes);
        }
    }

    fn confirm_ui(&mut self, ctx: &egui::Context) {
        let Some(action) = self.confirm_discard.clone() else {
            return;
        };
        let mut decided: Option<bool> = None;
        let leave = self.leave_check();
        egui::Modal::new(egui::Id::new("confirm_discard")).show(ctx, |ui| {
            if let crate::cloud_ui::live_status::Leave::Ask { lacking, note } = &leave {
                if !note.is_empty() {
                    ui.label(RichText::new("Leave without these edits?").strong());
                    ui.label(note);
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        for item in lacking {
                            ui.label(format!("• {item}"));
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Save As…").clicked() {
                            self.save_as();
                        }
                        if ui.button("Close anyway").clicked() {
                            decided = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            decided = Some(false);
                        }
                    });
                    return;
                }
            }
            ui.label(RichText::new("Discard unsaved changes?").strong());
            ui.label(format!(
                "{} has changes that are not saved.",
                self.game.doc.title
            ));
            ui.horizontal(|ui| {
                if ui.button("Save first").clicked() {
                    self.save();
                    decided = Some(!self.game.doc.dirty());
                }
                if ui.button("Discard").clicked() {
                    decided = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    decided = Some(false);
                }
            });
        });
        if let Some(go) = decided {
            self.confirm_discard = None;
            if go {
                self.perform(action);
            }
        }
    }
}

struct Viewer<'a> {
    ed: &'a mut Editor,
    /// The panel window this dock is in (None: the main window).
    window: Option<&'a str>,
    /// Every panel window: key and title (the tab menu's Move to window).
    windows: &'a [(String, String)],
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        match tab {
            Tab::Script(r) => format!("📜 {}", self.ed.script_title(*r)).into(),
            // Studio's document tab: the place's name.
            Tab::Viewport => match self.ed.game.play_status() {
                Some(s) => RichText::new(format!("{} — {s}", self.ed.game.doc.title))
                    .color(theme::ACCENT)
                    .into(),
                None => self.ed.game.doc.title.clone().into(),
            },
            Tab::Properties => crate::properties::panel_title(self.ed).into(),
            Tab::Plugin(n) => self.ed.plugin_widget_title(*n).into(),
            other => other.title().into(),
        }
    }

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        match tab {
            Tab::Script(r) => egui::Id::new(("script_tab", *r)),
            Tab::Plugin(n) => egui::Id::new(("plugin_widget_tab", *n)),
            other => egui::Id::new(("tab", other.title())),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        let _p = rustudio_core::bench::scope(match tab {
            Tab::Viewport => "viewport",
            Tab::Explorer => "explorer",
            Tab::Properties => "properties",
            Tab::Output => "output",
            Tab::Script(_) => "script_editor",
            _ => "other_tab",
        });
        match tab {
            Tab::Viewport => self.ed.viewport_ui(ui),
            Tab::Explorer if self.window.is_none() => self.ed.docked_panel(ui, *tab, Editor::explorer_ui),
            Tab::Explorer => self.ed.explorer_ui(ui),
            Tab::Properties if self.window.is_none() => self.ed.docked_panel(ui, *tab, Editor::properties_ui),
            Tab::Properties => self.ed.properties_ui(ui),
            Tab::Output if self.window.is_none() => self.ed.docked_panel(ui, *tab, Editor::output_ui),
            Tab::Output => self.ed.output_ui(ui),
            Tab::AvatarSettings => self.ed.avatar_settings_ui(ui),
            Tab::TerrainEditor => self.ed.terrain_editor_ui(ui),
            Tab::AnimationEditor => self.ed.anim_editor_ui(ui),
            Tab::Plugin(n) => self.ed.plugin_widget_ui(ui, *n),
            Tab::Toolbox => self.ed.toolbox_ui(ui),
            Tab::AssetManager => self.ed.asset_manager_ui(ui),
            Tab::FileSync => self.ed.file_sync_panel_ui(ui),
            Tab::Profiler => {
                let rect = ui.available_rect_before_wrap();
                let mut c = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                c.set_clip_rect(rect);
                let p = self.ed.profiler.get_or_insert_with(rustudio_profiler_ui::ProfilerWindow::new);
                p.can_open_scripts = true;
                p.show(&mut c);
                if let Some((path, line)) = p.take_open_script() {
                    self.ed.open_script_named(&path, line);
                }
                ui.allocate_rect(rect, egui::Sense::hover());
            }
            Tab::Script(r) => {
                if !self.ed.document_tabs_reset {
                    self.ed.script_tab_ui(ui, *r);
                }
            },
        }
    }

    fn show_pop_out_button(&self, tab: &Tab) -> bool {
        *tab != Tab::Viewport
    }

    // The button: its own OS window and back (dragging a tab out floats it
    // inside rustudio instead).
    fn pop_out_docks_back(&self, _tab: &Tab) -> bool {
        self.window.is_some()
    }

    fn pop_out_tooltip(&self, _tab: &Tab) -> String {
        if self.window.is_some() {
            "Dock back into the main window".into()
        } else {
            "Pop out into its own window".into()
        }
    }

    fn on_pop_out(&mut self, tab: &mut Tab) {
        use crate::panels::DockMove;
        self.ed.ui.dock_moves.push(if self.window.is_some() { DockMove::DockBack(*tab) } else { DockMove::PopOut(*tab) });
    }

    fn context_menu(&mut self, ui: &mut egui::Ui, tab: &mut Tab, _path: egui_dock::NodePath) {
        use crate::panels::DockMove;
        if *tab == Tab::Viewport {
            return;
        }
        let alone = self.window.is_some()
            && self.ed.ui.popped.iter().find(|(k, _)| Some(k.as_str()) == self.window).is_some_and(|(_, t)| t.len() == 1);
        let floating = self.window.is_none() && self.ed.ui.floating_tabs.contains(tab);
        if !alone && ui.button("Pop Out").clicked() {
            self.ed.ui.dock_moves.push(DockMove::PopOut(*tab));
        }
        if self.window.is_none() && !floating && ui.button("Float in Window").clicked() {
            self.ed.ui.dock_moves.push(DockMove::Float(*tab));
        }
        if floating && ui.button("Dock Back").clicked() {
            self.ed.ui.dock_moves.push(DockMove::Unfloat(*tab));
        }
        if self.window.is_some() && ui.button("Dock Back").clicked() {
            self.ed.ui.dock_moves.push(DockMove::DockBack(*tab));
        }
        let others: Vec<&(String, String)> = self.windows.iter().filter(|(k, _)| Some(k.as_str()) != self.window).collect();
        if !others.is_empty() {
            ui.menu_button("Move to Window", |ui| {
                for (k, title) in others {
                    if ui.button(title).clicked() {
                        self.ed.ui.dock_moves.push(DockMove::MoveTo(*tab, k.clone()));
                    }
                }
            });
        }
    }

    fn no_tab_bar(&self, tabs: &[Tab]) -> bool {
        // The 3D view alone: no strip with the place's name over it.
        tabs == [Tab::Viewport]
    }

    fn show_eject_in_context_menu(&self, _tab: &Tab) -> bool {
        false
    }

    fn allowed_in_windows(&self, tab: &mut Tab) -> bool {
        // Dragged out, a tab becomes a panel window; the 3D view stays.
        *tab != Tab::Viewport
    }

    fn is_closeable(&self, tab: &Tab) -> bool {
        // The place tab's × is File > Close Place (not while playing).
        *tab != Tab::Viewport || self.ed.game.play_status().is_none()
    }

    fn on_close(&mut self, tab: &mut Tab) -> egui_dock::tab_viewer::OnCloseResponse {
        if *tab == Tab::Viewport {
            // Closing the place goes back to the Start Page (asks first
            // when there are unsaved changes); the tab itself stays.
            self.ed.request(PendingAction::Close);
            return egui_dock::tab_viewer::OnCloseResponse::Ignore;
        }
        if let Tab::Plugin(n) = *tab {
            // The plugin hears of it (BindToClose, or Enabled = false) and
            // the panel goes when the widget is disabled.
            self.ed.plugin_widget_close(n);
            return egui_dock::tab_viewer::OnCloseResponse::Ignore;
        }
        egui_dock::tab_viewer::OnCloseResponse::Close
    }

    fn scroll_bars(&self, _tab: &Tab) -> [bool; 2] {
        [false, false]
    }

    fn clear_background(&self, tab: &Tab) -> bool {
        *tab != Tab::Viewport
    }
}

impl RustudioApp {
    fn reset_document_tabs(&mut self) {
        if std::mem::take(&mut self.ed.document_tabs_reset) {
            crate::panels::close_document_tabs(&mut self.dock, &mut self.panels);
            strip_disabled_native_tabs(&mut self.dock, &mut self.panels, self.ed.builtin_tools);
            self.ed.ui.popped.clear();
            self.ed.ui.viewport_group.clear();
        }
    }

    /// Draws each panel window (its own OS window: an immediate viewport),
    /// then does the frame's pop-out / dock-back moves and turns tabs
    /// dragged out of a dock into windows. `dock_rect`: the main dock's
    /// area (a popped tab's window opens over where it was).
    fn panel_windows(&mut self, ctx: &egui::Context, windows: &[(String, String)], dock_rect: egui::Rect) {
        self.panels.remember_windows();
        if !self.panels.popped.is_empty() {
            crate::hypr::float_panel_windows();
        }
        let mut closed = Vec::new();
        for (i, w) in self.panels.popped.iter_mut().enumerate() {
            let id = w.viewport_id();
            if !w.preplaced {
                w.preplaced = true;
                crate::hypr::preplace(&w.app_id(), w.wm_restore.as_ref(), w.open_size(), w.open_pos());
            }
            if std::mem::take(&mut w.focus) {
                ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
            }
            let ed = &mut self.ed;
            // eframe panics ("the user callback was never called") when it
            // can't make the window this frame (no event loop context, or
            // the platform refused the window: seen on a Windows host's hidden
            // desktop). That's not fatal: skip the window this frame, and
            // after a run of failures dock its tabs back.
            let builder = w.builder();
            let shown = rustudio_core::crash::recoverable(std::panic::AssertUnwindSafe(|| ctx.show_viewport_immediate(id, builder, |ui, class| {
                ed.ui.popped_class = Some(class);
                let vctx = ui.ctx().clone();
                w.track(&vctx);
                let key = w.key.clone();
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(theme::BG))
                    .show(ui, |ui| {
                        DockArea::new(&mut w.dock)
                            .id(egui::Id::new(("panel_window_dock", &key)))
                            .style(theme::dock_style(ui.style()))
                            .show_add_buttons(false)
                            .show_leaf_collapse_buttons(false)
                            .show_leaf_close_all_buttons(false)
                            .show_inside(ui, &mut Viewer { ed: &mut *ed, window: Some(&key), windows });
                    });
                vctx.input(|i| i.viewport().close_requested())
            })));
            let close = match shown {
                Ok(c) => {
                    w.fails = 0;
                    c
                }
                Err(_) => {
                    w.fails += 1;
                    eprintln!("rustudio: panel window {} could not be shown ({} frames)", w.key, w.fails);
                    ctx.request_repaint();
                    w.fails >= rustudio_core::mech::PANEL_WINDOW_MAX_FAILS
                }
            };
            if close {
                // Closing the window docks its panels back (not lost).
                closed.push(i);
            }
        }
        for i in closed.into_iter().rev() {
            self.panels.dock_back_window(&mut self.dock, i);
        }
        // Tabs dragged out of a panel window's dock: windows of their own.
        let zoom = ctx.zoom_factor();
        let mut adopted = Vec::new();
        for w in self.panels.popped.iter_mut() {
            let mut sub = crate::panels::Panels::default();
            let origin = w.pos.map(|p| egui::pos2(p[0] / zoom, p[1] / zoom));
            sub.adopt_floating(&mut w.dock, origin, zoom);
            adopted.extend(sub.popped);
        }
        self.panels.popped.retain(|w| w.dock.iter_all_tabs().next().is_some());
        for mut w in adopted {
            w.key = self.panels.free_key(w.tabs()[0].key());
            self.panels.popped.push(w);
        }
        // Main window: its outer top-left on the screen (None on Wayland).
        let origin = ctx.input(|i| {
            let v = i.viewport();
            match (v.outer_rect, v.inner_rect) {
                (Some(_), Some(inner)) => Some(inner.min),
                _ => None,
            }
        });
        // (Tabs dragged out of the main dock stay floating inside it.)
        for mv in std::mem::take(&mut self.ed.ui.dock_moves) {
            match &mv {
                crate::panels::DockMove::Float(t) => {
                    let Some(path) = self.dock.find_main_surface_tab(t).map(|(n, i)| (n, i)) else { continue };
                    let rect = self.dock.main_surface()[path.0]
                        .get_leaf()
                        .map(|l| l.rect)
                        .filter(|r| r.is_finite() && r.width() > 50.0 && r.height() > 50.0)
                        .unwrap_or_else(|| egui::Rect::from_center_size(dock_rect.center(), egui::vec2(420.0, 320.0)));
                    let Some(found) = self.dock.find_tab(t) else { continue };
                    let Some(tab) = self.dock.remove_tab(found) else { continue };
                    let surface = self.dock.add_window(vec![tab]);
                    if let Some(w) = self.dock.get_window_state_mut(surface) {
                        w.set_position(rect.min + egui::vec2(24.0, 24.0)).set_size(rect.size());
                    }
                    continue;
                }
                crate::panels::DockMove::Unfloat(t) => {
                    if let Some(found) = self.dock.find_tab(t).filter(|p| !p.surface.is_main()) {
                        if let Some(tab) = self.dock.remove_tab(found) {
                            // (not the remembered whole-layout snapshot: it was
                            // taken with this tab floating and would float it again)
                            let mut homes = self.panels.homes.clone();
                            homes.remove("__topology");
                            crate::panels::place(&mut self.dock, tab, &homes);
                        }
                    }
                    continue;
                }
                _ => {}
            }
            // A popped tab opens about where it sat, at its size.
            let tab = match &mv {
                crate::panels::DockMove::PopOut(t) => Some(*t),
                _ => None,
            };
            let rect = tab
                .and_then(|t| self.dock.find_main_surface_tab(&t))
                .and_then(|(n, _)| self.dock.main_surface()[n].get_leaf().map(|l| l.rect))
                .filter(|r| r.is_finite() && r.width() > 50.0 && r.height() > 50.0)
                .unwrap_or_else(|| egui::Rect::from_center_size(dock_rect.center(), egui::vec2(420.0, 320.0)));
            let size = [(rect.width() * zoom).max(320.0), (rect.height() * zoom).max(220.0)];
            let pos = origin.filter(|_| rect.is_finite()).map(|o| [(o.x + rect.min.x) * zoom, (o.y + rect.min.y) * zoom]);
            self.panels.apply(&mut self.dock, mv, size, pos);
        }
    }
}

impl eframe::App for RustudioApp {
    fn on_exit(&mut self) {
        crate::studio_settings::flush();
        rustudio_core::rustudio_settings::flush();
        rustudio_render::pipeline_cache::save(true);
        crate::plugin_host::widgets_ui::on_exit();
        if let Some(s) = self.layout.as_mut() {
            s.save(&self.dock, &self.panels, &self.ed);
        }
    }
    /// eframe's own save moments (every 30 s, the activity losing its
    /// surface): on Android the loop can end without `on_exit` (a destroyed
    /// activity leaves the process, src/android/lifecycle.rs), so the layout
    /// goes to disk here too.
    #[cfg(target_os = "android")]
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        if let Some(s) = self.layout.as_mut() {
            s.save(&self.dock, &self.panels, &self.ed);
        }
        rustudio_render::pipeline_cache::save(true);
        // Going to the background: the free heap pages go back now, so the
        // process is judged (and kept) by what it really holds.
        rustudio_core::heap::trim();
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Fingers: scroll / tap / hold / look, before egui sees them; a
        // paste brings the system clipboard's text.
        #[cfg(target_os = "android")]
        {
            // The Start Page is plain egui (lists that scroll by drag,
            // cards that are taps, long presses for menus): the finger
            // layer is for the editor's chrome and 3D view, once a place
            // is open.
            let start_page = self.ed.start.visible && self.ed.game.doc.id == self.ed.start.doc_id;
            self.ed.game.touch.set_enabled(!start_page, raw);
            self.ed.game.touch.filter(ctx, raw, self.ed.game.frame_no);
            // (the clipboard first: it swaps egui-winit's pastes, Ctrl+V,
            // for the system clipboard's text; the menu's and the bar's
            // pastes come with their text)
            crate::android::clipboard::before_frame(raw);
            self.text_menu.before_frame(raw);
            self.text_bar.before_frame(ctx, raw);
            // What the soft keyboard typed (src/android/ime.rs).
            crate::android::ime::before_frame(ctx, raw);
            let textbox = self.ed.textbox_focused();
            let game_box = self.ed.game.play.is_some() && self.ed.game.gui.focus.is_some();
            self.keyboard.before_frame(ctx, raw, textbox, game_box);
            // The display cutout's insets (with the keyboard's bottom).
            self.cutout.before_frame(ctx, raw);
            let playing = self.ed.game.play_game_input();
            crate::android::lifecycle::back_pressed(ctx, raw, playing);
            // The window back in focus: the system bars hidden again.
            crate::android::system_bars::before_frame(raw);
        }
        // A scripted run's window on Windows sits on the real desktop: the
        // person's (or an idle session's) mouse pointer over it would hover
        // and click like the script (a ribbon tooltip in a screenshot). Linux
        // runs them in a headless gamescope with no pointer; here the real
        // pointer is dropped before the script's own input goes in.
        if cfg!(windows)
            && (self.ed.harness.selftest.is_some() || self.ed.harness.run.screenshot.is_some() || self.ed.harness.ui_shots.is_some())
            && std::env::var_os("RUSTUDIO_VISIBLE_TESTS").is_none()
        {
            raw.events.retain(|e| {
                !matches!(
                    e,
                    egui::Event::PointerMoved(_)
                        | egui::Event::MouseMoved(_)
                        | egui::Event::PointerButton { .. }
                        | egui::Event::PointerGone
                        | egui::Event::MouseWheel { .. }
                        | egui::Event::Touch { .. }
                )
            });
        }
        if let Some(t) = self.ed.harness.selftest.as_mut() {
            t.inject(raw);
        }
        if !self.ed.harness.run.input.actions.is_empty() {
            self.ed.harness_inject(raw);
        }
        if let Some(u) = self.ed.harness.ui_shots.as_mut() {
            u.inject(raw);
        }
        if self.ed.game.play_game_input() && ctx.memory(|m| m.focused().is_none()) {
            rustudio_player::keys::PhysicalKeys::keep_tab_for_game(raw);
        }
        self.ed.game.phys.ingest(raw);
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // eframe can run logic without a UI pass when the root is hidden.
        if ctx.input(|i| i.viewport().visible() == Some(false)) {
            Self::tick_plugins(&mut self.ed, &mut self.last_plugin_tick, Instant::now());
            Self::schedule_plugin_repaint(&self.ed, ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // A lost GPU device: on the desktop the frame stops here with the
        // reason in the crash report. On Android the device is lost when a
        // phone runs out of memory (a 2026-11-24 report: a 250 s Play of a
        // large place, 7.2 GB resident, "GPU device lost: Out of memory");
        // that check ran before `gpu_watch` below and panicked before it
        // could save the open place and relaunch — the user got a crash
        // report where a saved copy was meant. The Android build leaves the
        // loss to `gpu_watch` (a save to the recovery folder, a report, a
        // relaunch) and never panics for it.
        #[cfg(not(target_os = "android"))]
        rustudio_render::gpu_health::check();
        // The driver's pipeline cache goes to disk once a burst of pipeline
        // builds is over (pipeline_cache.rs): the next start skips them.
        rustudio_render::pipeline_cache::save(false);
        if let Some(message) = rustudio_render::gpu_health::take_uncaptured() {
            self.ed.warn(message);
        }
        // (a phone shows black until this frame is presented: logcat
        // `RustStdoutStderr` has the renderer's and the first frame's share
        // of its start, render.rs `ensure_pipelines` the viewport's)
        let first = self.last_ui_frame.is_none()
            && (cfg!(target_os = "android") || std::env::var_os("RUSTUDIO_STARTUP_TRACE").is_some());
        // (the frame limit's sleep, before the frame's clock starts; the
        // last frame's sleep is in eframe's measure of it, taken out below)
        #[cfg(target_os = "android")]
        let slept_last = {
            let slept = self.pace_frame();
            std::mem::replace(&mut self.paced_last, slept)
        };
        let t0 = Instant::now();
        self.last_ui_frame = Some(t0);
        if self.egui_ctx.is_none() {
            rustudio_core::load_trace::mark("first ui pass");
            self.egui_ctx = Some(ui.ctx().clone());
        }
        self.cpu_usage = frame.info().cpu_usage;
        #[cfg(target_os = "android")]
        if let Some(c) = self.cpu_usage.as_mut() {
            *c = (*c - slept_last).max(0.0);
        }
        // (a lost GPU device or one out of memory: save and restart,
        // src/android/mod.rs gpu_watch)
        #[cfg(target_os = "android")]
        if crate::android::gpu_watch(&mut self.ed) {
            return;
        }
        self.frame(ui);
        if first {
            eprintln!(
                "[startup] first frame built in {} ms, {} ms after the engine's start",
                t0.elapsed().as_millis(),
                rustudio_engine::runtime::process_start().elapsed().as_millis()
            );
        }
        if let Some(report) = self.perf_report.as_mut() {
            #[cfg(target_os = "android")]
            let slept_s = slept_last;
            #[cfg(not(target_os = "android"))]
            let slept_s = 0.0;
            let adapter = self.ed.game.render_state.as_ref().map(|rs| rs.adapter.get_info()).map(|i| format!("{} ({:?}, {} {})", i.name, i.backend, i.driver, i.driver_info)).unwrap_or_default();
            let line = report.frame(rustudio_player::perf_report::FrameIn {
                cpu_s: self.cpu_usage,
                app_s: t0.elapsed().as_secs_f32(),
                slept_s,
                causes: ui.ctx().repaint_causes(),
                view: self.ed.game.perf_view(),
                adapter: &adapter,
            });
            if let Some(line) = line {
                log::info!("{line}");
            }
        }
    }
}

const PLUGIN_IDLE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
/// Play's step while the window gets no redraws (Studio's 60 Hz).
const HIDDEN_PLAY_INTERVAL: std::time::Duration = std::time::Duration::from_micros(16_667);
/// Team Create alone: often enough that edits keep arriving in order.
const HIDDEN_LIVE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

impl RustudioApp {
    /// Work due between frames: plugin ticks, and while the compositor sends
    /// no redraws (`hidden`: another workspace, minimized) the playtest and
    /// Team Create, which Studio keeps running behind other windows.
    pub fn tick_between_frames(&mut self, now: Instant, hidden: bool) -> Option<Instant> {
        let plugins = self.tick_plugins_between_frames(now);
        let hidden = if hidden { self.hidden_tick(now) } else { None };
        match (plugins, hidden) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// What keeps running in a hidden window (Performance > Hidden window).
    /// Fixed-step and harness runs stay frame-driven, so they replay the same.
    fn hidden_interval(&self) -> Option<std::time::Duration> {
        let ed = &self.ed;
        if rustudio_core::rustudio_settings::performance_value("background") == "pause"
            || ed.game.fixed_dt.is_some()
            || ed.harness.run.screenshot.is_some()
            || ed.harness.run.bench.is_some()
            || ed.harness.selftest.is_some()
            || ed.place_loading()
        {
            return None;
        }
        if ed.game.play.is_some() {
            Some(HIDDEN_PLAY_INTERVAL)
        } else {
            ed.live_session_open().then_some(HIDDEN_LIVE_INTERVAL)
        }
    }

    /// The simulation part of a frame, with nothing drawn: Play steps with no
    /// keys held (the window lost focus), Team Create applies what arrived.
    fn hidden_tick(&mut self, now: Instant) -> Option<Instant> {
        let interval = self.hidden_interval()?;
        if now >= self.last_hidden_tick + interval {
            let _p = rustudio_core::bench::scope("hidden_tick");
            self.last_hidden_tick = now;
            self.watchdog.hidden();
            let ed = &mut self.ed;
            ed.game.frame_no += 1;
            let dt = now.saturating_duration_since(ed.last_frame).as_secs_f32();
            ed.last_frame = now;
            ed.game.poll_assets();
            ed.flush_output_changes();
            if ed.game.play.is_some() {
                let step = ed.game.play_step(dt);
                ed.play_tick_hidden(step);
            }
            if let Some(ctx) = self.egui_ctx.clone() {
                // (no overlays: nothing is drawn until the window shows)
                ed.hud.viewport_drawn = false;
                ed.cloud_poll(&ctx);
            }
        }
        let interval = self.hidden_interval()?;
        Some(self.last_hidden_tick + interval)
    }

    pub fn tick_plugins_between_frames(&mut self, now: Instant) -> Option<Instant> {
        let active_audio = self.ed.game.play.is_none() && self.ed.game.edit_audio.has_active();
        if self.ed.place_loading() || (!self.ed.plugins.needs_idle_tick() && !active_audio) {
            return None;
        }
        let interval = if active_audio {
            std::time::Duration::from_millis(33)
        } else {
            PLUGIN_IDLE_INTERVAL
        };
        if now >= self.last_plugin_tick + interval {
            Self::tick_plugins(&mut self.ed, &mut self.last_plugin_tick, now);
            self.ed.edit_audio_tick();
            self.plugins_ticked_between_frames = true;
        }
        let still_audio = self.ed.game.play.is_none() && self.ed.game.edit_audio.has_active();
        (!self.ed.place_loading() && (self.ed.plugins.needs_idle_tick() || still_audio))
            .then(|| {
                self.last_plugin_tick
                    + if still_audio {
                        std::time::Duration::from_millis(33)
                    } else {
                        PLUGIN_IDLE_INTERVAL
                    }
            })
    }

    fn tick_plugins(ed: &mut Editor, last_tick: &mut Instant, now: Instant) {
        let _p = rustudio_core::bench::scope("plugin_tick");
        let dt = now.saturating_duration_since(std::mem::replace(last_tick, now)).as_secs_f32();
        let step = ed.game.fixed_dt.map_or_else(|| frame_grid(dt) as f64, f64::from);
        ed.plugin_tick(step);
    }

    fn schedule_plugin_repaint(ed: &Editor, ctx: &egui::Context) {
        if !ed.place_loading() && ed.plugins.needs_idle_tick() {
            // Frame-driven work (RunService, timers, and yielding services
            // such as HTTP) still needs a tick. Cap the idle gap at 100 ms
            // without keeping the renderer at its normal refresh rate.
            ctx.request_repaint_after(PLUGIN_IDLE_INTERVAL);
        }
    }

    /// One frame, in eframe's window or offscreen (src/offscreen.rs).
    /// Android, at the end of a frame: the soft keyboard follows what
    /// there is to type into (src/android/keyboard.rs; egui-winit gets no
    /// IME output, src/android/ime.rs), with the Cut / Copy / Paste bar
    /// over it (src/android/text_bar.rs).
    #[cfg(target_os = "android")]
    fn keyboard_end_frame(&mut self, ctx: &egui::Context) {
        let textbox = self.ed.textbox_focused();
        let want = crate::android::keyboard::wanted(ctx, textbox);
        crate::android::ime::end_frame(ctx, textbox);
        self.text_bar.end_frame(ctx, &self.keyboard, want);
        self.keyboard.end_frame(ctx, textbox);
    }

    /// Android, at the end of a frame: all of the phone's text handling —
    /// the keyboard and bar (`keyboard_end_frame`), text copied this frame
    /// to the system clipboard too, and a long press's Cut / Copy / Paste
    /// menu (src/android/text_menu.rs). Every way out of a frame that
    /// shows a text field passes here (the editor, a game filling the
    /// screen, the Start Page).
    #[cfg(target_os = "android")]
    fn text_end_frame(&mut self, ctx: &egui::Context) {
        self.keyboard_end_frame(ctx);
        crate::android::clipboard::after_frame(ctx);
        self.text_menu.end_frame(ctx);
    }

    pub fn frame(&mut self, ui: &mut egui::Ui) {
        // A display frame ends here and the next begins (the profiler gathers
        // every thread's records at this boundary).
        #[cfg(not(target_os = "android"))]
        let native_frames = crate::native_loop::NATIVE_FRAMES.load(std::sync::atomic::Ordering::Relaxed);
        // (the Android activity runs eframe's own loop, src/android/mod.rs:
        // no native_loop to mark the frames, so they are marked here)
        #[cfg(target_os = "android")]
        let native_frames = false;
        if !native_frames {
            rustudio_core::profiler::frame(rustudio_core::profiler::FrameKind::Render);
        }
        // (the whole frame, for --bench: the editor's and --no-ui's alike)
        let _frame_scope = rustudio_core::bench::scope("frame");
        rustudio_core::sim_clock::advance();
        let ctx = ui.ctx().clone();
        self.watchdog.begin(ctx.input(|i| !i.viewport().minimized.unwrap_or(false)) && (self.ed.game.play.is_some() || self.ed.harness.run.screenshot.is_some()));
        // The panels end above the soft keyboard while a field is typed in
        // — except a game's TextBox: the keyboard then lies over the
        // window, as it does over a phone's games (the panels laid out
        // above it leave a landscape phone a view a few lines tall, the
        // box typed into squeezed or off it, the status bar in its
        // place), and the view slides up to show the box
        // (Editor::keyboard_pan). Which of the two: android/keyboard.rs
        // `over`, decided in raw_input_hook with the safe-area inset.
        #[cfg(target_os = "android")]
        {
            self.ed.keyboard_cover = self.keyboard.cover(&ctx);
            self.keyboard.shrink(ui);
        }
        // And short of the display cutout (android/cutout.rs): the rest of
        // the frame is laid out in a Ui that ends before the camera hole.
        #[cfg(target_os = "android")]
        let mut cutout_ui = self.cutout.child(ui);
        #[cfg(target_os = "android")]
        let ui = &mut cutout_ui;
        let ed = &mut self.ed;
        // Files opened from Finder (double-click, Open With, the Dock icon).
        #[cfg(target_os = "macos")]
        for p in crate::macos::take_opened() {
            ed.request(PendingAction::Open(Some(p)));
        }
        // Files handed over by a file manager while the editor runs (a
        // second activity's VIEW intent: src/android/intent.rs), and the
        // opening marker of src/android/storage.rs once the start settled.
        #[cfg(target_os = "android")]
        {
            for p in crate::android::storage::take_opened() {
                ed.request(PendingAction::Open(Some(p)));
            }
            crate::android::storage::settle();
        }
        // The first frames after the activity's surface came up are drawn
        // whatever the event loop says: a redraw request that arrives while
        // the first frame is still being set up has been seen to go
        // missing, leaving the window black until the next resume.
        #[cfg(target_os = "android")]
        {
            if ed.harness.run.started.elapsed() < std::time::Duration::from_secs(3) {
                ctx.request_repaint();
            }
            crate::android::touch::auto_zoom(ed, &ctx);
            // Two frames went through the swapchain: a start on the Turnip
            // driver (android/turnip.rs) counts as good from here.
            if ed.game.frame_no == 2 {
                crate::android::turnip::mark_healthy();
            }
        }
        ed.poll_file_dialogs();
        if let crate::update::Action::Restart = self.updater.show(&ctx) {
            ed.request(PendingAction::Restart);
        }
        ed.game.frame_no += 1;
        let frame_start = Instant::now();
        // The CPU governor's frame hint (src/android/perf_hint.rs): the
        // last frame's time on the CPU (eframe's, without the wait for the
        // display) against this frame's budget, the display's period or
        // the Editor frame limit's (`pace_frame`, which sleeps for it
        // where vsync is faster).
        #[cfg(target_os = "android")]
        if let Some(cpu) = self.cpu_usage {
            let hz = if self.refresh_hz > 1.0 { self.refresh_hz } else { 60.0 };
            // (the budget is the frame limit's period when that is longer)
            let hz = rustudio_core::rustudio_settings::fps_limit().map_or(hz, |cap| cap.min(hz));
            crate::android::perf_hint::frame(
                std::time::Duration::from_secs_f32(cpu.max(0.0)),
                std::time::Duration::from_secs_f32(1.0 / hz.max(1.0)),
            );
        }
        let causes = ctx.repaint_causes();
        let now = Instant::now();
        let dt = now.duration_since(ed.last_frame).as_secs_f32();
        ed.last_frame = now;
        rustudio_core::load_trace::frame(ed.game.frame_no.saturating_sub(1) as u64, dt as f64 * 1000.0);
        // The status bar's fps counts the frames that followed the frame
        // before at once — asked for by it, or brought by input — not the
        // ones a timer woke the editor for (a poll, a tooltip, a sway) or
        // the first after an idle pause: eframe draws on request, and on a
        // phone, where no pointer hovers, a still editor's few frames a
        // second are its idleness, not its frame rate.
        // (a frame longer than this follows an idle pause, not slow drawing)
        const IDLE_PAUSE_S: f32 = 0.1;
        let timed = !ctx.requested_repaint_last_pass() && !causes.is_empty();
        if dt > 0.0 && dt < IDLE_PAUSE_S && !timed {
            ed.fps = ed.fps * 0.9 + (1.0 / dt) * 0.1;
        }
        ed.hud = HudStats::default();
        ed.game.poll_assets();
        ed.flush_output_changes();
        self.watchdog.stage("asset polling", "Play tick: scripts/physics");
        if ed.game.play.is_some() {
            let step = ed.game.play_step(dt);
            if ed.game.fixed_dt.is_some() && ed.harness.run.play_started.is_some() {
                *ed.harness.run.play_game_time.get_or_insert(0.0) += step;
            }
            let _p = rustudio_core::bench::scope("play_tick");
            ed.play_tick(&ctx, step);
        }
        self.watchdog.stage("Play tick", "plugins");
        if ctx.input(|i| i.viewport().visible() != Some(false)) {
            if !std::mem::take(&mut self.plugins_ticked_between_frames) {
                Self::tick_plugins(ed, &mut self.last_plugin_tick, now);
                ed.edit_audio_tick();
            }
            Self::schedule_plugin_repaint(ed, &ctx);
        }
        self.watchdog.stage("plugins", "project and scene synchronization");
        ed.project_tick();
        ed.file_sync_tick();
        ed.template_preview_tick();
        ed.plugins_tick();
        if ed.builtin_tools.use_native(rustudio_core::rustudio_settings::BuiltinTool::Toolbox) {
            ed.toolbox_tick(&ctx);
        }
        ed.asset_manager_tick(&ctx);
        if ed.builtin_tools.use_native(rustudio_core::rustudio_settings::BuiltinTool::Toolbox) {
            ed.toolbox_harness_tick();
        }
        if self.ed.harness.run.no_ui {
            self.no_ui_frame(ui, &ctx, frame_start, dt);
            return;
        }
        if self.play_fills_screen() {
            self.play_full_frame(ui, &ctx, frame_start, dt);
            return;
        }
        let ed = &mut self.ed;
        // A folder with several project files: which one to open.
        if let Some((dir, files)) = ed.project_choice.clone() {
            let mut pick = None;
            let mut open = true;
            egui::Window::new("Open project")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(&ctx, |ui| {
                    ui.label(format!("{} has several project files:", dir.display()));
                    for f in &files {
                        let n = crate::project::name_of(f).to_string();
                        let label = if n == "default.project.json" { format!("{n} (default)") } else { n };
                        if ui.button(label).clicked() {
                            pick = Some(f.clone());
                        }
                    }
                });
            if let Some(f) = pick {
                ed.project_choice = None;
                ed.open_project(&f);
            } else if !open {
                ed.project_choice = None;
            }
        }
        ed.project_windows(&ctx);
        ed.sync_explorer_to_selection();
        {
            let _p = rustudio_core::bench::scope("scene_sync");
            ed.game.scene.sync(&mut ed.game.doc);
        }

        if ctx.input(|i| i.viewport().close_requested())
            && ed.leave_check() != crate::cloud_ui::live_status::Leave::Free
            && ed.harness.run.screenshot.is_none()
            && ed.harness.selftest.is_none()
            && ed.harness.run.bench.is_none()
            && ed.harness.ui_shots.is_none()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ed.confirm_discard = Some(PendingAction::Quit);
        }
        if ctx
            .data_mut(|d| d.remove_temp::<bool>(egui::Id::new("reset_dock")))
            .unwrap_or(false)
        {
            // Reset Layout: panel windows close, their tabs go back too.
            let popped: Vec<Tab> = self.panels.popped_tabs().collect();
            self.panels = crate::panels::Panels::default();
            self.dock = default_dock_for(self.ed.builtin_tools);
            for t in popped {
                if self.dock.find_tab(&t).is_none()
                    && native_tab_enabled(t, self.ed.builtin_tools)
                    && !matches!(t, Tab::AvatarSettings | Tab::TerrainEditor | Tab::Toolbox | Tab::AssetManager | Tab::FileSync)
                {
                    crate::panels::place(&mut self.dock, t, &self.panels.homes);
                }
            }
        }

        self.reset_document_tabs();
        let ed = &mut self.ed;
        if (ctx.zoom_factor() - ed.ui.zoom).abs() > 1e-3 {
            ctx.set_zoom_factor(ed.ui.zoom);
        }
        // The Start Page replaces the editor's chrome until something opens.
        if ed.start_page_ui(ui) {
            ed.confirm_ui(&ctx);
            ed.account_window(&ctx);
            ed.cloud_windows(&ctx);
            ed.mcp_windows(&ctx);
            ed.file_sync_popover_ui(&ctx);
            ed.mcp_tick(&ctx);
            if ed.harness.run.screenshot.is_some() {
                ed.harness_tick(&ctx);
                ctx.request_repaint();
            }
            if ed.game.account.busy() || ed.game.assets.busy() {
                ctx.request_repaint();
            }
            // The phone's text handling, as at the end of the editor's
            // frame below: the Start Page's fields (the search box, a new
            // place's name, the sign-in) had no soft keyboard, no Cut /
            // Copy / Paste bar or menu and no copy to the system clipboard
            // — this return came before them.
            #[cfg(target_os = "android")]
            self.text_end_frame(&ctx);
            self.watchdog.complete();
            return;
        }
        self.watchdog.stage("scene synchronization", "dock UI and GPU viewport preparation");
        ed.shortcuts(&ctx);

        {
            let _p = rustudio_core::bench::scope("editor_chrome");
            // Android, the typing layout (keyboard.rs `typing_layout`): the
            // soft keyboard is up for an editor field, and the menu bar,
            // the test bar and the ribbon leave their height to the dock
            // — the script editor, the Properties cell — until it goes.
            #[cfg(target_os = "android")]
            let chrome = !self.keyboard.typing_layout(&ctx);
            #[cfg(not(target_os = "android"))]
            let chrome = true;
            if chrome {
                egui::Panel::top("menu")
                    .frame(
                        egui::Frame::new()
                            .fill(theme::MENU_BAR)
                            .inner_margin(egui::Margin::symmetric(6, 2)),
                    )
                    .show(ui, |ui| {
                        let _p = rustudio_core::bench::scope("menu_bar");
                        ed.studio_menu_bar(ui);
                    });
                egui::Panel::top("testbar")
                    .frame(
                        egui::Frame::new()
                            .fill(theme::PANEL)
                            .inner_margin(egui::Margin::symmetric(8, 4)),
                    )
                    .show(ui, |ui| {
                        let _p = rustudio_core::bench::scope("test_toolbar");
                        ed.studio_top_row(ui);
                    });
                egui::Panel::top("toolbar")
                    .frame(
                        egui::Frame::new()
                            .fill(theme::PANEL)
                            .inner_margin(egui::Margin::symmetric(6, 4)),
                    )
                    .show(ui, |ui| {
                        let _p = rustudio_core::bench::scope("ribbon");
                        ed.studio_ribbon(ui);
                    });
            }
            // Android: no status bar while the soft keyboard is up. It
            // carries nothing a typist needs, and it sat over the text
            // being typed — a game TextBox the view had slid up to show
            // (2026-11-25 report: the Team Create / camera strip over the
            // box) — or took a line off the few a landscape phone has left
            // above the keyboard for the panels.
            #[cfg(target_os = "android")]
            let status_bar = self.keyboard.room(&ctx) <= 0.0;
            #[cfg(not(target_os = "android"))]
            let status_bar = true;
            if status_bar {
                egui::Panel::bottom("status")
                    .frame(
                        egui::Frame::new()
                            .fill(theme::PANEL_DARK)
                            .inner_margin(egui::Margin::symmetric(8, 3)),
                    )
                    .show(ui, |ui| ed.status_bar(ui));
            }
        }

        // Editor::start_run requests this when a playtest begins.
        if std::mem::take(&mut self.ed.switch_to_viewport) {
            if let Some(loc) = self.dock.find_tab(&Tab::Viewport) {
                let _ = self.dock.set_active_tab(loc);
            }
        }

        let _dock_scope = rustudio_core::bench::scope("dock_panels");
        let windows: Vec<(String, String)> = self.panels.popped.iter().map(|w| (w.key.clone(), w.title())).collect();
        let mut dock_rect = egui::Rect::NOTHING;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG))
            .show(ui, |ui| {
                dock_rect = ui.max_rect();
                self.ed.ui.floating_tabs = self
                    .dock
                    .iter_surfaces_indexed()
                    .filter(|(i, _)| !i.is_main())
                    .flat_map(|(_, s)| s.iter_all_tabs().map(|(_, t)| *t).collect::<Vec<_>>())
                    .collect();
                DockArea::new(&mut self.dock)
                    .style(theme::dock_style(ui.style()))
                    .show_add_buttons(false)
                    .show_leaf_collapse_buttons(false)
                    .show_leaf_close_all_buttons(false)
                    .show_inside(ui, &mut Viewer { ed: &mut self.ed, window: None, windows: &windows });
            });
        drop(_dock_scope);
        self.reset_document_tabs();
        self.panel_windows(&ctx, &windows, dock_rect);
        self.reset_document_tabs();
        // Window > Explorer / Properties / Output / Avatar Settings toggles
        // (in a panel window too), new panels where Studio docks them
        // (src/panels.rs, never over the 3D view), and which panels are
        // open (their checkmarks next frame).
        use crate::panels::Location;
        // `toolbox:ui:width:<px>` (Toolbox layout checks): the side-by-side
        // split holding the Toolbox gives it that width.
        if let Some(px) = self.ed.toolbox.ui.want_width.take() {
            // The split changes after this frame's draw: a layout report
            // waits for the next one (src/toolbox/glue.rs).
            self.ed.toolbox.ui.action_seq += 1;
            if let Some(path) = self.dock.find_tab(&Tab::Toolbox) {
                let mut node = path.node;
                let tree = &mut self.dock[path.surface];
                while let Some(parent) = node.parent() {
                    if let egui_dock::Node::Horizontal(sp) = &mut tree[parent] {
                        let w = sp.rect.width();
                        if w > px {
                            let f = (px / w).clamp(0.05, 0.95);
                            sp.fraction = if node.is_left() { f } else { 1.0 - f };
                        }
                        break;
                    }
                    node = parent;
                }
            }
        }
        if let Some(on) = rustudio_engine::gui::share().microprofiler_req.take() {
            self.ed.ui.panel_requests.push((Tab::Profiler, Some(on)));
        }
        for (tab, want) in std::mem::take(&mut self.ed.ui.panel_requests) {
            if !native_tab_enabled(tab, self.ed.builtin_tools) {
                continue;
            }
            let found = self.panels.locate(&self.dock, &tab);
            let open = want.unwrap_or(found.is_none());
            match (found, open) {
                (Some(_), false) => {
                    self.panels.take(&mut self.dock, &tab);
                }
                (Some(Location::Main(at)), true) => {
                    let _ = self.dock.set_active_tab(at);
                }
                (Some(Location::Popped(i, at)), true) => {
                    let w = &mut self.panels.popped[i];
                    let _ = w.dock.set_active_tab(at);
                    w.focus = true;
                }
                (None, true) => {
                    if self.panels.restore_window(&self.dock, tab) {
                        // Restored the hidden panel in its original OS window.
                    } else if let Some(size) = crate::plugin_host::widgets_ui::float_size(&tab) {
                        // A plugin's Float widget: a floating window of the dock
                        // (it can be docked by its tab), centred on the main
                        // window as Studio's (src/plugin_host/widgets_ui.rs).
                        let size = egui::vec2(size[0].max(60.0), size[1].max(40.0)).min(dock_rect.size() - egui::vec2(8.0, 8.0));
                        let at = dock_rect.center() - size / 2.0;
                        let surface = self.dock.add_window(vec![tab]);
                        if let Some(w) = self.dock.get_window_state_mut(surface) {
                            w.set_position(at).set_size(size);
                        }
                    } else if tab == Tab::Profiler && std::env::var_os("RUSTUDIO_PROFILER_DOCKED").is_some() {
                        // (checks: docked under the 3D view so a window shot shows both)
                        crate::panels::place(&mut self.dock, tab, &self.panels.homes);
                    } else if tab == Tab::Profiler {
                        // (the mockup's size, smaller on a small screen)
                        let size = egui::vec2(1440.0, 900.0).min(ctx.content_rect().size() * 0.92);
                        self.panels.pop_out(tab, [size.x, size.y], None);
                    } else if crate::panels::placement(tab.key()).floats {
                        self.panels.pop_out(tab, [420.0, 520.0], None);
                    } else {
                        crate::panels::place(&mut self.dock, tab, &self.panels.homes);
                    }
                }
                (None, false) => {}
            }
        }
        {
            crate::plugin_host::widgets_ui::set_dock_area(dock_rect);
            let (dock, panels) = (&self.dock, &self.panels);
            self.ed.plugin_ui_note_open(|t| panels.locate(dock, t).is_some());
            // Float widgets' windows sized so their content is Studio's client area.
            for (tab, size) in self.ed.plugin_float_fixes() {
                if let Some(path) = self.dock.find_tab(&tab) {
                    if let Some(w) = self.dock.get_window_state_mut(path.surface) {
                        w.set_size(size);
                    }
                }
            }
            self.ed.plugin_drag_tick(&ctx);
            self.ed.plugin_widget_shots(&ctx);
        }
        self.ed.ui.panels_open = [
            (Tab::Explorer, "Explorer"),
            (Tab::Properties, "Properties"),
            (Tab::Output, "Output"),
            (Tab::AvatarSettings, "Avatar Settings"),
            (Tab::TerrainEditor, "Terrain Editor"),
            (Tab::AnimationEditor, "Animation Editor"),
            (Tab::Toolbox, "Toolbox"),
            (Tab::AssetManager, "Asset Manager"),
            (Tab::FileSync, "File Sync"),
            (Tab::Profiler, "MicroProfiler"),
        ]
        .into_iter()
        .filter(|(t, _)| self.panels.locate(&self.dock, t).is_some())
        .map(|(_, n)| n)
        .collect();
        // Closing the MicroProfiler stops recording.
        if self.ed.profiler.is_some() && !self.ed.ui.panels_open.contains("MicroProfiler") {
            self.ed.profiler = None;
        }
        self.ed.anim_editor_check_closed();
        self.ed.anim_editor_sync_selection();
        let ed = &mut self.ed;
        ed.open_script_tabs(&mut self.dock, &mut self.panels);
        crate::panels::record_homes(&self.dock, &mut self.panels.homes);
        self.ed.ui.popped = self.panels.popped.iter().map(|w| (w.key.clone(), w.tabs())).collect();
        // A script tab in front of the 3D view (plugin toolbar buttons
        // without ClickableWhenViewportHidden grey out: ui_ribbon.rs).
        self.ed.ui.viewport_hidden = self.dock.find_main_surface_tab(&Tab::Viewport).is_some_and(|(n, t)| {
            matches!(&self.dock.main_surface()[n], egui_dock::Node::Leaf(l) if l.active != t)
        });
        self.ed.ui.viewport_group = self
            .dock
            .find_main_surface_tab(&Tab::Viewport)
            .and_then(|(n, _)| self.dock.main_surface()[n].tabs().map(|t| t.to_vec()))
            .unwrap_or_default();
        if let Some(s) = self.layout.as_mut() {
            s.tick(&ctx, &self.dock, &self.panels, &self.ed);
        }
        let ed = &mut self.ed;
        ed.insert_popup_ui(&ctx);
        ed.about_ui(&ctx);
        ed.plugins_ui(&ctx);
        ed.graphics_settings_ui(&ctx);
        ed.studio_settings_ui(&ctx);
        ed.user_screenshot_tick(&ctx);
        rustudio_engine::runtime::domains::assets_cloud::capture::frame_tick(&ctx, ed.game.viewport_rect);
        ed.confirm_ui(&ctx);
        ed.account_window(&ctx);
        if ed.builtin_tools.use_native(rustudio_core::rustudio_settings::BuiltinTool::RigBuilder) {
            ed.character_builder_window(&ctx);
        }
        ed.cloud_windows(&ctx);
        ed.mcp_windows(&ctx);
        ed.file_sync_popover_ui(&ctx);
        self.watchdog.stage("dock UI", "harness and screenshot readback");
        self.finish_frame(&ctx, frame_start, dt);
        // The soft keyboard follows what there is to type into; egui-winit
        // gets no IME output (src/android/keyboard.rs). Over the keyboard,
        // the text bar (src/android/text_bar.rs).
        #[cfg(target_os = "android")]
        self.keyboard_end_frame(&ctx);
        // The memory, as it grows (src/android/memory.rs).
        #[cfg(target_os = "android")]
        {
            let doing = match (&self.ed.game.doc.path, self.ed.game.play.is_some()) {
                (Some(p), playing) => format!(
                    "{} {}",
                    if playing { "playing" } else { "editing" },
                    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                ),
                (None, _) => String::new(),
            };
            let game = &self.ed.game;
            crate::android::memory::tick(&doing, || {
                let mut parts = vec![format!("DataModel {} instances", game.doc.dom.descendants().count())];
                if let Some(r) = game.renderer.as_ref() {
                    parts.push(r.memory_summary());
                }
                if let Some(rs) = game.render_state.as_ref() {
                    let gpu = crate::android::memory::gpu_line(&rs.device);
                    if !gpu.is_empty() {
                        parts.push(gpu);
                    }
                }
                parts.join("; ")
            });
        }
    }
}

impl RustudioApp {
    /// The end of every frame, the editor's and the no-UI mode's: the
    /// harness (shots, input, --luau), control, --bench, and repaints.
    fn finish_frame(&mut self, ctx: &egui::Context, frame_start: Instant, dt: f32) {
        let ctx = ctx.clone();
        let ed = &mut self.ed;
        ed.harness_tick(&ctx);
        #[cfg(unix)]
        ed.control_tick(&ctx);
        ed.mcp_tick(&ctx);
        ed.edit_audio_tick();
        ed.ui_shots_tick(&ctx);
        let work_ms = frame_start.elapsed().as_secs_f32() * 1000.0;
        ed.last_work_ms = work_ms;
        if rustudio_core::profiler::active() {
            rustudio_player::profiler_counters::sample(&ed.game);
            thread_local! { static TC: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((0, 0)) }; }
            TC.with(|tc| {
                let (mut s, mut r) = tc.get();
                use std::sync::atomic::Ordering::Relaxed;
                use rustudio_core::profiler::Cat;
                rustudio_player::profiler_counters::rate_from_total("Team Create sent", Cat::Network, rustudio_net::rtcio::BYTES_SENT.load(Relaxed), &mut s);
                rustudio_player::profiler_counters::rate_from_total("Team Create received", Cat::Network, rustudio_net::rtcio::BYTES_RECEIVED.load(Relaxed), &mut r);
                tc.set((s, r));
            });
        }
        // RUSTUDIO_GPU_TIMING: resolve this frame's GPU timestamps, collect finished ones.
        if let Some(rs) = ed.game.render_state.as_ref() {
            rustudio_render::gpu_timing::end_frame(&rs.device, &rs.queue);
        }
        if ed.harness.run.bench_frame(&mut ed.game, &ctx, work_ms, dt) {
            ed.finish_run(&ctx, "bench");
        }
        if ed.drag.is_some()
            || ed.game.play.is_some()
            || (ed.game.play.is_none() && ed.game.edit_audio.has_active())
            || ed.game.assets.busy()
            || ed.game.textures.decoding()
            || ctx.input(|i| i.pointer.any_down() || !i.keys_down.is_empty())
            || ed.harness.run.screenshot.is_some()
            || ed.harness.selftest.is_some()
            || ed.game.account.busy()
        {
            ctx.request_repaint();
        }
        // Text copied this frame goes to the system clipboard too; a long
        // press on a text field has its Cut / Copy / Paste menu.
        #[cfg(target_os = "android")]
        {
            crate::android::clipboard::after_frame(&ctx);
            self.text_menu.end_frame(&ctx);
        }
        self.watchdog.complete();
    }

    /// View > Play Fills the Screen (on by default on a phone or tablet):
    /// while a game is played, the game alone over the whole window, at
    /// the display's own scale, as Roblox's app shows it — the menu bar,
    /// ribbon, status bar and the other panels come back when it ends.
    /// A phone's Studio is zoomed out to fit the ribbon (android::touch
    /// auto_zoom), which left a game in the dock's 3D view a canvas a
    /// third of the screen's, in units a fifth smaller than Roblox's
    /// density-independent pixels: LBG's PlayerModule and its own Dash /
    /// M1 / Block / Shiftlock buttons, laid out from the bottom up in
    /// those units, ran off the top of the view (the same phone gives
    /// Roblox a 886 × 411 unit canvas: the phone layout, 70-unit jump
    /// button, every button on the screen).
    fn play_fills_screen(&self) -> bool {
        self.ed.ui.play_full && !self.ed.play_full_exit && self.ed.game.play.is_some()
    }

    fn play_full_frame(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, frame_start: Instant, dt: f32) {
        let ed = &mut self.ed;
        {
            let _p = rustudio_core::bench::scope("scene_sync");
            ed.game.scene.sync(&mut ed.game.doc);
        }
        // The display's scale: a point is a density-independent pixel,
        // Roblox's GUI unit on Android (the editor's zoom returns with its
        // chrome: `frame` sets it back every frame).
        if (ctx.zoom_factor() - 1.0).abs() > 1e-3 {
            ctx.set_zoom_factor(1.0);
        }
        self.watchdog.stage("scene synchronization", "3D view");
        // (Shift+F5 stops, and the game's keys)
        ed.shortcuts(ctx);
        // (the dock's 3D view tab is the one drawn here: the same state)
        self.ed.ui.viewport_hidden = false;
        let ed = &mut self.ed;
        let mut stop = false;
        let mut dock = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG))
            .show(ui, |ui| {
                let _p = rustudio_core::bench::scope("viewport");
                ed.viewport_ui(ui);
                stop = ed.play_stop_button(ui);
                dock = ed.play_full_button(ui, true);
            });
        if stop {
            ed.stop_play();
        } else if dock {
            // Back to Studio around the game: the next frame is the editor's
            // (`frame`), the game in the dock's 3D view until Fill the Screen.
            ed.play_full_exit = true;
            ctx.request_repaint();
        }
        ed.user_screenshot_tick(ctx);
        rustudio_engine::runtime::domains::assets_cloud::capture::frame_tick(ctx, ed.game.viewport_rect);
        ed.confirm_ui(ctx);
        ed.account_window(ctx);
        ed.cloud_windows(ctx);
        self.watchdog.stage("3D view", "harness and screenshot readback");
        self.finish_frame(ctx, frame_start, dt);
        // The soft keyboard over a game's TextBox (the chat), as in `frame`.
        #[cfg(target_os = "android")]
        {
            self.keyboard_end_frame(ctx);
            let doing = match &self.ed.game.doc.path {
                Some(p) => format!("playing {}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                None => String::new(),
            };
            let game = &self.ed.game;
            crate::android::memory::tick(&doing, || {
                let mut parts = vec![format!("DataModel {} instances", game.doc.dom.descendants().count())];
                if let Some(r) = game.renderer.as_ref() {
                    parts.push(r.memory_summary());
                }
                if let Some(rs) = game.render_state.as_ref() {
                    let gpu = crate::android::memory::gpu_line(&rs.device);
                    if !gpu.is_empty() {
                        parts.push(gpu);
                    }
                }
                parts.join("; ")
            });
            // As in `frame`: a copy made while the game fills the screen
            // (the bar's Copy of a TextBox, the game's own Ctrl+C) reaches
            // the system clipboard too — it didn't, so a text copied in
            // Play pasted nowhere outside the editor.
            crate::android::clipboard::after_frame(ctx);
            self.text_menu.end_frame(ctx);
        }
    }

    /// `--no-ui`: the 3D view alone over the whole window, nothing of the
    /// editor built or drawn (no menus, ribbon, dock, Explorer, Properties,
    /// Output, status bar, panel windows or editor shortcuts). In Edit state
    /// the document's view through the edit camera; with --play the game,
    /// with every key going to it (game_keys).
    fn no_ui_frame(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, frame_start: Instant, dt: f32) {
        let ed = &mut self.ed;
        {
            let _p = rustudio_core::bench::scope("scene_sync");
            ed.game.scene.sync(&mut ed.game.doc);
        }
        if (ctx.zoom_factor() - ed.ui.zoom).abs() > 1e-3 {
            ctx.set_zoom_factor(ed.ui.zoom);
        }
        // --open-roblox / --team-create once the session is checked (no
        // picker or join windows are drawn).
        ed.cloud_poll(ctx);
        self.watchdog.stage("scene synchronization", "3D view");
        ed.game_keys(ctx);
        // (Studio menu actions need the editor: --action does nothing here,
        // except the edits that need no panel: Ctrl+Z / Ctrl+Y / Delete …,
        // which the plugin checks drive.)
        for a in std::mem::take(&mut ed.harness.ui_due) {
            // The ribbon's tools still switch (a plugin's Activate /
            // Deactivation against them: src/plugin_host/ui_ribbon.rs).
            if matches!(
                a.as_str(),
                "undo" | "redo" | "delete" | "duplicate" | "group" | "ungroup" | "tool_select" | "tool_move" | "tool_scale" | "tool_rotate"
            ) {
                ed.run_action(&a);
                continue;
            }
            // The Toolbox's deep links work without the panel (headless checks).
            if a.starts_with("toolbox:") {
                ed.toolbox_action(&a);
            } else {
                eprintln!("--action {a}: no editor in --no-ui");
            }
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG))
            .show(ui, |ui| {
                let _p = rustudio_core::bench::scope("viewport");
                ed.viewport_ui(ui);
            });
        // Open plugin widgets as fixed panels over the view (no dock here).
        if ed.game.play.is_none() {
            ed.plugin_noui_widgets(ctx);
        }
        ed.user_screenshot_tick(ctx);
        rustudio_engine::runtime::domains::assets_cloud::capture::frame_tick(ctx, ed.game.viewport_rect);
        self.watchdog.stage("3D view", "harness and screenshot readback");
        self.finish_frame(ctx, frame_start, dt);
    }
}

impl Editor {
    /// The way out of a game that fills the screen (`play_fills_screen`):
    /// a Stop button at the top right of the view, where Roblox's own top
    /// bar keeps nothing (its Roblox and chat buttons are at the left,
    /// play_game.rs `top_bar`; a game's TopbarPlus icons follow them). The
    /// ribbon's Stop and Shift+F5 are the editor's, which this mode hides,
    /// and a phone has no keyboard: Back opens Roblox's menu, whose Leave
    /// ends the session too, but a Studio tester looks for Stop. Drawn as
    /// one of the bar's IconButtons (iconBase under a stop square), after
    /// the view so it is over the game's GUIs and the menu's shield; a
    /// finger landing on it stays egui's (`Zones::ui`, src/touch_input.rs).
    /// Returns whether it was tapped.
    pub fn play_stop_button(&mut self, ui: &mut egui::Ui) -> bool {
        use egui::{Color32, Rect, Stroke};
        const SQUARE: f32 = 16.0;
        let mut pressed = self.stop_pressed;
        let hit = self.play_top_right_button(ui, 0, "play_full_stop", "Stop (Shift+F5)", &mut pressed, |painter, r| {
            let square = Rect::from_center_size(r.center(), egui::vec2(SQUARE, SQUARE));
            painter.rect_filled(square, 2.0, Color32::from_rgb(242, 72, 72));
            painter.rect_stroke(square, 2.0, Stroke::new(1.0, Color32::from_white_alpha(200)), egui::StrokeKind::Outside);
        });
        self.stop_pressed = pressed;
        hit
    }

    /// Beside Stop, the way between the two places a played game is shown
    /// (`play_full_exit`): over a filled screen (`full`) a Studio button —
    /// a window glyph — puts the game back in the dock's 3D view, with the
    /// Output, Explorer, plugin widgets and the ribbon's Stop around it
    /// (the phone's Studio around a game, which Play Fills the Screen hid);
    /// in that view (View > Play Fills the Screen on, `viewport_ui`) a Fill
    /// the Screen button — four corners — takes the game back over the
    /// window. Returns whether it was tapped.
    pub fn play_full_button(&mut self, ui: &mut egui::Ui, full: bool) -> bool {
        use egui::{Color32, Pos2, Rect, Stroke};
        let mut pressed = self.full_pressed;
        let (id, tip) = if full {
            ("play_full_studio", "Studio around the game (Output, Explorer, plugins; View > Play Fills the Screen)")
        } else {
            ("play_full_fill", "Fill the screen with the game")
        };
        // (the Stop button's slot in a filled screen; the dock's view has
        // the ribbon's Stop, so this one takes the corner)
        let slot = if full { 1 } else { 0 };
        let hit = self.play_top_right_button(ui, slot, id, tip, &mut pressed, |painter, r| {
            let stroke = Stroke::new(2.0, Color32::from_white_alpha(230));
            let c = r.center();
            if full {
                // A window: its frame and title bar.
                let w = Rect::from_center_size(c, egui::vec2(18.0, 15.0));
                painter.rect_stroke(w, 2.0, stroke, egui::StrokeKind::Inside);
                painter.rect_filled(Rect::from_min_max(w.min, Pos2::new(w.max.x, w.min.y + 4.0)), 1.0, Color32::from_white_alpha(230));
            } else {
                // Four corners of a screen.
                let h = egui::vec2(9.0, 8.0);
                let arm = 5.0;
                for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    let corner = Pos2::new(c.x + sx * h.x, c.y + sy * h.y);
                    painter.line_segment([corner, Pos2::new(corner.x - sx * arm, corner.y)], stroke);
                    painter.line_segment([corner, Pos2::new(corner.x, corner.y - sy * arm)], stroke);
                }
            }
        });
        self.full_pressed = pressed;
        hit
    }

    /// One of the top-right buttons over a played game, in `slot` (0 at the
    /// right edge, 1 beside it …): drawn as one of Roblox's top-bar
    /// IconButtons (iconBase under `glyph`), after the view so it is over
    /// the game's GUIs and the menu's shield; a finger landing on it stays
    /// egui's (`Zones::ui`, src/touch_input.rs). `pressed` is the button's
    /// own press memory. Returns whether it was tapped.
    fn play_top_right_button(
        &mut self,
        ui: &mut egui::Ui,
        slot: usize,
        id: &str,
        tip: &str,
        pressed: &mut bool,
        glyph: impl FnOnce(&egui::Painter, egui::Rect),
    ) -> bool {
        use egui::{Color32, Pos2, Rect};
        if !self.game.is_playing() {
            return false;
        }
        let rect = self.game.viewport_rect;
        if !rect.is_positive() || rect.width() < 160.0 {
            return false;
        }
        const BAR_H: f32 = mech::GUI_TOP_INSET; // TopBarHeight
        const BUTTON: f32 = 44.0; // TopBarButtonHeight
        const BUTTON_PAD: f32 = 2.0; // TopBarButtonPadding
        const GAP: f32 = 12.0; // between the bar's buttons (TopBar Constants Padding)
        const TOP_MARGIN: f32 = BAR_H - BUTTON - 2.0 * BUTTON_PAD; // TopBarTopMargin
        const SIDE: f32 = 16.0; // ScreenSideOffset
        let y = rect.min.y + TOP_MARGIN + (BAR_H - TOP_MARGIN - BUTTON) * 0.5;
        let x = rect.max.x - SIDE - BUTTON - slot as f32 * (BUTTON + GAP);
        let r = Rect::from_min_size(Pos2::new(x, y), egui::vec2(BUTTON, BUTTON));
        self.game.touch.zones.ui.push(r);
        let resp = ui.interact(r, ui.id().with(id), egui::Sense::click()).on_hover_text(tip);
        let painter = ui.painter_at(rect);
        // (iconBase.png: black at 0.5, corners 8 of its 32)
        painter.rect_filled(r, 8.0, Color32::from_black_alpha(128));
        glyph(&painter, r);
        // IconButton's StateOverlay: black at 0.3 while pressed, white at 0.1 while hovered.
        let down_on = resp.is_pointer_button_down_on();
        if down_on {
            painter.rect_filled(r, 8.0, Color32::from_black_alpha(77));
        } else if resp.hovered() {
            painter.rect_filled(r, 8.0, Color32::from_white_alpha(26));
        }
        // A click, or a press on it let go over it: egui's click needs the
        // release within 0.8 s and 6 points of the press, and a game's long
        // frame (a phone loading one) or a finger that rests on the button
        // breaks that — the buttons that leave the game have to work
        // regardless (the harness's `--ui-click` lost Stop that way at 1 fps).
        let (released, at) = ui.input(|i| (i.pointer.any_released(), i.pointer.interact_pos()));
        let let_go = released && *pressed && at.is_some_and(|p| r.contains(p));
        *pressed = down_on || (*pressed && !released && ui.input(|i| i.pointer.any_down()));
        if resp.clicked() || let_go {
            *pressed = false;
            return true;
        }
        false
    }

    pub fn finish_run(&mut self, ctx: &egui::Context, reason: &str) {
        if self.harness.run.finished {
            return;
        }
        crate::plugin_host::widgets_ui::on_exit();
        let extras = self.harness.run.result.as_ref().map(|_| [
            ("plugins", self.plugins.result_json()),
            ("plugin_toolbars", self.plugin_ui_json()),
            ("toolbox", self.toolbox.result_json()),
        ]);
        self.harness.run.finish_run(&self.game, ctx, reason, extras.into_iter().flatten());
    }
}

// ------------------------------------------------------------------ harness

impl Editor {
    fn harness_tick(&mut self, ctx: &egui::Context) {
        if self.harness.run.start_play && self.game.viewport_rect.is_positive() && self.game.frame_no >= 2 && !self.place_loading() {
            self.harness.run.start_play = false;
            self.start_play(rustudio_engine::runtime::RunMode::Play);
            self.harness.run.play_started = Some(rustudio_core::sim_clock::now());
        }
        if let Some(reason) = self.harness.run.tick_before_shot(&mut self.game, self.last_work_ms) {
            self.finish_run(ctx, reason);
            return;
        }
        if self.harness.run.finished {
            return;
        }
        if let Some(mut t) = self.harness.selftest.take() {
            let done = t.tick(self, ctx);
            self.harness.selftest = Some(t);
            if done {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        let widgets = if self.harness.run.no_ui {
            Vec::new()
        } else {
            self.plugin_widget_rects()
        };
        let reason = self.harness.run.tick_shots(&self.game, ctx, |image, path, written| {
            let ppp = ctx.pixels_per_point();
            for (key, r) in widgets {
                let safe: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
                let wp = path.with_extension(format!("widget-{safe}.png"));
                if save_png(&crate::plugin_host::widgets_ui::crop(image, r, ppp), &wp).is_ok() {
                    written.push(wp);
                }
            }
        });
        if let Some(reason) = reason {
            self.finish_run(ctx, reason);
        }

    }
}

impl Editor {
    pub fn harness_inject(&mut self, raw: &mut egui::RawInput) {
        let mut input = std::mem::take(&mut self.harness.run.input);
        input.inject(self.game.viewport_rect, self.game.lock_center_held(), self.harness.run.now(), raw, |name, raw| {
            if let Some(f) = name.strip_prefix("finger:") {
                // finger:ID:start|move|end:X,Y (view coordinates): a finger
                // of the game's (--finger, --tap; touch_input::harness_finger).
                let mut it = f.splitn(3, ':');
                let (id, phase, xy) = (it.next(), it.next(), it.next());
                let id = id.and_then(|s| s.parse::<u64>().ok());
                let phase = match phase {
                    Some("start") => Some(egui::TouchPhase::Start),
                    Some("move") => Some(egui::TouchPhase::Move),
                    Some("end") => Some(egui::TouchPhase::End),
                    Some("cancel") => Some(egui::TouchPhase::Cancel),
                    _ => None,
                };
                let xy = xy.and_then(|s| s.split_once(',')).and_then(|(x, y)| Some((x.parse::<f32>().ok()?, y.parse::<f32>().ok()?)));
                if let (Some(id), Some(phase), Some((x, y))) = (id, phase, xy) {
                    let pos = self.game.viewport_rect.min + egui::vec2(x, y);
                    self.game.touch.harness_finger(id, phase, pos);
                } else {
                    eprintln!("harness: bad finger action {name:?}");
                }
            } else if self.plugin_widget_action(&name, raw) {
                // plugin-widget:PLUGIN/WIDGET:CMD
            } else if self.harness_pointer(&name, raw) {
                // ptr:VERB:TARGET
            } else if let Some(ans) = name.strip_prefix("plugin-prompt:") {
                crate::plugin_host::prompts::answer_oldest(ans);
            } else if let Some(t) = crate::plugin_host::Trigger::parse(&name) {
                match t {
                    crate::plugin_host::Trigger::Button(tb, b) => self.plugin_harness_click(&tb, &b),
                    t => self.plugins.trigger(t),
                }
            } else {
                self.harness.ui_due.push(name);
            }
        });
        self.harness.run.input = input;
    }
}

/// The sample scene's parts, scripts and lights, into a place document
/// (`--demo`, and the fidelity corpus's demo place). Returns the Ball.
pub use crate::game::build_demo_parts;

/// The whole pixels the 3D view covers in a screenshot of the window: its
/// rectangle in points scaled by `ppp`, rounded inward, where an edge within
/// a hundredth of a pixel of a whole one counts as on it. With fractional
/// scaling (1.25) an edge at 143.2 points is pixel row 179 but comes out
/// 178.99997 in f32: truncating took row 178, the editor's frame (shots were
/// a pixel low), and plain inward rounding lost a row at the other edge.
/// `(x0, y0, w, h)`, clipped to `size`.
pub fn view_pixels(r: egui::Rect, ppp: f32, size: [usize; 2]) -> (usize, usize, usize, usize) {
    const EPS: f32 = 0.01;
    let start = |v: f32, n: usize| (((v * ppp) - EPS).ceil().max(0.0) as usize).min(n);
    let end = |v: f32, n: usize| (((v * ppp) + EPS).floor().max(0.0) as usize).min(n);
    let (x0, y0) = (start(r.min.x, size[0]), start(r.min.y, size[1]));
    let (x1, y1) = (end(r.max.x, size[0]), end(r.max.y, size[1]));
    (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
}

/// The selected parts for one (document, journal end, scene) state and
/// selection, and the ribbon's questions about them, answered once.
#[derive(Default)]
pub struct SelectionInfo {
    scene: Option<(u64, u64, usize)>,
    journal: u64,
    selection: Vec<Ref>,
    parts: std::rc::Rc<Vec<Ref>>,
    /// (property, every selected part has it true)
    all_true: Vec<(&'static str, bool)>,
    /// Some(shared material, if any) once asked.
    material: Option<Option<u32>>,
}

/// Studio's Insert Object / UI-tab default properties for GUI classes and UI
/// components, over the bare class-constructor / reflection defaults.
///
/// Studio's Insert Object defaults for GUI objects and UI components.
///
/// Called by `Editor::insert_class` so the Explorer `+` popup, Insert Object
/// window, Explorer context menu, Home > GUI, and Ribbon UI tab all apply the
/// exact same defaults. `Position` is intentionally untouched (`{0, 0}, {0, 0}`).
pub(crate) fn apply_insert_gui_defaults(class: &str, mut b: InstanceBuilder) -> InstanceBuilder {
    use rbx_dom_weak::types::{Color3, Content, Enum, Font, UDim, UDim2};
    const WHITE: Color3 = Color3::new(1.0, 1.0, 1.0);
    const BLACK: Color3 = Color3::new(0.0, 0.0, 0.0);
    const PLACEHOLDER_IMAGE: &str = "rbxasset://textures/ui/GuiImagePlaceholder.png";
    const SOURCE_SANS_FAMILY: &str = "rbxasset://fonts/families/SourceSansPro.json";

    if doc::is_a(class, "GuiObject") {
        // Every GuiObject (Frame, TextLabel/Button/Box, ImageLabel/Button,
        // ScrollingFrame, ViewportFrame, CanvasGroup, VideoFrame, …).
        b = b
            .with_property("Size", UDim2::new(UDim::new(0.0, 100), UDim::new(0.0, 100)))
            .with_property("BackgroundColor3", WHITE)
            .with_property("BorderColor3", BLACK)
            .with_property("BorderSizePixel", 0i32);

        if doc::is_a(class, "TextLabel")
            || doc::is_a(class, "TextButton")
            || doc::is_a(class, "TextBox")
        {
            // Font = Enum.Font.SourceSans (3) → FontFace SourceSansPro Regular;
            // FontSize = Enum.FontSize.Size14 (5) → TextSize 14.0;
            // Size = {0, 200}, {0, 50}; TextColor3 = [0, 0, 0].
            // Text stays at its class default ("Label" / "Button" / "TextBox").
            b = b
                .with_property("FontFace", Font::regular(SOURCE_SANS_FAMILY))
                .with_property("TextSize", 14.0f32)
                .with_property("Size", UDim2::new(UDim::new(0.0, 200), UDim::new(0.0, 50)))
                .with_property("TextColor3", BLACK);
        } else if doc::is_a(class, "ImageLabel") || doc::is_a(class, "ImageButton") {
            b = b.with_property("ImageContent", Content::from(PLACEHOLDER_IMAGE));
        } else if doc::is_a(class, "ScrollingFrame") {
            b = b
                .with_property("Active", true)
                .with_property("ScrollBarImageColor3", BLACK);
        } else if doc::is_a(class, "TextChannelWindow") {
            b = b.with_property("Size", UDim2::new(UDim::new(0.0, 250), UDim::new(0.0, 200)));
        }
        if doc::is_a(class, "InputActionLabel") {
            b = b
                .with_property("AutomaticSize", Enum::from_u32(1)) // Enum.AutomaticSize.X
                .with_property("Size", UDim2::new(UDim::new(0.0, 0), UDim::new(0.0, 36)));
        }
    } else if doc::is_a(class, "BillboardGui") {
        b = b
            .with_property("Size", UDim2::new(UDim::new(0.0, 200), UDim::new(0.0, 50)))
            .with_property("LightInfluence", 1.0f32)
            .with_property("ClipsDescendants", true)
            .with_property("Active", true);
    } else if doc::is_a(class, "SurfaceGui") {
        b = b
            .with_property("LightInfluence", 1.0f32)
            .with_property("ClipsDescendants", true)
            .with_property("SizingMode", Enum::from_u32(1)) // Enum.SurfaceGuiSizingMode.PixelsPerStud
            .with_property("MaxDistance", 1000.0f32);
    } else if doc::is_a(class, "UIGridStyleLayout") {
        // UIListLayout, UIGridLayout, UIPageLayout, UITableLayout.
        b = b.with_property("SortOrder", Enum::from_u32(2)); // Enum.SortOrder.LayoutOrder
    } else if doc::is_a(class, "UIShadow") {
        b = b
            .with_property("BlurRadius", UDim::new(0.0, 20))
            .with_property("Transparency", 0.5f32);
    }

    if doc::is_a(class, "LayerCollector") {
        // ScreenGui, SurfaceGui, BillboardGui, AdGui, …
        b = b.with_property("ZIndexBehavior", Enum::from_u32(1)); // Enum.ZIndexBehavior.Sibling
    }

    b
}

#[cfg(test)]
mod insert_gui_tests {
    use super::apply_insert_gui_defaults;
    use rbx_dom_weak::types::{Color3, ContentId, Enum, Font, UDim, UDim2, Variant};
    use rbx_dom_weak::InstanceBuilder;
    use rustudio_core::doc::{self, Document};

    fn insert(doc: &mut Document, parent: rbx_dom_weak::types::Ref, class: &str) -> rbx_dom_weak::types::Ref {
        let b = apply_insert_gui_defaults(
            class,
            InstanceBuilder::new(class).with_name(doc::default_instance_name(class)),
        );
        doc.insert(parent, b)
    }

    #[test]
    fn gui_insert_defaults_match_studio() {
        let mut doc = Document::new_place();
        let root = doc.dom.root_ref();
        let sg_svc = doc.find_child(root, "StarterGui").expect("StarterGui");

        // ScreenGui: LayerCollector gets ZIndexBehavior = Sibling (1).
        let sg = insert(&mut doc, sg_svc, "ScreenGui");
        assert_eq!(doc.prop(sg, "ZIndexBehavior"), Some(Variant::Enum(Enum::from_u32(1))));

        // Frame: 100x100 offset, white bg, black border, 0 border px, {0,0},{0,0} pos.
        let frame = insert(&mut doc, sg, "Frame");
        assert_eq!(doc.prop(frame, "Size"), Some(Variant::UDim2(UDim2::new(UDim::new(0.0, 100), UDim::new(0.0, 100)))));
        assert_eq!(doc.prop(frame, "Position"), Some(Variant::UDim2(UDim2::new(UDim::new(0.0, 0), UDim::new(0.0, 0)))));
        assert_eq!(doc.prop(frame, "BackgroundColor3"), Some(Variant::Color3(Color3::new(1.0, 1.0, 1.0))));
        assert_eq!(doc.prop(frame, "BorderColor3"), Some(Variant::Color3(Color3::new(0.0, 0.0, 0.0))));
        assert_eq!(doc.prop(frame, "BorderSizePixel"), Some(Variant::Int32(0)));
        assert_eq!(doc.prop(frame, "BackgroundTransparency"), Some(Variant::Float32(0.0)));
        assert_eq!(doc.prop(frame, "Active"), Some(Variant::Bool(false)));
        assert_eq!(doc.prop(frame, "LayoutOrder"), Some(Variant::Int32(0)));

        // ViewportFrame, CanvasGroup, VideoFrame inherit base GuiObject 100x100 defaults.
        for cls in ["ViewportFrame", "CanvasGroup", "VideoFrame"] {
            let r = insert(&mut doc, sg, cls);
            assert_eq!(doc.prop(r, "Size"), Some(Variant::UDim2(UDim2::new(UDim::new(0.0, 100), UDim::new(0.0, 100)))), "{cls}");
            assert_eq!(doc.prop(r, "BackgroundColor3"), Some(Variant::Color3(Color3::new(1.0, 1.0, 1.0))), "{cls}");
            assert_eq!(doc.prop(r, "BorderSizePixel"), Some(Variant::Int32(0)), "{cls}");
        }

        // TextLabel, TextButton, TextBox: 200x50, SourceSans 14, black text, class default Text.
        for (cls, expected_text) in [("TextLabel", "Label"), ("TextButton", "Button"), ("TextBox", "TextBox")] {
            let r = insert(&mut doc, sg, cls);
            assert_eq!(doc.prop(r, "Size"), Some(Variant::UDim2(UDim2::new(UDim::new(0.0, 200), UDim::new(0.0, 50)))), "{cls}");
            assert_eq!(doc.prop(r, "FontFace"), Some(Variant::Font(Font::regular("rbxasset://fonts/families/SourceSansPro.json"))), "{cls}");
            assert_eq!(doc.prop(r, "TextSize"), Some(Variant::Float32(14.0)), "{cls}");
            assert_eq!(doc.prop(r, "TextColor3"), Some(Variant::Color3(Color3::new(0.0, 0.0, 0.0))), "{cls}");
            assert_eq!(doc.prop(r, "BackgroundColor3"), Some(Variant::Color3(Color3::new(1.0, 1.0, 1.0))), "{cls}");
            assert_eq!(doc.prop(r, "BorderSizePixel"), Some(Variant::Int32(0)), "{cls}");
            assert_eq!(doc.prop(r, "Text"), Some(Variant::String(expected_text.into())), "{cls}");
        }

        // ImageLabel, ImageButton: 100x100 + GuiImagePlaceholder.png.
        for cls in ["ImageLabel", "ImageButton"] {
            let r = insert(&mut doc, sg, cls);
            assert_eq!(doc.prop(r, "Size"), Some(Variant::UDim2(UDim2::new(UDim::new(0.0, 100), UDim::new(0.0, 100)))), "{cls}");
            assert_eq!(
                doc.prop(r, "Image"),
                Some(Variant::ContentId(ContentId::from("rbxasset://textures/ui/GuiImagePlaceholder.png"))),
                "{cls}"
            );
        }

        // ScrollingFrame: 100x100 + Active true + black ScrollBarImageColor3.
        let sf = insert(&mut doc, sg, "ScrollingFrame");
        assert_eq!(doc.prop(sf, "Size"), Some(Variant::UDim2(UDim2::new(UDim::new(0.0, 100), UDim::new(0.0, 100)))));
        assert_eq!(doc.prop(sf, "Active"), Some(Variant::Bool(true)));
        assert_eq!(doc.prop(sf, "ScrollBarImageColor3"), Some(Variant::Color3(Color3::new(0.0, 0.0, 0.0))));

        // BillboardGui & SurfaceGui.
        let ws = doc.workspace().expect("Workspace");
        let bb = insert(&mut doc, ws, "BillboardGui");
        assert_eq!(doc.prop(bb, "Size"), Some(Variant::UDim2(UDim2::new(UDim::new(0.0, 200), UDim::new(0.0, 50)))));
        assert_eq!(doc.prop(bb, "LightInfluence"), Some(Variant::Float32(1.0)));
        assert_eq!(doc.prop(bb, "ClipsDescendants"), Some(Variant::Bool(true)));
        assert_eq!(doc.prop(bb, "Active"), Some(Variant::Bool(true)));
        assert_eq!(doc.prop(bb, "ZIndexBehavior"), Some(Variant::Enum(Enum::from_u32(1))));

        let surf = insert(&mut doc, ws, "SurfaceGui");
        assert_eq!(doc.prop(surf, "LightInfluence"), Some(Variant::Float32(1.0)));
        assert_eq!(doc.prop(surf, "ClipsDescendants"), Some(Variant::Bool(true)));
        assert_eq!(doc.prop(surf, "SizingMode"), Some(Variant::Enum(Enum::from_u32(1))));
        assert_eq!(doc.prop(surf, "MaxDistance"), Some(Variant::Float32(1000.0)));
        assert_eq!(doc.prop(surf, "ZIndexBehavior"), Some(Variant::Enum(Enum::from_u32(1))));

        // UIGridStyleLayout subclasses: SortOrder = LayoutOrder (2).
        for cls in ["UIListLayout", "UIGridLayout", "UIPageLayout", "UITableLayout"] {
            let r = insert(&mut doc, frame, cls);
            assert_eq!(doc.prop(r, "SortOrder"), Some(Variant::Enum(Enum::from_u32(2))), "{cls}");
        }

        // UICorner keeps Studio's {0, 8} default.
        let corner = insert(&mut doc, frame, "UICorner");
        assert_eq!(doc.prop(corner, "CornerRadius"), Some(Variant::UDim(UDim::new(0.0, 8))));
    }

    #[test]
    fn gui_paste_target_and_show_development_gui_toggle() {
        let mut doc = Document::new_place();
        let root = doc.dom.root_ref();
        let sg_svc = doc.find_child(root, "StarterGui").expect("StarterGui");
        let sg = doc.insert(sg_svc, InstanceBuilder::new("ScreenGui").with_name("ScreenGui"));
        let frame = doc.insert(sg, InstanceBuilder::new("Frame").with_name("Frame"));

        // ShowDevelopmentGui defaults to true and toggles cleanly with undo.
        assert!(super::starter_gui_shown_in(&doc));
        super::set_starter_gui_shown_in(&mut doc, false);
        assert!(!super::starter_gui_shown_in(&doc));
        assert_eq!(doc.prop(sg_svc, "ShowDevelopmentGui"), Some(Variant::Bool(false)));
        assert!(doc.undo().is_some());
        assert!(super::starter_gui_shown_in(&doc));

        // Pasting with a top-level Service (StarterGui) selected pastes INTO StarterGui,
        // never directly under DataModel root.
        doc.selection = vec![sg_svc];
        assert_eq!(super::paste_target_for(&doc, false), sg_svc);
        assert_eq!(super::paste_target_for(&doc, true), sg_svc);

        // Pasting with a Frame selected pastes next to it (ScreenGui); Paste Into pastes into Frame.
        doc.selection = vec![frame];
        assert_eq!(super::paste_target_for(&doc, false), sg);
        assert_eq!(super::paste_target_for(&doc, true), frame);
    }
}

fn paste_target_for(doc: &Document, into: bool) -> Ref {
    let root = doc.dom.root_ref();
    match doc.selection.first() {
        Some(&r) if into => r,
        Some(&r) => match doc.dom.get_by_ref(r).map(|i| i.parent()) {
            Some(p) if p == root => r,
            Some(p) if p.is_some() => p,
            _ => doc.default_parent(),
        },
        None => doc.default_parent(),
    }
}

fn starter_gui_shown_in(doc: &Document) -> bool {
    let root = doc.dom.root_ref();
    doc.find_child(root, "StarterGui")
        .and_then(|sg| match doc.prop(sg, "ShowDevelopmentGui") {
            Some(rbx_dom_weak::types::Variant::Bool(b)) => Some(b),
            _ => None,
        })
        .unwrap_or(true)
}

fn set_starter_gui_shown_in(doc: &mut Document, shown: bool) {
    let root = doc.dom.root_ref();
    if let Some(sg) = doc.find_child(root, "StarterGui").filter(|_| starter_gui_shown_in(doc) != shown) {
        doc.set_props(
            "Toggle ShowDevelopmentGui",
            vec![(sg, rbx_dom_weak::ustr("ShowDevelopmentGui"), rbx_dom_weak::types::Variant::Bool(shown))],
            None,
        );
    }
}

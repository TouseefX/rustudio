//! Modular native Rust plugins hosted inside [`super::PluginHost`].
//!
//! Native Rust tools (such as RusTudio's Animation Editor, and future modular
//! ports of Toolbox / Terrain Editor / Asset Manager) implement [`NativePlugin`]
//! and register into the same plugin host as Luau plugins rather than being
//! hardcoded fields and `Tab::*` variants on `Editor`.
//!
//! Every native plugin can be registered, loaded, toggled, unloaded, or swapped
//! for a Luau / Roblox built-in provider cleanly at runtime with zero leftover
//! state and zero Luau VM overhead while closed.

use std::any::Any;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rbx_dom_weak::types::Ref;
use rustudio_core::doc::Document;
use rustudio_core::rustudio_settings::{BuiltinTool, BuiltinToolChoices};
use rustudio_render::camera::Camera;
use rustudio_render::render::LineVertex;
use rustudio_render::scene::Scene;
use serde::{Deserialize, Serialize};

use crate::app::Level;

use super::{PluginId, PluginInfo, PluginSource};

/// Dockable panel descriptor for a [`NativePlugin`].
#[derive(Clone, Copy, Debug)]
pub struct NativeWidgetSpec {
    /// Widget id within the plugin (forms key `plugin:native_<plugin_id>_<id>`).
    pub id: &'static str,
    /// Tab / window title shown in the dock.
    pub title: &'static str,
    /// Optional key into `crate::panels::PLACEMENTS` (e.g. `"clip_editor"`).
    /// When set, the widget uses that Studio dock placement row; otherwise it
    /// falls back to `dock_state`.
    pub placement_key: Option<&'static str>,
    /// Initial dock state when `placement_key` is absent (`"Left"`, `"Right"`,
    /// `"Top"`, `"Bottom"`, `"Float"`).
    pub dock_state: &'static str,
    /// Default floating window size `[w, h]` when `dock_state == "Float"`.
    pub float_size: [f32; 2],
}

/// Static manifest describing one modular native Rust plugin.
#[derive(Clone, Copy, Debug)]
pub struct NativePluginMeta {
    /// Stable unique id (e.g. `"rustudio_AnimationEditor"`).
    pub id: &'static str,
    /// Human-readable name (e.g. `"Animation Editor"`).
    pub display_name: &'static str,
    /// Short description shown in Manage Plugins / suite UI.
    pub description: &'static str,
    /// If this plugin provides one of the duplicated Studio tools, which one.
    /// Used to coordinate cleanly with `BuiltinToolChoices` (loading when set
    /// to RusTudio's implementation, unloading when switched to Roblox's).
    pub builtin_tool: Option<BuiltinTool>,
    /// Logical shell capability / ribbon action id this plugin provides
    /// (e.g. `"clip_editor"`).
    pub capability: Option<&'static str>,
    /// Optional dockable widget managed via `Tab::Plugin(slot)`.
    pub widget: Option<NativeWidgetSpec>,
    /// True if this plugin renders a full-width bottom strip directly above
    /// Studio's status bar (e.g. Command Bar).
    pub has_bottom_bar: bool,
}

/// Side-effect requested by a [`NativePlugin`] during a tick, UI draw, or
/// viewport interaction; applied by the editor host immediately afterwards.
#[derive(Debug)]
pub enum NativePluginRequest {
    Log(Level, String),
    Select(Vec<Ref>),
    OpenFileDialog {
        tag: String,
        title: String,
        default_name: Option<String>,
        filters: Vec<(String, Vec<String>)>,
        save: bool,
    },
    SetWidgetOpen(bool),
    /// Executes Luau source at `security::COMMAND` identity (Edit VM when not
    /// playing, or the currently active Server/Client Play VM during Play).
    RunLuau { code: String },
}

/// Host context passed to [`NativePlugin`] lifecycle, tick, dialog, and widget
/// rendering callbacks.
pub struct NativeHostCtx<'a> {
    pub doc: &'a mut Document,
    pub cam: &'a mut Camera,
    pub is_playing: bool,
    /// Active Play side label (`Some("Server")` / `Some("Client")`), or `None` in Edit mode.
    pub play_side: Option<&'static str>,
    /// True for plain interactive runs (controls whether per-plugin user prefs persist).
    pub interactive: bool,
    pub requests: &'a mut Vec<NativePluginRequest>,
}

/// Host context passed to [`NativePlugin::render_viewport`].
pub struct NativeViewportCtx<'a> {
    pub ctx: &'a egui::Context,
    pub painter: &'a egui::Painter,
    pub rect: egui::Rect,
    pub resp: &'a egui::Response,
    pub cam: &'a Camera,
    pub scene: &'a Scene,
    pub doc: &'a mut Document,
    pub is_playing: bool,
    pub snap_move: Option<f32>,
    pub snap_rot: Option<f32>,
    pub requests: &'a mut Vec<NativePluginRequest>,
}

/// A modular Rust plugin hosted inside [`super::PluginHost`].
pub trait NativePlugin: Any {
    fn meta(&self) -> NativePluginMeta;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Called once right after the plugin instance is created and added to the host.
    fn on_load(&mut self) {}
    /// Called right before the plugin is unloaded/removed from the host.
    fn on_unload(&mut self, _ctx: &mut NativeHostCtx<'_>) {}
    /// Called when the editor opens a different place / document.
    fn on_place_changed(&mut self, _ctx: &mut NativeHostCtx<'_>) {}

    /// Called when the plugin's dock widget transitions from closed to open.
    fn on_widget_open(&mut self, _ctx: &mut NativeHostCtx<'_>) {}
    /// Called when the plugin's dock widget transitions from open to closed.
    fn on_widget_close(&mut self, _ctx: &mut NativeHostCtx<'_>) {}

    /// Called once per frame while the plugin is loaded.
    fn tick(&mut self, _ctx: &mut NativeHostCtx<'_>, _widget_open: bool) {}

    /// Draws the plugin's dockable panel inside `Tab::Plugin(slot)`.
    fn render_widget(&mut self, _ui: &mut egui::Ui, _ctx: &mut NativeHostCtx<'_>) {}

    /// Draws a full-width bottom strip directly above Studio's status bar when
    /// `meta().has_bottom_bar` is true (e.g. Command Bar).
    fn render_bottom_bar(&mut self, _ui: &mut egui::Ui, _ctx: &mut NativeHostCtx<'_>) {}

    /// Whether this plugin's capability toggle button should appear active/checked.
    /// Defaults to its dock widget open state (`widget_open`).
    fn is_capability_active(&self, widget_open: bool) -> bool {
        widget_open
    }

    /// Custom handler invoked when this plugin's shell capability is triggered
    /// and it does not use a standard dock `widget` (e.g. bottom-bar plugins).
    fn on_capability_trigger(&mut self, _ctx: &mut NativeHostCtx<'_>) {}

    /// True while this plugin owns 3D viewport picking/draggers (suppressing
    /// Studio's default Select/Move/Scale/Rotate handles & hover outlines).
    fn owns_viewport(&self, _widget_open: bool, _is_playing: bool) -> bool {
        false
    }

    /// True while Studio's default whole-model selection outline should be
    /// hidden because this plugin draws its own per-part/joint selection.
    fn suppress_selection_outline(&self, _widget_open: bool, _is_playing: bool) -> bool {
        false
    }

    /// Appends 3D world-space debug/gizmo lines before the 3D scene draws.
    fn append_viewport_lines(
        &self,
        _doc: &Document,
        _widget_open: bool,
        _is_playing: bool,
        _lines: &mut Vec<LineVertex>,
    ) {
    }

    /// Draws 2D/3D viewport overlays and handles pointer input. Returns `true`
    /// if pointer input was consumed.
    fn render_viewport(&mut self, _ctx: &mut NativeViewportCtx<'_>, _widget_open: bool) -> bool {
        false
    }

    /// Receives a path chosen in a file dialog requested via
    /// [`NativePluginRequest::OpenFileDialog`].
    fn on_file_dialog(&mut self, _tag: &str, _path: PathBuf, _ctx: &mut NativeHostCtx<'_>) {}
}

/// Factory registration for one modular native plugin in the suite.
pub struct NativePluginEntry {
    pub meta: NativePluginMeta,
    pub factory: Box<dyn Fn() -> Box<dyn NativePlugin>>,
}

/// Live instance of a loaded native plugin inside [`NativePluginHost`].
pub struct HostedNativePlugin {
    pub id: PluginId,
    pub meta: NativePluginMeta,
    pub plugin: Box<dyn NativePlugin>,
    /// Live dock visibility as reported after the dock pass.
    pub widget_open: bool,
    /// Desired dock visibility (`true` opens the panel, `false` closes it).
    pub want_open: bool,
    /// Previous `widget_open` seen by [`NativePluginHost::sync_open_transitions`].
    pub(crate) last_notified_open: bool,
}

/// Snapshot of a loaded native plugin's dock widget, used by `widgets_ui.rs`
/// to register and synchronize `Tab::Plugin(slot)` panels alongside Luau widgets.
#[derive(Clone, Debug)]
pub struct NativeWidgetView {
    pub plugin_id: PluginId,
    pub native_id: &'static str,
    pub spec: NativeWidgetSpec,
    pub want_open: bool,
}

/// Persisted Built-in Suite enable/disable state (`plugin_suite.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SuitePrefs {
    /// Plugin ids (native or bundled) explicitly turned off by the user.
    #[serde(default)]
    pub disabled: HashSet<String>,
}

impl SuitePrefs {
    pub fn default_path() -> PathBuf {
        rustudio_core::paths::config()
            .join("rustudio")
            .join("plugin_suite.json")
    }

    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save_to(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

/// Registry and lifecycle host for modular Rust plugins inside `PluginHost`.
#[derive(Default)]
pub struct NativePluginHost {
    factories: Vec<NativePluginEntry>,
    loaded: Vec<HostedNativePlugin>,
    /// Persisted disabled built-in suite plugin ids (native + bundled Luau).
    prefs: SuitePrefs,
    /// When `Some`, changes to `prefs` are persisted to this path.
    prefs_path: Option<PathBuf>,
    rev: u64,
}

impl NativePluginHost {
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// Enables persistence of built-in suite toggles at `path` and loads any
    /// previously saved state.
    pub fn attach_prefs_file(&mut self, path: PathBuf) {
        self.prefs = SuitePrefs::load_from(&path);
        self.prefs_path = Some(path);
        self.rev = self.rev.wrapping_add(1);
    }

    /// True if the user explicitly disabled suite plugin `plugin_id`.
    pub fn is_suite_disabled(&self, plugin_id: &str) -> bool {
        self.prefs.disabled.contains(plugin_id)
    }

    /// Marks suite plugin `plugin_id` enabled (`true`) or disabled (`false`)
    /// and saves `plugin_suite.json` if attached.
    pub fn set_suite_enabled_pref(&mut self, plugin_id: &str, enabled: bool) {
        let changed = if enabled {
            self.prefs.disabled.remove(plugin_id)
        } else {
            self.prefs.disabled.insert(plugin_id.to_string())
        };
        if changed {
            if let Some(path) = &self.prefs_path {
                self.prefs.save_to(path);
            }
            self.rev = self.rev.wrapping_add(1);
        }
    }

    /// Registers a modular native plugin factory in the suite (replaces any
    /// existing factory with the same `meta.id`).
    pub fn register_factory(&mut self, entry: NativePluginEntry) {
        if let Some(slot) = self.factories.iter_mut().find(|e| e.meta.id == entry.meta.id) {
            *slot = entry;
        } else {
            self.factories.push(entry);
        }
        self.rev = self.rev.wrapping_add(1);
    }

    /// Returns metadata for all registered native plugin factories in the suite.
    pub fn registered(&self) -> impl Iterator<Item = NativePluginMeta> + '_ {
        self.factories.iter().map(|e| e.meta)
    }

    /// Returns whether a factory with `native_id` is registered.
    pub fn has_factory(&self, native_id: &str) -> bool {
        self.factories.iter().any(|e| e.meta.id == native_id)
    }

    /// Unregisters a native plugin factory from the suite (unloading its live
    /// instance first if running).
    pub fn unregister_factory(&mut self, native_id: &str, ctx: Option<&mut NativeHostCtx<'_>>) -> bool {
        self.unload(native_id, ctx);
        let before = self.factories.len();
        self.factories.retain(|e| e.meta.id != native_id);
        let removed = self.factories.len() != before;
        if removed {
            self.rev = self.rev.wrapping_add(1);
        }
        removed
    }

    /// Instantiates and loads a registered native plugin by `native_id`.
    /// No-op returning its existing `PluginId` if already loaded.
    pub fn load(&mut self, native_id: &str, alloc_id: impl FnOnce() -> PluginId) -> Result<PluginId, String> {
        if let Some(existing) = self.loaded.iter().find(|p| p.meta.id == native_id) {
            return Ok(existing.id);
        }
        let entry = self
            .factories
            .iter()
            .find(|e| e.meta.id == native_id)
            .ok_or_else(|| format!("Native plugin '{native_id}' is not registered"))?;
        let mut plugin = (entry.factory)();
        let meta = plugin.meta();
        plugin.on_load();
        let id = alloc_id();
        self.loaded.push(HostedNativePlugin {
            id,
            meta,
            plugin,
            widget_open: false,
            want_open: false,
            last_notified_open: false,
        });
        self.rev = self.rev.wrapping_add(1);
        Ok(id)
    }

    /// Unloads a running native plugin by `native_id`, calling `on_unload`
    /// when a host context is provided.
    pub fn unload(&mut self, native_id: &str, ctx: Option<&mut NativeHostCtx<'_>>) -> bool {
        let Some(pos) = self.loaded.iter().position(|p| p.meta.id == native_id) else {
            return false;
        };
        let mut hosted = self.loaded.remove(pos);
        if let Some(ctx) = ctx {
            if hosted.last_notified_open {
                hosted.plugin.on_widget_close(ctx);
                hosted.last_notified_open = false;
            }
            hosted.plugin.on_unload(ctx);
        }
        self.rev = self.rev.wrapping_add(1);
        true
    }

    /// Unloads a running native plugin by its host [`PluginId`].
    pub fn unload_by_id(&mut self, id: PluginId, ctx: Option<&mut NativeHostCtx<'_>>) -> bool {
        let Some(native_id) = self.loaded.iter().find(|p| p.id == id).map(|p| p.meta.id) else {
            return false;
        };
        self.unload(native_id, ctx)
    }

    /// Loads or unloads built-in native plugins to match `choices` and the
    /// user's Built-in Suite enable/disable preferences.
    pub fn sync_builtin_choices(
        &mut self,
        choices: BuiltinToolChoices,
        mut alloc_id: impl FnMut() -> PluginId,
        mut ctx: Option<&mut NativeHostCtx<'_>>,
    ) {
        let targets: Vec<(&'static str, Option<BuiltinTool>)> =
            self.factories.iter().map(|e| (e.meta.id, e.meta.builtin_tool)).collect();
        for (native_id, tool) in targets {
            let allowed_by_choices = match tool {
                Some(t) => choices.use_native(t),
                None => true,
            };
            let want = allowed_by_choices && !self.prefs.disabled.contains(native_id);
            if want {
                let _ = self.load(native_id, &mut alloc_id);
            } else {
                self.unload(native_id, ctx.as_deref_mut());
            }
        }
    }

    pub fn is_loaded(&self, native_id: &str) -> bool {
        self.loaded.iter().any(|p| p.meta.id == native_id)
    }

    pub fn is_empty(&self) -> bool {
        self.loaded.is_empty()
    }

    pub fn loaded(&self) -> &[HostedNativePlugin] {
        &self.loaded
    }

    pub fn loaded_mut(&mut self) -> &mut [HostedNativePlugin] {
        &mut self.loaded
    }

    pub fn loaded_info(&self) -> Vec<PluginInfo> {
        self.loaded
            .iter()
            .map(|p| PluginInfo {
                id: p.id,
                name: p.meta.id.to_string(),
                source: PluginSource::Native(p.meta.id.to_string()),
                running: true,
                errors: Vec::new(),
            })
            .collect()
    }

    /// Returns all dockable widget views for currently loaded native plugins.
    pub fn widget_views(&self) -> Vec<NativeWidgetView> {
        self.loaded
            .iter()
            .filter_map(|p| {
                let spec = p.meta.widget?;
                Some(NativeWidgetView {
                    plugin_id: p.id,
                    native_id: p.meta.id,
                    spec,
                    want_open: p.want_open,
                })
            })
            .collect()
    }

    pub fn find(&self, native_id: &str) -> Option<&HostedNativePlugin> {
        self.loaded.iter().find(|p| p.meta.id == native_id)
    }

    pub fn find_mut(&mut self, native_id: &str) -> Option<&mut HostedNativePlugin> {
        self.loaded.iter_mut().find(|p| p.meta.id == native_id)
    }

    pub fn find_by_capability(&self, cap: &str) -> Option<&HostedNativePlugin> {
        self.loaded.iter().find(|p| p.meta.capability == Some(cap))
    }

    pub fn find_by_capability_mut(&mut self, cap: &str) -> Option<&mut HostedNativePlugin> {
        self.loaded.iter_mut().find(|p| p.meta.capability == Some(cap))
    }

    pub fn get<T: NativePlugin>(&self, native_id: &str) -> Option<&T> {
        self.find(native_id)?.plugin.as_any().downcast_ref::<T>()
    }

    pub fn get_mut<T: NativePlugin>(&mut self, native_id: &str) -> Option<&mut T> {
        self.find_mut(native_id)?.plugin.as_any_mut().downcast_mut::<T>()
    }

    /// Toggles `want_open` for a loaded native plugin's dock widget. Returns
    /// the new `want_open` value, or `None` if the plugin isn't loaded or has
    /// no widget.
    pub fn toggle_widget(&mut self, native_id: &str) -> Option<bool> {
        let p = self.find_mut(native_id)?;
        p.meta.widget?;
        p.want_open = !p.want_open;
        let next = p.want_open;
        self.rev = self.rev.wrapping_add(1);
        Some(next)
    }

    /// Explicitly sets `want_open` for a loaded native plugin's dock widget.
    pub fn set_widget_open(&mut self, native_id: &str, open: bool) -> bool {
        let Some(p) = self.find_mut(native_id) else {
            return false;
        };
        if p.meta.widget.is_none() {
            return false;
        }
        let changed = p.want_open != open;
        p.want_open = open;
        if changed {
            self.rev = self.rev.wrapping_add(1);
        }
        true
    }

    /// Records the live dock visibility of a native plugin widget (called from
    /// `plugin_ui_note_open`).
    pub fn note_widget_open(&mut self, native_id: &str, open: bool) {
        if let Some(p) = self.find_mut(native_id) {
            p.widget_open = open;
        }
    }

    /// Called when the user clicks the `×` close button on a native plugin tab.
    pub fn request_widget_close(&mut self, native_id: &str) {
        let mut changed = false;
        if let Some(p) = self.find_mut(native_id) {
            if p.want_open {
                p.want_open = false;
                changed = true;
            }
        }
        if changed {
            self.rev = self.rev.wrapping_add(1);
        }
    }

    /// Fires `on_widget_open` / `on_widget_close` transitions whenever a native
    /// plugin's live dock visibility changes.
    pub fn sync_open_transitions(&mut self, ctx: &mut NativeHostCtx<'_>) {
        for hosted in &mut self.loaded {
            if hosted.widget_open != hosted.last_notified_open {
                hosted.last_notified_open = hosted.widget_open;
                if hosted.widget_open {
                    hosted.plugin.on_widget_open(ctx);
                } else {
                    hosted.plugin.on_widget_close(ctx);
                }
            }
        }
    }

    pub fn owns_viewport(&self, is_playing: bool) -> bool {
        self.loaded
            .iter()
            .any(|p| p.plugin.owns_viewport(p.widget_open, is_playing))
    }

    pub fn suppress_selection_outline(&self, is_playing: bool) -> bool {
        self.loaded
            .iter()
            .any(|p| p.plugin.suppress_selection_outline(p.widget_open, is_playing))
    }

    pub fn append_viewport_lines(&self, doc: &Document, is_playing: bool, lines: &mut Vec<LineVertex>) {
        for p in &self.loaded {
            p.plugin.append_viewport_lines(doc, p.widget_open, is_playing, lines);
        }
    }
}

/// Registers RusTudio's default suite of modular native Rust plugins into `host`.
pub fn register_default_suite(host: &mut NativePluginHost) {
    host.register_factory(crate::anim_editor::host::plugin_entry());
    host.register_factory(crate::command_bar::plugin_entry());
    super::builtin_suite::register(host);
}

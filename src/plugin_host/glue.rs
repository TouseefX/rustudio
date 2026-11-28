//! The editor's side of the plugin host: the per-frame tick, Output, the
//! requests plugins make of the editor, modular native Rust plugins, and the
//! Play session's plugin copies.

use std::path::PathBuf;

use crate::app::{Editor, Level};
use crate::file_dialog::{Kind, Purpose};
use crate::plugin_host::native::{NativeHostCtx, NativePluginRequest, NativeViewportCtx};
use rustudio_render::render::LineVertex;

impl Editor {
    /// Registers RusTudio's default suite of modular native Rust plugins into
    /// `self.plugins` and synchronizes which built-in native plugins are loaded
    /// against `self.builtin_tools` (loading persisted Built-in Suite prefs in
    /// interactive runs).
    pub fn init_native_plugin_suite(&mut self) {
        if self.harness.plain() && std::env::var_os("RUSTUDIO_DETERMINISTIC").is_none() {
            self.plugins
                .native_mut()
                .attach_prefs_file(super::native::SuitePrefs::default_path());
        }
        super::native::register_default_suite(self.plugins.native_mut());
        self.sync_native_builtins();
    }

    /// Snapshot of all Built-in Suite plugins (bundled Luau/Vide + modular Rust)
    /// for Manage Plugins.
    pub fn suite_items(&self) -> Vec<crate::plugin_store::ui::SuiteItem> {
        use crate::plugin_store::ui::{SuiteItem, SuiteItemKind};
        let mut out = Vec::new();
        for (id, name, path, enabled, errors) in self.plugins.bundled_suite_entries() {
            let pretty = pretty_bundled_name(&name);
            let desc = bundled_suite_description(&id, &path);
            out.push(SuiteItem {
                id,
                name: pretty,
                description: desc,
                kind: SuiteItemKind::BundledLuau,
                enabled,
                overridden_by_roblox: false,
                path: Some(path),
                errors,
            });
        }
        for meta in self.plugins.native().registered() {
            let overridden_by_roblox = meta
                .builtin_tool
                .is_some_and(|t| self.builtin_tools.use_roblox(t));
            let enabled = self.plugins.native().is_loaded(meta.id);
            out.push(SuiteItem {
                id: meta.id.to_string(),
                name: meta.display_name.to_string(),
                description: meta.description.to_string(),
                kind: SuiteItemKind::NativeRust,
                enabled,
                overridden_by_roblox,
                path: None,
                errors: Vec::new(),
            });
        }
        out
    }

    /// Applies user toggle / reload actions from Manage Plugins → Built-in Suite.
    pub fn apply_suite_actions(&mut self, actions: Vec<crate::plugin_store::ui::SuiteAction>) {
        use crate::plugin_store::ui::SuiteAction;
        for act in actions {
            match act {
                SuiteAction::ToggleNative(id, on) => {
                    self.plugins.native_mut().set_suite_enabled_pref(&id, on);
                    self.sync_native_builtins();
                }
                SuiteAction::ToggleBundled(path, on) => {
                    self.plugins.set_bundled_enabled(&path, on);
                }
                SuiteAction::ReloadBundled(path) => {
                    self.plugins.reload_bundled(&path);
                }
                SuiteAction::OpenSuiteFolder(dir) => {
                    let _ = std::fs::create_dir_all(&dir);
                    #[cfg(target_os = "macos")]
                    let _ = std::process::Command::new("open").arg(&dir).spawn();
                    #[cfg(target_os = "windows")]
                    let _ = std::process::Command::new("explorer").arg(&dir).spawn();
                    #[cfg(all(unix, not(target_os = "macos")))]
                    let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
                }
            }
        }
    }

    fn native_play_side_label(&self) -> Option<&'static str> {
        self.game.play.as_ref().map(|p| match p.showing {
            rustudio_engine::runtime::Side::Server => "Server",
            rustudio_engine::runtime::Side::Client => "Client",
        })
    }

    /// True when modular native Rust plugin `native_id` is currently loaded.
    pub fn native_tool_loaded(&self, native_id: &str) -> bool {
        self.plugins.native().is_loaded(native_id)
    }

    /// Loads/unloads modular native Rust built-in plugins to match
    /// `self.builtin_tools` and the user's Built-in Suite toggles (calling
    /// `on_unload` and immediately deactivating any unloaded tool's UI/state).
    pub fn sync_native_builtins(&mut self) {
        use super::builtin_suite;
        use crate::app::{Tab, Tool};
        let draggers_was_loaded = self.native_tool_loaded(builtin_suite::DRAGGERS_ID);
        let mut reqs = Vec::new();
        let is_playing = self.game.is_playing();
        let play_side = self.native_play_side_label();
        let interactive = self.harness.plain();
        {
            let mut ctx = NativeHostCtx {
                doc: &mut self.game.doc,
                cam: &mut self.game.cam,
                is_playing,
                play_side,
                interactive,
                requests: &mut reqs,
            };
            self.plugins.sync_native_builtins(self.builtin_tools, Some(&mut ctx));
        }
        self.apply_native_plugin_requests(None, reqs);
        if !self.native_tool_loaded(builtin_suite::TOOLBOX_ID) {
            self.ui.panel_requests.push((Tab::Toolbox, Some(false)));
        }
        if !self.native_tool_loaded(builtin_suite::TERRAIN_EDITOR_ID) {
            self.terrain_ed.tool = None;
            self.ui.panel_requests.push((Tab::TerrainEditor, Some(false)));
        }
        if !self.native_tool_loaded(builtin_suite::ASSET_MANAGER_ID) {
            self.ui.panel_requests.push((Tab::AssetManager, Some(false)));
        }
        if !self.native_tool_loaded(builtin_suite::RIG_BUILDER_ID) {
            self.chars.open = false;
        }
        if !self.native_tool_loaded(builtin_suite::DRAGGERS_ID) {
            if matches!(self.tool, Tool::Select | Tool::Move | Tool::Scale | Tool::Rotate) {
                self.tool = Tool::None;
            }
        } else if !draggers_was_loaded && self.tool == Tool::None && self.plugins.active_plugin().is_none() {
            self.tool = Tool::Select;
        }
        self.sync_native_plugin_widgets();
    }

    /// Applies side-effect requests emitted by a modular native plugin.
    fn apply_native_plugin_requests(&mut self, plugin_id: Option<&str>, reqs: Vec<NativePluginRequest>) {
        for req in reqs {
            match req {
                NativePluginRequest::Log(level, msg) => self.push_log(level, msg),
                NativePluginRequest::Select(refs) => self.select(refs),
                NativePluginRequest::OpenFileDialog {
                    tag,
                    title,
                    default_name,
                    filters,
                    save,
                } => {
                    let Some(pid) = plugin_id else { continue };
                    // (crate::file_dialog::Request: the desktop's rfd dialog,
                    // Android's own picker — src/android/storage.rs)
                    let mut d = crate::file_dialog::Request::new().set_title(&title);
                    if let Some(name) = default_name {
                        d = d.set_file_name(&name);
                    }
                    for (label, exts) in &filters {
                        let refs: Vec<&str> = exts.iter().map(String::as_str).collect();
                        d = d.add_filter(label, &refs);
                    }
                    let kind = if save { Kind::SaveFile } else { Kind::PickFile };
                    self.dialogs.start(
                        Purpose::NativePlugin {
                            plugin_id: pid.to_string(),
                            tag,
                        },
                        d,
                        kind,
                    );
                }
                NativePluginRequest::SetWidgetOpen(open) => {
                    if let Some(pid) = plugin_id {
                        self.plugins.native_mut().set_widget_open(pid, open);
                    }
                }
                NativePluginRequest::RunLuau { code } => {
                    self.run_command_bar_luau(&code);
                }
            }
        }
    }

    /// Executes a Command Bar Luau chunk at `security::COMMAND` identity in the
    /// active DataModel (Edit VM when editing, or the active Server/Client side
    /// during Play), echoing the command and any output/errors into Output.
    pub fn run_command_bar_luau(&mut self, code: &str) {
        let trimmed = code.trim();
        if trimmed.is_empty() {
            return;
        }
        let preview = trimmed
            .lines()
            .next()
            .unwrap_or(trimmed);
        let suffix = if trimmed.lines().count() > 1 { " …" } else { "" };
        self.push_log(Level::System, format!("> {preview}{suffix}"));

        self.command_eval_seq = self.command_eval_seq.wrapping_add(1);
        let id = (1u64 << 62) | self.command_eval_seq;

        if let Some(side) = self.game.play.as_ref().map(|p| p.showing) {
            match self.game.mcp_eval_start(side, code, id) {
                Ok(()) => {
                    self.command_evals.push((Some(side), id));
                }
                Err(e) => self.push_log(Level::Error, format!("Command Bar: {e}")),
            }
            return;
        }

        if self.place_loading() {
            self.push_log(Level::Warn, "Command Bar: place is still loading");
            return;
        }

        let identity = rustudio_engine::runtime::security::COMMAND;
        self.edit_audio_tick();
        let ea_len = self.game.edit_audio.lengths.clone();
        let run_res = self.plugins.with_edit_vm(&mut self.game.doc, |rt| {
            if let Ok(mut w) = rustudio_engine::runtime::world(&rt.lua) {
                for (&r, &len) in &ea_len {
                    w.input.sound_len.insert(r, len);
                }
            }
            rustudio_player::luau_eval::start(&rt.lua, code, id, identity)?;
            rt.dispatch();
            Ok::<_, String>(rustudio_player::luau_eval::poll(&rt.lua, id))
        });
        self.edit_audio_tick();
        self.plugin_output();

        match run_res {
            Ok(Ok(Some(outcome))) => {
                if outcome.get("ok").and_then(|v| v.as_bool()) == Some(false) {
                    if let Some(err) = outcome.get("error").and_then(|v| v.as_str()) {
                        self.push_log(Level::Error, err.to_string());
                    }
                }
                self.game.doc.commit_pending_script_changes("Command Bar");
            }
            Ok(Ok(None)) => {
                // Command yielded (e.g. task.wait); poll it on subsequent frames.
                self.command_evals.push((None, id));
            }
            Ok(Err(e)) | Err(e) => {
                self.push_log(Level::Error, e);
            }
        }
    }

    /// Polls any in-flight yielding Command Bar evaluations so their errors
    /// surface in Output and Edit-mode changes commit cleanly to undo history.
    fn poll_command_bar_evals(&mut self) {
        if self.command_evals.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.command_evals);
        for (side, id) in pending {
            let outcome = match side {
                None => {
                    if self.game.is_playing() {
                        Some(serde_json::json!({ "ok": true }))
                    } else {
                        self.plugins
                            .with_edit_vm(&mut self.game.doc, |rt| rustudio_player::luau_eval::poll(&rt.lua, id))
                            .unwrap_or_else(|e| Some(serde_json::json!({ "ok": false, "error": e })))
                    }
                }
                Some(s) => self.game.mcp_eval_poll(s, id),
            };
            match outcome {
                Some(val) => {
                    if val.get("ok").and_then(|v| v.as_bool()) == Some(false) {
                        if let Some(err) = val.get("error").and_then(|v| v.as_str()) {
                            self.push_log(Level::Error, err.to_string());
                        }
                    }
                    if side.is_none() && !self.game.is_playing() {
                        self.game.doc.commit_pending_script_changes("Command Bar");
                    }
                }
                None => self.command_evals.push((side, id)),
            }
        }
    }

    /// Renders full-width bottom strips owned by modular native plugins
    /// (e.g. Command Bar) directly above Studio's status bar.
    pub fn native_plugins_bottom_bars(&mut self, ui: &mut egui::Ui) {
        let mut all_reqs: Vec<(String, Vec<NativePluginRequest>)> = Vec::new();
        let is_playing = self.game.is_playing();
        let play_side = self.native_play_side_label();
        let interactive = self.harness.plain();
        for hosted in self.plugins.native_mut().loaded_mut() {
            if !hosted.meta.has_bottom_bar {
                continue;
            }
            let mut r = Vec::new();
            let mut ctx = NativeHostCtx {
                doc: &mut self.game.doc,
                cam: &mut self.game.cam,
                is_playing,
                play_side,
                interactive,
                requests: &mut r,
            };
            hosted.plugin.render_bottom_bar(ui, &mut ctx);
            if !r.is_empty() {
                all_reqs.push((hosted.meta.id.to_string(), r));
            }
        }
        for (pid, r) in all_reqs {
            self.apply_native_plugin_requests(Some(&pid), r);
        }
    }

    /// Notifies all loaded native plugins that a new place / document opened.
    pub fn native_plugins_on_place_changed(&mut self) {
        let mut reqs: Vec<(String, Vec<NativePluginRequest>)> = Vec::new();
        let is_playing = self.game.is_playing();
        let play_side = self.native_play_side_label();
        let interactive = self.harness.plain();
        for hosted in self.plugins.native_mut().loaded_mut() {
            let mut r = Vec::new();
            let mut ctx = NativeHostCtx {
                doc: &mut self.game.doc,
                cam: &mut self.game.cam,
                is_playing,
                play_side,
                interactive,
                requests: &mut r,
            };
            hosted.plugin.on_place_changed(&mut ctx);
            if !r.is_empty() {
                reqs.push((hosted.meta.id.to_string(), r));
            }
        }
        for (pid, r) in reqs {
            self.apply_native_plugin_requests(Some(&pid), r);
        }
    }

    /// Runs open/close transitions and per-frame ticks for loaded modular
    /// native Rust plugins, then syncs their `Tab::Plugin(slot)` dock state.
    pub fn native_plugins_tick(&mut self) {
        self.poll_command_bar_evals();
        let mut all_reqs: Vec<(String, Vec<NativePluginRequest>)> = Vec::new();
        let is_playing = self.game.is_playing();
        let play_side = self.native_play_side_label();
        let interactive = self.harness.plain();
        for hosted in self.plugins.native_mut().loaded_mut() {
            let mut r = Vec::new();
            let mut ctx = NativeHostCtx {
                doc: &mut self.game.doc,
                cam: &mut self.game.cam,
                is_playing,
                play_side,
                interactive,
                requests: &mut r,
            };
            if hosted.widget_open != hosted.last_notified_open {
                hosted.last_notified_open = hosted.widget_open;
                if hosted.widget_open {
                    hosted.plugin.on_widget_open(&mut ctx);
                } else {
                    hosted.plugin.on_widget_close(&mut ctx);
                }
            }
            hosted.plugin.tick(&mut ctx, hosted.widget_open);
            if !r.is_empty() {
                all_reqs.push((hosted.meta.id.to_string(), r));
            }
        }
        for (pid, r) in all_reqs {
            self.apply_native_plugin_requests(Some(&pid), r);
        }
        self.sync_native_plugin_widgets();
    }

    /// Renders one modular native plugin's dockable panel (`Tab::Plugin(slot)`).
    pub fn native_plugin_render_widget(&mut self, native_id: &str, ui: &mut egui::Ui) {
        let mut reqs = Vec::new();
        let is_playing = self.game.is_playing();
        let play_side = self.native_play_side_label();
        let interactive = self.harness.plain();
        if let Some(hosted) = self.plugins.native_mut().find_mut(native_id) {
            let mut ctx = NativeHostCtx {
                doc: &mut self.game.doc,
                cam: &mut self.game.cam,
                is_playing,
                play_side,
                interactive,
                requests: &mut reqs,
            };
            hosted.plugin.render_widget(ui, &mut ctx);
        }
        self.apply_native_plugin_requests(Some(native_id), reqs);
    }

    /// Routes a completed file dialog back to the modular native plugin that
    /// requested it.
    pub fn native_plugin_handle_file(&mut self, plugin_id: &str, tag: &str, path: PathBuf) {
        let mut reqs = Vec::new();
        let is_playing = self.game.is_playing();
        let play_side = self.native_play_side_label();
        let interactive = self.harness.plain();
        if let Some(hosted) = self.plugins.native_mut().find_mut(plugin_id) {
            let mut ctx = NativeHostCtx {
                doc: &mut self.game.doc,
                cam: &mut self.game.cam,
                is_playing,
                play_side,
                interactive,
                requests: &mut reqs,
            };
            hosted.plugin.on_file_dialog(tag, path, &mut ctx);
        }
        self.apply_native_plugin_requests(Some(plugin_id), reqs);
    }

    /// True if any loaded native plugin currently owns 3D viewport picking and
    /// draggers (suppressing Studio's default Select/Move/Scale/Rotate gizmos).
    pub fn native_plugins_own_viewport(&self) -> bool {
        self.plugins.native().owns_viewport(self.game.is_playing())
    }

    /// True if any loaded native plugin suppresses Studio's whole-model
    /// selection outline because it draws its own selection adornments.
    pub fn native_plugins_suppress_selection(&self) -> bool {
        self.plugins.native().suppress_selection_outline(self.game.is_playing())
    }

    /// Appends 3D world-space lines (e.g. Animation Editor FloorGrid) from
    /// loaded native plugins.
    pub fn native_plugins_viewport_lines(&self, lines: &mut Vec<LineVertex>) {
        self.plugins
            .native()
            .append_viewport_lines(&self.game.doc, self.game.is_playing(), lines);
    }

    /// Runs loaded native plugins' 2D/3D viewport overlays and pointer
    /// interaction. Returns `true` if any plugin consumed pointer input.
    pub fn native_plugins_viewport_ui(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: egui::Rect,
        resp: &egui::Response,
    ) -> bool {
        let is_playing = self.game.is_playing();
        let snap_move = self.game.snap_move.then_some(self.game.move_inc);
        let snap_rot = self.game.snap_rotate.then_some(self.game.rotate_inc);
        let mut consumed = false;
        let mut all_reqs: Vec<(String, Vec<NativePluginRequest>)> = Vec::new();
        for hosted in self.plugins.native_mut().loaded_mut() {
            let mut r = Vec::new();
            let mut vctx = NativeViewportCtx {
                ctx,
                painter,
                rect,
                resp,
                cam: &self.game.cam,
                scene: &self.game.scene,
                doc: &mut self.game.doc,
                is_playing,
                snap_move,
                snap_rot,
                requests: &mut r,
            };
            consumed |= hosted.plugin.render_viewport(&mut vctx, hosted.widget_open);
            if !r.is_empty() {
                all_reqs.push((hosted.meta.id.to_string(), r));
            }
        }
        for (pid, r) in all_reqs {
            self.apply_native_plugin_requests(Some(&pid), r);
        }
        consumed
    }

    /// Unified shell capability query (`(enabled, checked)`): checks bundled
    /// RusTudio Luau plugins, then Roblox built-in plugins (when configured for
    /// that tool), then modular native Rust plugins in `PluginHost`.
    pub fn capability_state(&self, cap: &str) -> (bool, Option<bool>) {
        if let Some((en, ck)) = self.bundled_capability_state(cap) {
            return (en, ck);
        }
        if let Some((stem, tb, btn)) = roblox_capability_target(cap, self.builtin_tools) {
            return self.builtin_button_state(stem, tb, btn);
        }
        if let Some(hosted) = self.plugins.native().find_by_capability(cap) {
            return (true, Some(hosted.plugin.is_capability_active(hosted.widget_open)));
        }
        (false, Some(false))
    }

    /// Unified shell capability trigger: activates/toggles whichever plugin
    /// (bundled Luau, Roblox built-in, or modular native Rust) provides `cap`.
    pub fn trigger_capability(&mut self, cap: &str) -> bool {
        if self.trigger_bundled_capability(cap) {
            return true;
        }
        if let Some((stem, tb, btn)) = roblox_capability_target(cap, self.builtin_tools) {
            return self.trigger_builtin_plugin_button(stem, tb, btn);
        }
        let target = self
            .plugins
            .native()
            .find_by_capability(cap)
            .map(|p| (p.meta.id, p.meta.widget.is_some()));
        if let Some((nid, has_widget)) = target {
            if has_widget {
                self.plugins.native_mut().toggle_widget(nid);
                self.sync_native_plugin_widgets();
            } else {
                let mut reqs = Vec::new();
                let is_playing = self.game.is_playing();
                let play_side = self.native_play_side_label();
                let interactive = self.harness.plain();
                if let Some(hosted) = self.plugins.native_mut().find_mut(nid) {
                    let mut ctx = NativeHostCtx {
                        doc: &mut self.game.doc,
                        cam: &mut self.game.cam,
                        is_playing,
                        play_side,
                        interactive,
                        requests: &mut reqs,
                    };
                    hosted.plugin.on_capability_trigger(&mut ctx);
                }
                self.apply_native_plugin_requests(Some(nid), reqs);
            }
            return true;
        }
        false
    }

    /// Once a frame, after the Play tick: the edit DataModel's plugins run
    /// (on the saved edit document while Play runs, without RenderStepped).
    pub fn plugin_tick(&mut self, dt: f64) {
        // --plugin / --plugins-dir / --no-plugins, once the place is open.
        if !self.harness.plugins_started && !self.place_loading() {
            self.harness.plugins_started = true;
            // Studio is signed in before plugins run (GetStudioUserId, the
            // installed list): a --no-ui run lets the session check answer
            // first, as Play's start does (at most 10 s).
            if self.harness.run.no_ui {
                for (level, text) in self.game.account.settle_for_play(std::time::Duration::from_secs(10), false) {
                    self.push_log(level, text);
                }
            }
            if self.harness.no_plugins {
                self.plugins.set_enabled(false);
            } else {
                self.builtin_tools = rustudio_core::rustudio_settings::BuiltinToolChoices::from_settings();
                self.sync_native_builtins();
                if self.builtin_tools.use_roblox(rustudio_core::rustudio_settings::BuiltinTool::Draggers)
                    && matches!(
                        self.tool,
                        crate::app::Tool::Select | crate::app::Tool::Move | crate::app::Tool::Scale | crate::app::Tool::Rotate
                    )
                {
                    self.tool = crate::app::Tool::None;
                }
                let _ = self.plugins.load_bundled_suite();
                if self.builtin_tools.plugins_enabled {
                    let _ = self.plugins.load_builtins_with_choices(self.builtin_tools);
                    if self.builtin_tools.use_roblox(rustudio_core::rustudio_settings::BuiltinTool::ViewSelector) {
                        self.plugins.wake_plugin_by_name("ViewSelector");
                    }
                }
                for d in self.harness.plugin_dirs.clone() {
                    let _ = self.plugins.load_dir(&d);
                }
                for (asset_id, path) in self.harness.installed_plugins.clone() {
                    let _ = self.plugins.load(super::PluginSource::Cloud { asset_id, path });
                }
                for f in self.harness.plugin_files.clone() {
                    let _ = self.plugins.load(super::PluginSource::File(f));
                }
            }
        }
        if self.plugins.is_idle() || self.place_loading() {
            self.sync_native_plugin_widgets();
            self.plugin_ui_sync();
            return;
        }
        // Plugin GUI / viewport input since the last tick (widgets_ui.rs).
        self.plugin_ui.dt = dt as f32;
        let ui_flush = self.prepare_plugin_ui_flush();
        // (Before the session check answers: the cached signed-in profile, as
        // the plugin store uses; Studio has the id before plugins run.)
        let uid = self.game.account.user_id().unwrap_or_else(|| {
            static FALLBACK_UID: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
            *FALLBACK_UID.get_or_init(|| {
                if std::env::var_os("RUSTUDIO_NO_SESSION").is_some() {
                    0
                } else {
                    crate::plugin_store::current_user_id()
                }
            })
        });
        super::settings::set_user_id(uid);
        // Plugin.GridSize / GetJoinMode read the editor's toolbar settings
        // (Play keeps them current while it runs).
        if self.game.play.is_none() {
            rustudio_engine::runtime::domains::testing::set_editor_state(rustudio_engine::runtime::domains::testing::EditorState {
                snap_move: self.game.snap_move,
                move_inc: self.game.move_inc,
                snap_rotate: self.game.snap_rotate,
                rotate_inc: self.game.rotate_inc,
                local_space: self.game.local_space,
                collisions: self.collisions,
                join_surfaces: self.join_surfaces,
            });
        }
        // GetSelectedRibbonTool reads the editor's tool; while a plugin is
        // the active tool it keeps the last ribbon tool.
        if self.plugins.active_plugin().is_none() && !matches!(self.tool, crate::app::Tool::None) {
            crate::plugin_host::api::studio::set_ribbon_tool(self.tool.label());
        }
        // Headless with no prompt answers scheduled: permission prompts are
        // refused instead of waiting forever.
        let answers = self.harness.run.no_ui
            && self
                .harness
                .run
                .input
                .actions
                .iter()
                .any(|(_, a)| matches!(a, crate::app::HarnessAction::UiAction(n) if n.starts_with("plugin-prompt:")));
        super::permissions::set_auto_deny(self.harness.run.no_ui && !answers);
        let rendering = self.game.play.is_none();
        let realtime = self.game.fixed_dt.is_none() && !rustudio_core::sim_clock::on();
        let mut cam_sent = ui_flush.cam_sent;
        let pre = |rt: &rustudio_engine::runtime::Runtime| {
            super::widgets_ui::apply_plugin_ui_flush(rt, ui_flush, &mut cam_sent);
        };
        match self.game.play_saved_doc_mut() {
            Some(doc) => {
                // (Play holds the edit document meanwhile.)
                let doc: *mut rustudio_core::doc::Document = doc;
                // SAFETY: `doc` lives in self.game.play, which plugin_tick
                // doesn't touch while the host runs.
                self.plugins.tick_with_pre(unsafe { &mut *doc }, dt, rendering, realtime, pre);
            }
            None => self.plugins.tick_with_pre(&mut self.game.doc, dt, rendering, realtime, pre),
        }
        self.finish_plugin_ui_flush(cam_sent);
        self.plugin_ui_after_tick();
        self.sync_native_plugin_widgets();
        self.plugin_output();
        self.profiler_open_requests();
        self.plugin_ui_sync();
    }

    /// Steps Edit-mode audio loading, `PlayLocalSound`, and Properties-panel
    /// preview playback, bridging incremental DOM edits and Edit VM requests.
    pub fn edit_audio_tick(&mut self) {
        if self.game.play.is_some() || self.place_loading() {
            return;
        }
        let (cmds, seeks, listener) = self.plugins.take_edit_audio_requests();
        let events = self.game.edit_audio_tick(cmds, seeks, listener);
        self.plugins.sync_edit_audio_to_vm(&self.game.edit_audio.lengths);
        self.plugins.deliver_edit_audio_events(&mut self.game.doc, &events);
    }

    /// Mirrors loaded audio lengths into the Edit VM (used before running Edit
    /// Luau so newly created VMs immediately see already-loaded sounds).
    pub fn sync_edit_audio_to_vm(&mut self) {
        if self.game.play.is_some() {
            return;
        }
        self.plugins.sync_edit_audio_to_vm(&self.game.edit_audio.lengths);
    }

    /// Live runtime Sound / AudioPlayer property value in Edit mode (Properties
    /// panel and MCP `get_properties`), without mutating or dirtying the saved
    /// document. Matches Studio 0.748: Preview playback is invisible to `Sound`
    /// properties (`Playing`, `IsPlaying`, `IsPaused`, `TimePosition`).
    pub fn live_sound_prop(&self, r: rbx_dom_weak::types::Ref, name: &str) -> Option<rbx_dom_weak::types::Variant> {
        use rbx_dom_weak::types::Variant;
        if self.game.play.is_some() {
            return None;
        }
        let ea = &self.game.edit_audio;
        match self.game.doc.class_of(r).as_deref() {
            Some("Sound") => {
                let playing = matches!(self.game.doc.prop(r, "Playing"), Some(Variant::Bool(true)));
                match name {
                    "IsLoaded" => Some(Variant::Bool(ea.is_loaded(r))),
                    "TimeLength" => Some(Variant::Float64(ea.time_length(r))),
                    "IsPlaying" => Some(Variant::Bool(playing)),
                    "IsPaused" => Some(Variant::Bool(!playing)),
                    _ => None,
                }
            }
            Some("AudioPlayer") => match name {
                "IsReady" => Some(Variant::Bool(ea.is_loaded(r))),
                "TimeLength" => Some(Variant::Float64(ea.time_length(r))),
                "IsPlaying" if ea.preview_ref() == Some(r) => Some(Variant::Bool(ea.preview_state(r).0)),
                "TimePosition" if ea.preview_ref() == Some(r) => Some(Variant::Float64(ea.preview_state(r).1)),
                _ => None,
            },
            _ => None,
        }
    }

    /// The profiler's Scripts tab asked to open a script at a line.
    fn profiler_open_requests(&mut self) {
        let Some(req) = rustudio_profiler_ui::scripts::take_open_request() else { return };
        let doc = &self.game.doc;
        let Some(script) = profiler_script_ref(doc, &req.script) else {
            self.push_log(Level::Warn, format!("Profiler: can't find {} in this place to open it", req.script));
            return;
        };
        self.open_script.push(script);
        super::script_docs::set_selection(script, (req.line.max(1) as i64, 1, req.line.max(1) as i64, 1), false);
    }

    /// Host and plugin Output lines into the Output panel (and --log).
    pub fn plugin_output(&mut self) {
        for n in super::permissions::take_notes() {
            self.push_log(Level::Warn, n);
        }
        for (level, text) in self.plugins.take_log() {
            self.push_log(level, text);
        }
        // ScriptEditorService / plugin:OpenScript: show or close tabs.
        for req in super::script_docs::take_requests() {
            match req {
                super::script_docs::EditorReq::Show(script, line) => {
                    if self.game.play.is_none() {
                        self.open_script.push(script);
                        if let Some(l) = line {
                            super::script_docs::set_selection(script, (l.max(1), 1, l.max(1), 1), false);
                        }
                    }
                }
                super::script_docs::EditorReq::Close(script) => self.close_script.push(script),
                super::script_docs::EditorReq::Select(..) => {}
            }
        }
        for req in self.plugins.take_requests() {
            match req {
                super::EditorRequest::StartPlaytest { mode, args, completion } => {
                    self.start_plugin_playtest(mode, args, completion);
                }
                super::EditorRequest::OpenScript { script, line } => {
                    super::script_docs::open(script, Some(line), false);
                }
                super::EditorRequest::OpenWikiPage(url) => {
                    self.push_log(Level::Info, format!("A plugin opened {url}"));
                    if !self.harness.run.no_ui {
                        super::prompts::open_url_later(url);
                    }
                }
                // The ribbon's tool changes (Studio's Group / Ungroup act
                // on the selection; the color / material pickers have no
                // rustudio tool and only report back).
                super::EditorRequest::SelectRibbonTool { tool } => {
                    use crate::app::Tool;
                    let native_draggers = self.native_tool_loaded(super::builtin_suite::DRAGGERS_ID);
                    match tool.as_str() {
                        "Select" | "Move" | "Scale" | "Rotate" if !native_draggers => {
                            self.tool = Tool::None;
                        }
                        "Select" => self.tool = Tool::Select,
                        "Move" => self.tool = Tool::Move,
                        "Scale" => self.tool = Tool::Scale,
                        "Rotate" => self.tool = Tool::Rotate,
                        "Transform" => self.tool = Tool::Transform,
                        "None" => self.tool = Tool::None,
                        "Group" => self.run_action("group"),
                        "Ungroup" => self.run_action("ungroup"),
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Formats a PascalCase bundled plugin stem (`"TagEditor"`) into `"Tag Editor"`.
fn pretty_bundled_name(stem: &str) -> String {
    match stem {
        "9SliceEditor" => return "9-Slice Editor".to_string(),
        "PathEditor" => return "Path2D Editor".to_string(),
        _ => {}
    }
    let mut out = String::with_capacity(stem.len() + 4);
    let mut prev_lower = false;
    for ch in stem.chars() {
        if ch == '_' || ch == '-' {
            out.push(' ');
            prev_lower = false;
            continue;
        }
        if ch.is_uppercase() && prev_lower {
            out.push(' ');
        }
        out.push(ch);
        prev_lower = ch.is_lowercase();
    }
    out
}

fn bundled_suite_description(id: &str, path: &std::path::Path) -> String {
    match id {
        "rustudio_9SliceEditor" => {
            "Visually edit SliceCenter 9-slice margins on ImageLabel and ImageButton UI (reactive Luau / Vide)".to_string()
        }
        "rustudio_PathEditor" => {
            "Visually edit Path2D Bézier control points, tangents, and presets (reactive Luau / Vide)".to_string()
        }
        "rustudio_TextureViewer" => {
            "Audit, preview, select, and bulk-replace textures across the place (reactive Luau / Vide)".to_string()
        }
        "rustudio_PlayerEmulator" => {
            "Emulate player country/region, locale, pseudolocalization, and PolicyService flags (reactive Luau / Vide)".to_string()
        }
        "rustudio_EventEmulator" => {
            "Emulate MemStorageService and VirtualInputManager / RBX events with live log (reactive Luau / Vide)".to_string()
        }
        "rustudio_ForceVisualizationControl" => {
            "Toggle physics force, torque, contact, constraint, and fluid draw overlays & scales (reactive Luau / Vide)".to_string()
        }
        "rustudio_TagEditor" => {
            "Inspect, create, and assign CollectionService tags (reactive Luau / Vide)".to_string()
        }
        "rustudio_AlignmentTool" => {
            "Align selected parts and models along X, Y, or Z axes (reactive Luau / Vide)".to_string()
        }
        "rustudio_WindControl" => {
            "Configure Workspace.GlobalWind speed, compass heading, and vector (reactive Luau / Vide)".to_string()
        }
        "rustudio_CollisionGroups" => {
            "Manage PhysicsService collision groups, matrix, and part assignments (reactive Luau / Vide)".to_string()
        }
        "rustudio_AudioDiscovery" => {
            "Inspect, search, select, and preview all Sound & AudioPlayer objects (reactive Luau / Vide)".to_string()
        }
        "rustudio_MaterialManager" => {
            "Browse built-in materials, toggle 2022 materials, and manage MaterialVariants (reactive Luau / Vide)".to_string()
        }
        "rustudio_PivotEditor" => {
            "Edit, snap, and reset PivotOffset / WorldPivot on parts and models (reactive Luau / Vide)".to_string()
        }
        "rustudio_StyleEditor" => {
            "Create and edit UI StyleSheets, StyleRules, and StyleLinks (reactive Luau / Vide)".to_string()
        }
        "rustudio_LocalizationTools" => {
            "Inspect, edit, and auto-capture LocalizationTable entries (reactive Luau / Vide)".to_string()
        }
        "rustudio_FindInPlace" => {
            "Search & replace across all Script, LocalScript, and ModuleScript objects (reactive Luau / Vide)".to_string()
        }
        "rustudio_ObjectBrowser" => {
            "Browse live Studio Enums and insertable classes (reactive Luau / Vide)".to_string()
        }
        _ => format!(
            "Bundled Luau / Vide plugin ({})",
            path.file_name().unwrap_or_default().to_string_lossy()
        ),
    }
}

/// Maps a shell capability id to the Roblox built-in plugin button when
/// `choices` routes that tool to Roblox's `.rbxm` plugin.
fn roblox_capability_target(
    cap: &str,
    choices: rustudio_core::rustudio_settings::BuiltinToolChoices,
) -> Option<(&'static str, Option<&'static str>, Option<&'static str>)> {
    use rustudio_core::rustudio_settings::BuiltinTool;
    match cap {
        "clip_editor" if choices.use_roblox(BuiltinTool::AnimationEditor) => {
            Some(("AnimationClipEditor", Some("Animations"), Some("Animation Editor")))
        }
        "tag_editor" if choices.plugins_enabled => {
            Some(("TagEditor", Some("tagEditorToolbar"), Some("TagEditor")))
        }
        "align" if choices.plugins_enabled => {
            Some(("AlignmentTool", Some("Alignment"), Some("AlignTool")))
        }
        "wind_direction" if choices.plugins_enabled => {
            Some(("WindControl", Some("windControl"), Some("WindControl")))
        }
        "audio_discovery" if choices.plugins_enabled => {
            Some(("AudioDiscovery", Some("Audio Discovery"), Some("View Sounds")))
        }
        "material_manager" if choices.plugins_enabled => {
            Some(("MaterialManager", Some("Edit"), Some("MaterialManager")))
        }
        "pivot" if choices.plugins_enabled => {
            Some(("PivotEditor", Some("Pivot"), Some("EditPivot")))
        }
        "pivot_reset" if choices.plugins_enabled => {
            Some(("PivotEditor", Some("Pivot"), Some("ClearPivot")))
        }
        "style_editor" if choices.plugins_enabled => {
            Some(("StyleEditor", Some("StyleEditor"), Some("StyleEditorToolbarButton")))
        }
        "localization_tools" if choices.plugins_enabled => {
            Some(("LocalizationTools", Some("Localization"), Some("Tools")))
        }
        _ => None,
    }
}

/// A profiled script's full name in `doc`: the path itself, else where Play
/// copied it from (PlayerScripts, PlayerGui, Backpack and character scripts
/// come from the Starter containers).
fn profiler_script_ref(doc: &rustudio_core::doc::Document, full: &str) -> Option<rbx_dom_weak::types::Ref> {
    let walk = |path: &[&str]| -> Option<rbx_dom_weak::types::Ref> {
        let mut cur = doc.dom.root_ref();
        for name in path {
            let inst = doc.dom.get_by_ref(cur)?;
            cur = inst.children().iter().copied().find(|c| doc.dom.get_by_ref(*c).is_some_and(|i| i.name == *name))?;
        }
        doc.dom.get_by_ref(cur).filter(|i| i.class.ends_with("Script")).map(|_| cur)
    };
    let parts: Vec<&str> = full.split('.').collect();
    if let Some(r) = walk(&parts) {
        return Some(r);
    }
    let rest = |n: usize| parts.get(n..).unwrap_or(&[]).to_vec();
    let mut tries: Vec<Vec<&str>> = Vec::new();
    match parts.as_slice() {
        ["Players", _, "PlayerScripts", ..] => tries.push([vec!["StarterPlayer", "StarterPlayerScripts"], rest(3)].concat()),
        ["Players", _, "PlayerGui", ..] => tries.push([vec!["StarterGui"], rest(3)].concat()),
        ["Players", _, "Backpack", ..] => tries.push([vec!["StarterPack"], rest(3)].concat()),
        ["Workspace", _, ..] => tries.push([vec!["StarterPlayer", "StarterCharacterScripts"], rest(2)].concat()),
        _ => {}
    }
    tries.iter().find_map(|p| walk(p))
}

#[cfg(test)]
mod profiler_open_tests {
    use rbx_dom_weak::InstanceBuilder;

    #[test]
    fn play_copies_map_back_to_their_starter_scripts() {
        let mut d = rustudio_core::doc::Document::new_place();
        let root = d.dom.root_ref();
        let sss = d.find_child(root, "ServerScriptService").unwrap();
        let npc = d.dom.insert(sss, InstanceBuilder::new("Script").with_name("NPC"));
        let sp = d.find_child(root, "StarterPlayer").unwrap();
        let sps = d.find_child(sp, "StarterPlayerScripts").unwrap();
        let hud = d.dom.insert(sps, InstanceBuilder::new("LocalScript").with_name("HUD"));
        let gui = d.find_child(root, "StarterGui").unwrap();
        let screen = d.dom.insert(gui, InstanceBuilder::new("ScreenGui").with_name("Main"));
        let inv = d.dom.insert(screen, InstanceBuilder::new("LocalScript").with_name("Inventory"));
        assert_eq!(super::profiler_script_ref(&d, "ServerScriptService.NPC"), Some(npc));
        assert_eq!(super::profiler_script_ref(&d, "Players.Player1.PlayerScripts.HUD"), Some(hud));
        assert_eq!(super::profiler_script_ref(&d, "Players.Player1.PlayerGui.Main.Inventory"), Some(inv));
        assert_eq!(super::profiler_script_ref(&d, "ServerScriptService.Missing"), None);
        // A folder isn't a script.
        assert_eq!(super::profiler_script_ref(&d, "StarterGui.Main"), None);
    }
}

//! Modular [`NativePlugin`] adapter for RusTudio's Animation Editor
//! (`AnimationClipEditor` 1:1 port).
//!
//! Runs inside [`crate::plugin_host::PluginHost`] using the same dock widget
//! (`Tab::Plugin(slot)`), viewport interaction, file dialog, and lifecycle
//! hooks as any other modular plugin — zero hardcoded fields or branches in
//! `Editor` or `viewport.rs`.

use std::any::Any;
use std::path::PathBuf;

use egui::Ui;
use rbx_dom_weak::types::Ref;

use rustudio_core::doc::Document;
use rustudio_core::rustudio_settings::BuiltinTool;
use rustudio_render::render::LineVertex;

use crate::anim_editor::constants::AUTOMATIC_SAVE_NAME;
use crate::anim_editor::state::{
    ActiveDialog, ImportSettingsDialogState, MigrationChoice, MigrationItem, PlayState,
    ReduceKeyframesDialogMode, ReduceKeyframesState, RigTypeKind, ValidationSeverity,
};
use crate::anim_editor::ui_dialogs::DialogOutcome;
use crate::anim_editor::ui_menus::MenuCommand;
use crate::anim_editor::{
    facs, history, ops, playback, reduction, rig, rig_viz, selection, serialization, ui_main,
    viewport_dragger, AnimationEditor,
};
use crate::app::Level;
use crate::plugin_host::native::{
    NativeHostCtx, NativePlugin, NativePluginEntry, NativePluginMeta, NativePluginRequest,
    NativeViewportCtx, NativeWidgetSpec,
};

/// Stable plugin id under [`crate::plugin_host::PluginHost`].
pub const PLUGIN_ID: &str = "rustudio_AnimationEditor";
/// Shell capability / ribbon action id owned by this plugin.
pub const CAPABILITY: &str = "clip_editor";

const TAG_IMPORT_FBX: &str = "import_fbx";
const TAG_IMPORT_RBXM: &str = "import_rbxm";
const TAG_EXPORT_RBXM: &str = "export_rbxm";

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Static manifest for RusTudio's modular Animation Editor plugin.
pub const META: NativePluginMeta = NativePluginMeta {
    id: PLUGIN_ID,
    display_name: "Animation Editor",
    description: "Create, preview, and edit keyframe and curve animations on R6/R15/custom rigs",
    builtin_tool: Some(BuiltinTool::AnimationEditor),
    capability: Some(CAPABILITY),
    widget: Some(NativeWidgetSpec {
        id: "AnimationEditor",
        title: "Animation Editor",
        placement_key: Some("clip_editor"),
        dock_state: "Bottom",
        float_size: [900.0, 320.0],
    }),
    has_bottom_bar: false,
};

/// Factory entry registering [`AnimationEditorPlugin`] in the native plugin suite.
pub fn plugin_entry() -> NativePluginEntry {
    NativePluginEntry {
        meta: META,
        factory: Box::new(|| Box::new(AnimationEditorPlugin::default())),
    }
}

/// Modular Animation Editor plugin hosted inside `PluginHost`.
#[derive(Default)]
pub struct AnimationEditorPlugin {
    pub state: AnimationEditor,
}

impl AnimationEditorPlugin {
    /// Attaches a rig `Model` to the Animation Editor: validates with all 9
    /// `RigValidation` rules, populates `RigCache`, checks legacy `AnimSaves`,
    /// and loads the latest saved animation (or prepares `StartScreen`).
    pub fn attach_rig(&mut self, doc: &mut Document, model: Ref, requests: &mut Vec<NativePluginRequest>) {
        if self.state.rig_cache.root_ref == Some(model) && self.state.active {
            return;
        }
        if self.state.rig_cache.root_ref.is_some() {
            self.release(doc);
        }

        let issues = rig::validate_rig(doc, model);
        let has_errors = issues.iter().any(|i| i.severity == ValidationSeverity::Error);
        if has_errors {
            self.state.active_dialog = ActiveDialog::RigErrors {
                title: "Rig Errors".to_string(),
                issues,
            };
            return;
        }

        let cache = rig::build_rig_cache(doc, model);
        let mut all_saves = cache.anim_saves.clone();
        all_saves.extend(cache.legacy_anim_saves.iter().cloned());
        rig::sort_anim_saves_latest(&mut all_saves);
        let latest = all_saves.into_iter().next();
        let legacy_items: Vec<MigrationItem> = cache
            .legacy_anim_saves
            .iter()
            .map(|e| MigrationItem {
                anim_ref: e.anim_ref,
                name: e.name.clone(),
                choice: MigrationChoice::Migrate,
            })
            .collect();

        self.state.rig_cache = cache;
        self.state.active = true;
        self.state.ik.enabled = false;
        self.state.ik.pinned_parts.clear();
        self.state.viewport_dragger.hovered_part = None;

        if let Some(entry) = latest {
            self.load_ref(doc, entry.anim_ref, requests);
        } else {
            self.state.animation_data = None;
            self.state.tracks.clear();
            selection::rebuild_track_list(&mut self.state);
        }

        if !legacy_items.is_empty() {
            self.state.active_dialog = ActiveDialog::Migration {
                items: legacy_items,
            };
        }
    }

    /// Detaches the Animation Editor from the active rig: autosaves
    /// `"Automatic Save"` when dirty, restores rest poses, and clears active state.
    pub fn release(&mut self, doc: &mut Document) {
        if self.state.is_dirty {
            if let Some(data) = self.state.animation_data.clone() {
                let _ = rig::save_clip_to_anim_saves(
                    doc,
                    &mut self.state.rig_cache,
                    &data,
                    AUTOMATIC_SAVE_NAME,
                );
            }
        }
        if self.state.rig_cache.root_ref.is_some() {
            rig::reset_rig_pose(doc, &self.state.rig_cache);
        }
        self.state.active = false;
        self.state.playback.set_play_state(PlayState::Pause);
        self.state.rig_cache.clear();
        self.state.animation_data = None;
        self.state.tracks.clear();
        self.state.selected_keyframes.clear();
        self.state.selected_events.clear();
        self.state.selected_tracks.clear();
        self.state.selected_track_instances.clear();
        self.state.is_dirty = false;
        self.state.drag_context = None;
        history::clear_history(&mut self.state);
    }

    /// Attaches or switches to a rig whenever the place selection changes
    /// while the panel is open. Selecting parts/joints inside the already-
    /// attached rig is ignored so editing is never interrupted.
    pub fn sync_selection(&mut self, ctx: &mut NativeHostCtx<'_>, widget_open: bool) {
        if !widget_open || ctx.is_playing {
            if !widget_open {
                self.state.last_synced_selection.clear();
            }
            return;
        }
        if self.state.last_synced_selection == ctx.doc.selection {
            return;
        }
        self.state.last_synced_selection = ctx.doc.selection.clone();

        let sel = ctx.doc.selection.clone();
        for r in sel {
            let mut candidate = None;
            let mut cur = r;
            while let Some(inst) = ctx.doc.dom.get_by_ref(cur) {
                if rig::is_valid_rig_candidate(ctx.doc, cur) {
                    candidate = Some(cur);
                }
                let parent = inst.parent();
                if parent.is_none() || cur == ctx.doc.dom.root_ref() {
                    break;
                }
                cur = parent;
            }
            if let Some(rig_model) = candidate {
                if self.state.rig_cache.root_ref != Some(rig_model) {
                    self.attach_rig(ctx.doc, rig_model, ctx.requests);
                }
                break;
            }
        }
    }

    /// Loads a `KeyframeSequence` or `CurveAnimation` instance by `Ref`.
    pub fn load_ref(&mut self, doc: &mut Document, anim_ref: Ref, requests: &mut Vec<NativePluginRequest>) {
        let frame_rate = self.state.frame_rate;
        let cls = doc.class_of(anim_ref).unwrap_or_default();
        let loaded = if cls == "CurveAnimation" {
            serialization::from_curve_animation(doc, anim_ref, frame_rate)
        } else {
            serialization::from_keyframe_sequence(doc, anim_ref, frame_rate)
        };
        match loaded {
            Ok(data) => {
                ops::load_animation_data(&mut self.state, data);
                self.step_rig(doc);
            }
            Err(e) => {
                self.state
                    .toast
                    .show_notice(format!("Failed to load animation: {e}"), now_secs());
                requests.push(NativePluginRequest::Log(
                    Level::Error,
                    format!("Animation Editor: {e}"),
                ));
            }
        }
    }

    /// Steps the attached rig in the `Document` to the current `playhead`.
    pub fn step_rig(&mut self, doc: &mut Document) {
        if self.state.rig_cache.root_ref.is_none() {
            return;
        }
        if let Some(data) = self.state.animation_data.as_ref() {
            rig::step_rig_animation(
                doc,
                &self.state.rig_cache,
                data,
                self.state.playback.playhead,
            );
        }
    }

    fn handle_menu_cmd(&mut self, cmd: MenuCommand, ctx: &mut NativeHostCtx<'_>) {
        match cmd {
            MenuCommand::LoadSaved(anim_ref) => self.load_ref(ctx.doc, anim_ref, ctx.requests),
            MenuCommand::Save => {
                if let Some(data) = self.state.animation_data.clone() {
                    let name = data.metadata.name.clone();
                    match rig::save_clip_to_anim_saves(
                        ctx.doc,
                        &mut self.state.rig_cache,
                        &data,
                        &name,
                    ) {
                        Ok(_) => {
                            self.state.is_dirty = false;
                            self.state
                                .toast
                                .show_notice(format!("Saved \"{name}\""), now_secs());
                        }
                        Err(e) => {
                            self.state
                                .toast
                                .show_notice(format!("Save failed: {e}"), now_secs());
                        }
                    }
                }
            }
            MenuCommand::SaveAs(name) => {
                if let Some(data) = self.state.animation_data.as_mut() {
                    data.metadata.name = name.clone();
                }
                if let Some(data) = self.state.animation_data.clone() {
                    match rig::save_clip_to_anim_saves(
                        ctx.doc,
                        &mut self.state.rig_cache,
                        &data,
                        &name,
                    ) {
                        Ok(_) => {
                            self.state.is_dirty = false;
                            self.state
                                .toast
                                .show_notice(format!("Saved as \"{name}\""), now_secs());
                        }
                        Err(e) => {
                            self.state
                                .toast
                                .show_notice(format!("Save failed: {e}"), now_secs());
                        }
                    }
                }
            }
            MenuCommand::CreateNew(name) => {
                ops::create_new_animation(&mut self.state, name);
                self.step_rig(ctx.doc);
            }
            MenuCommand::ImportFbx => {
                ctx.requests.push(NativePluginRequest::OpenFileDialog {
                    tag: TAG_IMPORT_FBX.to_string(),
                    title: "Import FBX Animation".to_string(),
                    default_name: None,
                    filters: vec![("FBX Animation".to_string(), vec!["fbx".to_string()])],
                    save: false,
                });
            }
            MenuCommand::ImportRbxm => {
                ctx.requests.push(NativePluginRequest::OpenFileDialog {
                    tag: TAG_IMPORT_RBXM.to_string(),
                    title: "Import Animation Clip".to_string(),
                    default_name: None,
                    filters: vec![(
                        "Roblox Model".to_string(),
                        vec!["rbxm".to_string(), "rbxmx".to_string()],
                    )],
                    save: false,
                });
            }
            MenuCommand::ExportRbxm => {
                let default_name = self
                    .state
                    .animation_data
                    .as_ref()
                    .map(|a| format!("{}.rbxm", a.metadata.name))
                    .unwrap_or_else(|| "Animation.rbxm".to_string());
                ctx.requests.push(NativePluginRequest::OpenFileDialog {
                    tag: TAG_EXPORT_RBXM.to_string(),
                    title: "Export Animation Clip".to_string(),
                    default_name: Some(default_name),
                    filters: vec![(
                        "Roblox Model".to_string(),
                        vec!["rbxm".to_string(), "rbxmx".to_string()],
                    )],
                    save: true,
                });
            }
            MenuCommand::OpenReduceKeyframes => {
                if let Some(orig) = self.state.animation_data.clone() {
                    let prioritized_ticks = reduction::compute_reduction_order(&orig);
                    let total = prioritized_ticks.len().max(2);
                    self.state.active_dialog =
                        ActiveDialog::ReduceKeyframes(ReduceKeyframesState {
                            mode: ReduceKeyframesDialogMode::FromMenu,
                            animation_backup: Some(orig.clone()),
                            source_data: Some(orig),
                            prioritized_ticks,
                            initial_keyframes: total,
                            target_keyframes: total,
                            has_moved: false,
                        });
                }
            }
            MenuCommand::PromoteToCurves => {
                ops::promote_to_curves(&mut self.state);
                self.step_rig(ctx.doc);
            }
            MenuCommand::SanitizeAnimation => {
                ops::sanitize_animation(&mut self.state);
                self.step_rig(ctx.doc);
                self.state
                    .toast
                    .show_notice("Animation sanitized", now_secs());
            }
            MenuCommand::SetPriority(prio) => {
                if self.state.animation_data.is_some() {
                    history::add_waypoint(&mut self.state);
                    if let Some(data) = self.state.animation_data.as_mut() {
                        data.metadata.priority = prio;
                    }
                    self.state.is_dirty = true;
                }
            }
            MenuCommand::DeleteSaved(anim_ref) => {
                ctx.doc.remove(&[anim_ref]);
                if let Some(root) = self.state.rig_cache.root_ref {
                    let (entries, is_legacy) = rig::list_anim_saves(ctx.doc, root);
                    self.state.rig_cache.anim_saves = entries
                        .iter()
                        .filter(|e| !e.is_legacy_anim_saves)
                        .cloned()
                        .collect();
                    self.state.rig_cache.legacy_anim_saves = entries
                        .into_iter()
                        .filter(|e| e.is_legacy_anim_saves)
                        .collect();
                    self.state.is_legacy_anim_saves = is_legacy;
                }
                self.state
                    .toast
                    .show_notice("Deleted saved animation", now_secs());
            }
        }
    }

    fn handle_dialog_outcome(&mut self, outcome: DialogOutcome, ctx: &mut NativeHostCtx<'_>) {
        match outcome {
            DialogOutcome::None => {}
            DialogOutcome::CreateNew(name) => {
                ops::create_new_animation(&mut self.state, name);
                self.step_rig(ctx.doc);
            }
            DialogOutcome::SaveAs(name) => {
                self.handle_menu_cmd(MenuCommand::SaveAs(name), ctx);
            }
            DialogOutcome::RenameKeyframe(tick, name) => {
                ops::rename_keyframe(&mut self.state, tick, name);
            }
            DialogOutcome::PromoteToCurves => {
                ops::promote_to_curves(&mut self.state);
                self.step_rig(ctx.doc);
            }
            DialogOutcome::ApplyReduction(reduced) => {
                history::add_waypoint(&mut self.state);
                self.state.animation_data = Some(reduced);
                selection::prune_invalid_selection(&mut self.state);
                selection::rebuild_track_list(&mut self.state);
                self.state.is_dirty = true;
                self.step_rig(ctx.doc);
            }
            DialogOutcome::ConfirmFbxImport {
                path,
                scale_factor,
                is_r15,
            } => {
                match serialization::import_fbx_file(
                    &path,
                    &self.state.rig_cache,
                    scale_factor,
                    is_r15,
                ) {
                    Ok(data) => {
                        ops::load_animation_data(&mut self.state, data);
                        self.state.is_dirty = true;
                        self.step_rig(ctx.doc);
                        self.state.toast.show_notice(
                            format!(
                                "Imported FBX: {}",
                                path.file_name().unwrap_or_default().to_string_lossy()
                            ),
                            now_secs(),
                        );
                    }
                    Err(e) => {
                        self.state
                            .toast
                            .show_notice(format!("FBX import failed: {e}"), now_secs());
                        ctx.requests.push(NativePluginRequest::Log(
                            Level::Error,
                            format!("FBX import failed: {e}"),
                        ));
                    }
                }
            }
            DialogOutcome::ApplyMigration(choices) => {
                rig::migrate_legacy_anim_saves(ctx.doc, &mut self.state.rig_cache, &choices);
                self.state
                    .toast
                    .show_notice("Legacy AnimSaves migration complete", now_secs());
            }
        }
    }
}

impl NativePlugin for AnimationEditorPlugin {
    fn meta(&self) -> NativePluginMeta {
        META
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn on_unload(&mut self, ctx: &mut NativeHostCtx<'_>) {
        if self.state.active || self.state.rig_cache.root_ref.is_some() {
            self.release(ctx.doc);
        }
    }

    fn on_place_changed(&mut self, ctx: &mut NativeHostCtx<'_>) {
        if self.state.active || self.state.rig_cache.root_ref.is_some() {
            self.release(ctx.doc);
        }
        self.state.last_synced_selection.clear();
    }

    fn on_widget_close(&mut self, ctx: &mut NativeHostCtx<'_>) {
        if self.state.active {
            self.release(ctx.doc);
        }
        self.state.last_synced_selection.clear();
    }

    fn tick(&mut self, ctx: &mut NativeHostCtx<'_>, widget_open: bool) {
        if self.state.active && !widget_open {
            self.release(ctx.doc);
        }
        self.sync_selection(ctx, widget_open);
    }

    fn render_widget(&mut self, ui: &mut Ui, ctx: &mut NativeHostCtx<'_>) {
        let dt = ui.ctx().input(|i| i.stable_dt).min(0.1);
        let res = ui_main::show_animation_editor_panel(ui, &mut self.state, ctx.is_playing, dt);

        if res.needs_rig_step {
            self.step_rig(ctx.doc);
        }
        if res.focus_face_camera {
            if let Some((cam_pos, look_at)) =
                facs::focus_camera_on_face_target(ctx.doc, &self.state.rig_cache)
            {
                ctx.cam.pos = cam_pos;
                let dir = (look_at - cam_pos).normalize_or_zero();
                if dir.length_squared() > 1e-6 {
                    ctx.cam.yaw = (-dir.x).atan2(-dir.z);
                    ctx.cam.pitch = dir.y.clamp(-0.99, 0.99).asin();
                }
            }
        }

        if let Some(cmd) = res.menu_cmd {
            self.handle_menu_cmd(cmd, ctx);
        }
        self.handle_dialog_outcome(res.dialog_outcome, ctx);
    }

    fn owns_viewport(&self, widget_open: bool, is_playing: bool) -> bool {
        widget_open && !is_playing
    }

    fn suppress_selection_outline(&self, widget_open: bool, is_playing: bool) -> bool {
        self.owns_viewport(widget_open, is_playing) && self.state.active
    }

    fn append_viewport_lines(
        &self,
        doc: &Document,
        widget_open: bool,
        is_playing: bool,
        lines: &mut Vec<LineVertex>,
    ) {
        if !self.owns_viewport(widget_open, is_playing)
            || !self.state.settings.visualize_grid
            || self.state.rig_cache.root_ref.is_none()
        {
            return;
        }
        let grid_offset = playback::compute_grid_offset(&self.state);
        rig_viz::append_floor_grid_lines(lines, doc, &self.state.rig_cache, grid_offset);
    }

    fn render_viewport(&mut self, vctx: &mut NativeViewportCtx<'_>, widget_open: bool) -> bool {
        if !self.owns_viewport(widget_open, vctx.is_playing) {
            return false;
        }
        let action = viewport_dragger::handle_viewport_interaction(
            vctx.ctx,
            vctx.painter,
            vctx.rect,
            vctx.resp,
            vctx.cam,
            vctx.scene,
            vctx.doc,
            &mut self.state,
            vctx.snap_move,
            vctx.snap_rot,
        );
        if let Some(rig_ref) = action.attach_rig {
            vctx.doc.selection = vec![rig_ref];
            vctx.requests.push(NativePluginRequest::Select(vec![rig_ref]));
            self.state.last_synced_selection = vctx.doc.selection.clone();
            self.attach_rig(vctx.doc, rig_ref, vctx.requests);
        }
        if action.pose_changed {
            self.step_rig(vctx.doc);
        }
        action.consumed_pointer
    }

    fn on_file_dialog(&mut self, tag: &str, path: PathBuf, ctx: &mut NativeHostCtx<'_>) {
        match tag {
            TAG_IMPORT_FBX => {
                self.state.active_dialog =
                    ActiveDialog::ImportSettings(ImportSettingsDialogState {
                        fbx_path: Some(path),
                        rig_type_index: usize::from(
                            self.state.rig_cache.rig_type != RigTypeKind::R15,
                        ),
                        ..Default::default()
                    });
            }
            TAG_IMPORT_RBXM => {
                match serialization::import_clip_from_rbxm(&path, self.state.frame_rate) {
                    Ok(data) => {
                        ops::load_animation_data(&mut self.state, data);
                        self.state.is_dirty = true;
                        self.step_rig(ctx.doc);
                        self.state
                            .toast
                            .show_notice("Imported animation clip", now_secs());
                    }
                    Err(e) => {
                        self.state
                            .toast
                            .show_notice(format!("Import failed: {e}"), now_secs());
                        ctx.requests.push(NativePluginRequest::Log(
                            Level::Error,
                            format!("Animation import failed: {e}"),
                        ));
                    }
                }
            }
            TAG_EXPORT_RBXM => {
                if let Some(data) = self.state.animation_data.as_ref() {
                    let rig_opt = self
                        .state
                        .rig_cache
                        .root_ref
                        .is_some()
                        .then_some(&self.state.rig_cache);
                    match serialization::export_clip_to_rbxm(ctx.doc, data, rig_opt, &path) {
                        Ok(()) => {
                            self.state.toast.show_notice(
                                format!("Exported to {}", path.display()),
                                now_secs(),
                            );
                        }
                        Err(e) => {
                            self.state
                                .toast
                                .show_notice(format!("Export failed: {e}"), now_secs());
                            ctx.requests.push(NativePluginRequest::Log(
                                Level::Error,
                                format!("Animation export failed: {e}"),
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

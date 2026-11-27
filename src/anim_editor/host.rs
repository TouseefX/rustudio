//! Host integration between RusTudio's [`Editor`] and the native
//! [`AnimationEditor`] (`AnimationClipEditor` 1:1 port).

use std::path::PathBuf;

use egui::{Painter, Rect, Response, Ui};
use rbx_dom_weak::types::Ref;

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
    viewport_dragger,
};
use crate::app::{Editor, Tab};
use crate::file_dialog::{Kind, Purpose};

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

impl Editor {
    /// True when the native Animation Editor is enabled (not overridden to
    /// Roblox's built-in plugin) and its panel is currently open.
    pub fn anim_editor_open(&self) -> bool {
        self.builtin_tools.use_native(BuiltinTool::AnimationEditor)
            && self.ui.panels_open.contains("Animation Editor")
    }

    /// True when the native Animation Editor owns 3D viewport picking/draggers.
    pub fn anim_editor_owns_viewport(&self) -> bool {
        self.anim_editor_open() && !self.game.is_playing()
    }

    /// Attaches a rig `Model` to the Animation Editor: validates with all 9
    /// `RigValidation` rules, populates `RigCache`, checks legacy `AnimSaves`,
    /// and loads the latest saved animation (or prepares `StartScreen`).
    pub fn anim_editor_attach_rig(&mut self, model: Ref) {
        if self.anim_editor.rig_cache.root_ref == Some(model) && self.anim_editor.active {
            return;
        }
        // Release any previous rig cleanly first.
        if self.anim_editor.rig_cache.root_ref.is_some() {
            self.anim_editor_release();
        }

        let issues = rig::validate_rig(&self.game.doc, model);
        let has_errors = issues.iter().any(|i| i.severity == ValidationSeverity::Error);
        if has_errors {
            self.anim_editor.active_dialog = ActiveDialog::RigErrors {
                title: "Rig Errors".to_string(),
                issues,
            };
            return;
        }

        let cache = rig::build_rig_cache(&self.game.doc, model);
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

        self.anim_editor.rig_cache = cache;
        self.anim_editor.active = true;
        self.anim_editor.ik.enabled = false;
        self.anim_editor.ik.pinned_parts.clear();
        self.anim_editor.viewport_dragger.hovered_part = None;

        if let Some(entry) = latest {
            self.anim_editor_load_ref(entry.anim_ref);
        } else {
            self.anim_editor.animation_data = None;
            self.anim_editor.tracks.clear();
            selection::rebuild_track_list(&mut self.anim_editor);
        }

        if !legacy_items.is_empty() {
            self.anim_editor.active_dialog = ActiveDialog::Migration {
                items: legacy_items,
            };
        }
    }

    /// Detaches the Animation Editor from the active rig: autosaves
    /// `"Automatic Save"` when dirty, restores rest poses, and clears active state.
    pub fn anim_editor_release(&mut self) {
        if self.anim_editor.is_dirty {
            if let Some(data) = self.anim_editor.animation_data.clone() {
                let _ = rig::save_clip_to_anim_saves(
                    &mut self.game.doc,
                    &mut self.anim_editor.rig_cache,
                    &data,
                    AUTOMATIC_SAVE_NAME,
                );
            }
        }
        if self.anim_editor.rig_cache.root_ref.is_some() {
            rig::reset_rig_pose(&mut self.game.doc, &self.anim_editor.rig_cache);
        }
        self.anim_editor.active = false;
        self.anim_editor.playback.set_play_state(PlayState::Pause);
        self.anim_editor.rig_cache.clear();
        self.anim_editor.animation_data = None;
        self.anim_editor.tracks.clear();
        self.anim_editor.selected_keyframes.clear();
        self.anim_editor.selected_events.clear();
        self.anim_editor.selected_tracks.clear();
        self.anim_editor.selected_track_instances.clear();
        self.anim_editor.is_dirty = false;
        self.anim_editor.drag_context = None;
        history::clear_history(&mut self.anim_editor);
    }

    /// Called each frame when the Animation Editor is open to attach or switch
    /// to a rig whenever the place selection changes (e.g. clicking a rig in
    /// Explorer). Selecting parts/joints inside the already-attached rig is
    /// ignored so editing is never interrupted.
    pub fn anim_editor_sync_selection(&mut self) {
        if !self.anim_editor_open() || self.game.is_playing() {
            if !self.anim_editor_open() {
                self.anim_editor.last_synced_selection.clear();
            }
            return;
        }
        if self.anim_editor.last_synced_selection == self.game.doc.selection {
            return;
        }
        self.anim_editor.last_synced_selection = self.game.doc.selection.clone();

        for &r in &self.game.doc.selection {
            let mut candidate = None;
            let mut cur = r;
            while let Some(inst) = self.game.doc.dom.get_by_ref(cur) {
                if rig::is_valid_rig_candidate(&self.game.doc, cur) {
                    candidate = Some(cur);
                }
                let parent = inst.parent();
                if parent.is_none() || cur == self.game.doc.dom.root_ref() {
                    break;
                }
                cur = parent;
            }
            if let Some(rig_model) = candidate {
                if self.anim_editor.rig_cache.root_ref != Some(rig_model) {
                    self.anim_editor_attach_rig(rig_model);
                }
                break;
            }
        }
    }

    /// Loads a `KeyframeSequence` or `CurveAnimation` instance by `Ref`.
    pub fn anim_editor_load_ref(&mut self, anim_ref: Ref) {
        let frame_rate = self.anim_editor.frame_rate;
        let cls = self.game.doc.class_of(anim_ref).unwrap_or_default();
        let loaded = if cls == "CurveAnimation" {
            serialization::from_curve_animation(&self.game.doc, anim_ref, frame_rate)
        } else {
            serialization::from_keyframe_sequence(&self.game.doc, anim_ref, frame_rate)
        };
        match loaded {
            Ok(data) => {
                ops::load_animation_data(&mut self.anim_editor, data);
                self.anim_editor_step_rig();
            }
            Err(e) => {
                self.anim_editor
                    .toast
                    .show_notice(format!("Failed to load animation: {e}"), now_secs());
                self.error(format!("Animation Editor: {e}"));
            }
        }
    }

    /// Steps the attached rig in the `Document` to the current `playhead`.
    pub fn anim_editor_step_rig(&mut self) {
        if self.anim_editor.rig_cache.root_ref.is_none() {
            return;
        }
        if let Some(data) = self.anim_editor.animation_data.as_ref() {
            rig::step_rig_animation(
                &mut self.game.doc,
                &self.anim_editor.rig_cache,
                data,
                self.anim_editor.playback.playhead,
            );
        }
    }

    /// Renders the docked/popped Animation Editor panel and handles emitted commands.
    pub fn anim_editor_ui(&mut self, ui: &mut Ui) {
        let dt = ui.ctx().input(|i| i.stable_dt).min(0.1);
        let is_playing = self.game.is_playing();
        let res = ui_main::show_animation_editor_panel(ui, &mut self.anim_editor, is_playing, dt);

        if res.needs_rig_step {
            self.anim_editor_step_rig();
        }
        if res.focus_face_camera {
            if let Some((cam_pos, look_at)) =
                facs::focus_camera_on_face_target(&self.game.doc, &self.anim_editor.rig_cache)
            {
                self.game.cam.pos = cam_pos;
                let dir = (look_at - cam_pos).normalize_or_zero();
                if dir.length_squared() > 1e-6 {
                    self.game.cam.yaw = (-dir.x).atan2(-dir.z);
                    self.game.cam.pitch = dir.y.clamp(-0.99, 0.99).asin();
                }
            }
        }

        if let Some(cmd) = res.menu_cmd {
            self.handle_anim_menu_cmd(cmd);
        }
        self.handle_anim_dialog_outcome(res.dialog_outcome);
    }

    fn handle_anim_menu_cmd(&mut self, cmd: MenuCommand) {
        match cmd {
            MenuCommand::LoadSaved(anim_ref) => self.anim_editor_load_ref(anim_ref),
            MenuCommand::Save => {
                if let Some(data) = self.anim_editor.animation_data.clone() {
                    let name = data.metadata.name.clone();
                    match rig::save_clip_to_anim_saves(
                        &mut self.game.doc,
                        &mut self.anim_editor.rig_cache,
                        &data,
                        &name,
                    ) {
                        Ok(_) => {
                            self.anim_editor.is_dirty = false;
                            self.anim_editor
                                .toast
                                .show_notice(format!("Saved \"{name}\""), now_secs());
                        }
                        Err(e) => {
                            self.anim_editor
                                .toast
                                .show_notice(format!("Save failed: {e}"), now_secs());
                        }
                    }
                }
            }
            MenuCommand::SaveAs(name) => {
                if let Some(data) = self.anim_editor.animation_data.as_mut() {
                    data.metadata.name = name.clone();
                }
                if let Some(data) = self.anim_editor.animation_data.clone() {
                    match rig::save_clip_to_anim_saves(
                        &mut self.game.doc,
                        &mut self.anim_editor.rig_cache,
                        &data,
                        &name,
                    ) {
                        Ok(_) => {
                            self.anim_editor.is_dirty = false;
                            self.anim_editor
                                .toast
                                .show_notice(format!("Saved as \"{name}\""), now_secs());
                        }
                        Err(e) => {
                            self.anim_editor
                                .toast
                                .show_notice(format!("Save failed: {e}"), now_secs());
                        }
                    }
                }
            }
            MenuCommand::CreateNew(name) => {
                ops::create_new_animation(&mut self.anim_editor, name);
                self.anim_editor_step_rig();
            }
            MenuCommand::ImportFbx => {
                // (crate::file_dialog::Request: the desktop's rfd dialog,
                // Android's own picker — src/android/storage.rs)
                let d = crate::file_dialog::Request::new()
                    .set_title("Import FBX Animation")
                    .add_filter("FBX Animation", &["fbx"]);
                self.dialogs.start(Purpose::AnimImportFbx, d, Kind::PickFile);
            }
            MenuCommand::ImportRbxm => {
                let d = crate::file_dialog::Request::new()
                    .set_title("Import Animation Clip")
                    .add_filter("Roblox Model", &["rbxm", "rbxmx"]);
                self.dialogs
                    .start(Purpose::AnimImportRbxm, d, Kind::PickFile);
            }
            MenuCommand::ExportRbxm => {
                let default_name = self
                    .anim_editor
                    .animation_data
                    .as_ref()
                    .map(|a| format!("{}.rbxm", a.metadata.name))
                    .unwrap_or_else(|| "Animation.rbxm".to_string());
                let d = crate::file_dialog::Request::new()
                    .set_title("Export Animation Clip")
                    .set_file_name(&default_name)
                    .add_filter("Roblox Model", &["rbxm", "rbxmx"]);
                self.dialogs
                    .start(Purpose::AnimExportRbxm, d, Kind::SaveFile);
            }
            MenuCommand::OpenReduceKeyframes => {
                if let Some(orig) = self.anim_editor.animation_data.clone() {
                    let prioritized_ticks = reduction::compute_reduction_order(&orig);
                    let total = prioritized_ticks.len().max(2);
                    self.anim_editor.active_dialog =
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
                ops::promote_to_curves(&mut self.anim_editor);
                self.anim_editor_step_rig();
            }
            MenuCommand::SanitizeAnimation => {
                ops::sanitize_animation(&mut self.anim_editor);
                self.anim_editor_step_rig();
                self.anim_editor
                    .toast
                    .show_notice("Animation sanitized", now_secs());
            }
            MenuCommand::SetPriority(prio) => {
                if self.anim_editor.animation_data.is_some() {
                    history::add_waypoint(&mut self.anim_editor);
                    if let Some(data) = self.anim_editor.animation_data.as_mut() {
                        data.metadata.priority = prio;
                    }
                    self.anim_editor.is_dirty = true;
                }
            }
            MenuCommand::DeleteSaved(anim_ref) => {
                self.game.doc.remove(&[anim_ref]);
                if let Some(root) = self.anim_editor.rig_cache.root_ref {
                    let (entries, is_legacy) = rig::list_anim_saves(&self.game.doc, root);
                    self.anim_editor.rig_cache.anim_saves = entries
                        .iter()
                        .filter(|e| !e.is_legacy_anim_saves)
                        .cloned()
                        .collect();
                    self.anim_editor.rig_cache.legacy_anim_saves = entries
                        .into_iter()
                        .filter(|e| e.is_legacy_anim_saves)
                        .collect();
                    self.anim_editor.is_legacy_anim_saves = is_legacy;
                }
                self.anim_editor
                    .toast
                    .show_notice("Deleted saved animation", now_secs());
            }
        }
    }

    fn handle_anim_dialog_outcome(&mut self, outcome: DialogOutcome) {
        match outcome {
            DialogOutcome::None => {}
            DialogOutcome::CreateNew(name) => {
                ops::create_new_animation(&mut self.anim_editor, name);
                self.anim_editor_step_rig();
            }
            DialogOutcome::SaveAs(name) => {
                self.handle_anim_menu_cmd(MenuCommand::SaveAs(name));
            }
            DialogOutcome::RenameKeyframe(tick, name) => {
                ops::rename_keyframe(&mut self.anim_editor, tick, name);
            }
            DialogOutcome::PromoteToCurves => {
                ops::promote_to_curves(&mut self.anim_editor);
                self.anim_editor_step_rig();
            }
            DialogOutcome::ApplyReduction(reduced) => {
                history::add_waypoint(&mut self.anim_editor);
                self.anim_editor.animation_data = Some(reduced);
                selection::prune_invalid_selection(&mut self.anim_editor);
                selection::rebuild_track_list(&mut self.anim_editor);
                self.anim_editor.is_dirty = true;
                self.anim_editor_step_rig();
            }
            DialogOutcome::ConfirmFbxImport {
                path,
                scale_factor,
                is_r15,
            } => {
                match serialization::import_fbx_file(
                    &path,
                    &self.anim_editor.rig_cache,
                    scale_factor,
                    is_r15,
                ) {
                    Ok(data) => {
                        ops::load_animation_data(&mut self.anim_editor, data);
                        self.anim_editor.is_dirty = true;
                        self.anim_editor_step_rig();
                        self.anim_editor.toast.show_notice(
                            format!(
                                "Imported FBX: {}",
                                path.file_name().unwrap_or_default().to_string_lossy()
                            ),
                            now_secs(),
                        );
                    }
                    Err(e) => {
                        self.anim_editor
                            .toast
                            .show_notice(format!("FBX import failed: {e}"), now_secs());
                        self.error(format!("FBX import failed: {e}"));
                    }
                }
            }
            DialogOutcome::ApplyMigration(choices) => {
                rig::migrate_legacy_anim_saves(
                    &mut self.game.doc,
                    &mut self.anim_editor.rig_cache,
                    &choices,
                );
                self.anim_editor
                    .toast
                    .show_notice("Legacy AnimSaves migration complete", now_secs());
            }
        }
    }

    /// Handles completed file dialogs for Animation Editor import/export.
    pub fn anim_editor_handle_file(&mut self, purpose: &Purpose, path: PathBuf) {
        match purpose {
            Purpose::AnimImportFbx => {
                self.anim_editor.active_dialog =
                    ActiveDialog::ImportSettings(ImportSettingsDialogState {
                        fbx_path: Some(path),
                        rig_type_index: usize::from(
                            self.anim_editor.rig_cache.rig_type != RigTypeKind::R15,
                        ),
                        ..Default::default()
                    });
            }
            Purpose::AnimImportRbxm => {
                match serialization::import_clip_from_rbxm(&path, self.anim_editor.frame_rate) {
                    Ok(data) => {
                        ops::load_animation_data(&mut self.anim_editor, data);
                        self.anim_editor.is_dirty = true;
                        self.anim_editor_step_rig();
                        self.anim_editor
                            .toast
                            .show_notice("Imported animation clip", now_secs());
                    }
                    Err(e) => {
                        self.anim_editor
                            .toast
                            .show_notice(format!("Import failed: {e}"), now_secs());
                        self.error(format!("Animation import failed: {e}"));
                    }
                }
            }
            Purpose::AnimExportRbxm => {
                if let Some(data) = self.anim_editor.animation_data.as_ref() {
                    let rig_opt = self
                        .anim_editor
                        .rig_cache
                        .root_ref
                        .is_some()
                        .then_some(&self.anim_editor.rig_cache);
                    match serialization::export_clip_to_rbxm(&mut self.game.doc, data, rig_opt, &path)
                    {
                        Ok(()) => {
                            self.anim_editor.toast.show_notice(
                                format!("Exported to {}", path.display()),
                                now_secs(),
                            );
                        }
                        Err(e) => {
                            self.anim_editor
                                .toast
                                .show_notice(format!("Export failed: {e}"), now_secs());
                            self.error(format!("Animation export failed: {e}"));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Appends the Animation Editor's scrolling `FloorGrid` lines to the 3D viewport line buffer.
    pub fn anim_editor_floor_grid_lines(&self, lines: &mut Vec<LineVertex>) {
        if !self.anim_editor_owns_viewport()
            || !self.anim_editor.settings.visualize_grid
            || self.anim_editor.rig_cache.root_ref.is_none()
        {
            return;
        }
        let grid_offset = playback::compute_grid_offset(&self.anim_editor);
        rig_viz::append_floor_grid_lines(
            lines,
            &self.game.doc,
            &self.anim_editor.rig_cache,
            grid_offset,
        );
    }

    /// Runs the Animation Editor's 3D viewport draggers, joint/bone picking,
    /// microbone visualization, and SelectionBox adornments. Returns `true` if
    /// pointer input was consumed.
    pub fn anim_editor_viewport_ui(
        &mut self,
        ctx: &egui::Context,
        painter: &Painter,
        rect: Rect,
        resp: &Response,
    ) -> bool {
        if !self.anim_editor_owns_viewport() {
            return false;
        }
        let snap_move = self.game.snap_move.then_some(self.game.move_inc);
        let snap_rot = self.game.snap_rotate.then_some(self.game.rotate_inc);
        let action = viewport_dragger::handle_viewport_interaction(
            ctx,
            painter,
            rect,
            resp,
            &self.game.cam,
            &self.game.scene,
            &mut self.game.doc,
            &mut self.anim_editor,
            snap_move,
            snap_rot,
        );
        if let Some(rig_ref) = action.attach_rig {
            self.select(vec![rig_ref]);
            self.anim_editor.last_synced_selection = self.game.doc.selection.clone();
            self.anim_editor_attach_rig(rig_ref);
        }
        if action.pose_changed {
            self.anim_editor_step_rig();
        }
        action.consumed_pointer
    }

    /// Called at the end of each frame after `panels_open` is refreshed:
    /// if the Animation Editor panel was closed, cleanly release the rig!
    pub fn anim_editor_check_closed(&mut self) {
        if self.anim_editor.active && !self.anim_editor_open() {
            self.anim_editor_release();
        }
    }
}

/// Ensures `Tab::AnimationEditor` is referenced so dead-code checks stay clean.
#[allow(dead_code)]
pub const ANIM_EDITOR_TAB: Tab = Tab::AnimationEditor;

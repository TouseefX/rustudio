//! 1:1 Dialogs, Prompts, Covers, Start Screen, and Toasts for `AnimationClipEditor`.

use std::path::PathBuf;

use egui::{Align, Align2, Color32, Context, Id, Layout, Pos2, Rect, RichText, Stroke, Ui, Vec2};
use rbx_dom_weak::types::Ref;

use crate::anim_editor::constants::{
    DEFAULT_ANIMATION_NAME, MAX_FRAMERATE, MAX_PLAYBACK_SPEED, MIN_FRAMERATE, MIN_PLAYBACK_SPEED,
    SCALE_UNITS,
};
use crate::anim_editor::model::{AnimationData, Tick};
use crate::anim_editor::state::{
    ActiveDialog, EditorState, GridSpeedSetting, MigrationChoice, ToastKind, ValidationSeverity,
};
use crate::anim_editor::theme::AceTheme;

/// Result emitted when a modal dialog is confirmed.
#[derive(Debug, Clone, Default)]
pub enum DialogOutcome {
    #[default]
    None,
    CreateNew(String),
    SaveAs(String),
    RenameKeyframe(Tick, String),
    PromoteToCurves,
    ApplyReduction(AnimationData),
    ConfirmFbxImport {
        path: PathBuf,
        scale_factor: f32,
        is_r15: bool,
    },
    ApplyMigration(Vec<(Ref, MigrationChoice)>),
}

/// Exact `Components/StartScreen`:
/// - When no rig is selected: displays "Select a rig to animate..."
/// - When a rig is selected with no clip: displays rig name, animation name field, and "Create" button.
pub fn show_start_screen(ui: &mut Ui, state: &mut EditorState, theme: &AceTheme) -> Option<String> {
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(rect, 0.0, theme.background);

    if state.rig_cache.root_ref.is_none() {
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new(
                        "Select a rig to animate or create a new rig by selecting Rig Builder under the Avatar tab",
                    )
                    .size(theme.font_size_dialog)
                    .color(theme.start_screen_text),
                );
            });
        });
        return None;
    }

    let name_id = Id::new("ace_start_screen_clip_name");
    let mut clip_name = ui.ctx().data_mut(|d| {
        d.get_temp::<String>(name_id)
            .unwrap_or_else(|| DEFAULT_ANIMATION_NAME.to_string())
    });

    let mut created: Option<String> = None;

    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.vertical_centered(|ui| {
            let pad_top = ((rect.height() - 110.0) * 0.5).max(12.0);
            ui.add_space(pad_top);

            ui.label(
                RichText::new(format!("Rig: {}", state.rig_cache.root_name))
                    .size(theme.font_size_dialog_header)
                    .color(theme.bright_text)
                    .strong(),
            );
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                let row_w = 340.0_f32.min(rect.width() - 24.0);
                let left_margin = ((rect.width() - row_w) * 0.5).max(0.0);
                ui.add_space(left_margin);
                ui.label(
                    RichText::new("Animation Name:")
                        .size(theme.font_size_dialog)
                        .color(theme.start_screen_text),
                );
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut clip_name)
                        .desired_width((row_w - 130.0).max(100.0)),
                );
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let trimmed = clip_name.trim().to_string();
                    if !trimmed.is_empty() {
                        created = Some(trimmed);
                    }
                }
            });

            ui.add_space(10.0);

            let can_create = !clip_name.trim().is_empty();
            if ui
                .add_enabled(
                    can_create,
                    egui::Button::new(
                        RichText::new("Create")
                            .size(theme.font_size_dialog)
                            .color(theme.accent_text),
                    )
                    .fill(theme.accent)
                    .min_size(Vec2::new(100.0, 26.0)),
                )
                .clicked()
            {
                created = Some(clip_name.trim().to_string());
            }
        });
    });

    ui.ctx().data_mut(|d| d.insert_temp(name_id, clip_name));
    created
}

/// Exact `Components/InactiveCover`: dimmed overlay blocking interaction.
pub fn show_inactive_cover(ui: &mut Ui, message: &str, theme: &AceTheme) {
    let rect = ui.available_rect_before_wrap();
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme.overlay_dim);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        message,
        egui::FontId::proportional(theme.font_size_dialog),
        theme.start_screen_dark_text,
    );
}

/// Renders the currently active modal dialog (if any) and returns a [`DialogOutcome`].
pub fn show_active_dialog(
    ctx: &Context,
    state: &mut EditorState,
    theme: &AceTheme,
) -> DialogOutcome {
    if matches!(
        state.active_dialog,
        ActiveDialog::None | ActiveDialog::EditEvents(_)
    ) {
        return DialogOutcome::None;
    }

    let mut outcome = DialogOutcome::None;
    let mut close_dialog = false;
    let mut dialog = std::mem::take(&mut state.active_dialog);

    match &mut dialog {
        ActiveDialog::None | ActiveDialog::EditEvents(_) => {}

        ActiveDialog::CreateNewAnimation { name, error } => {
            show_text_entry_window(
                ctx,
                "Create New Animation",
                "Enter a name for the new animation clip:",
                "Animation Name",
                name,
                error,
                "Create",
                theme,
                &mut |submitted| {
                    let trimmed = submitted.trim().to_string();
                    if trimmed.is_empty() {
                        Err("Animation name cannot be empty.".to_string())
                    } else {
                        outcome = DialogOutcome::CreateNew(trimmed);
                        Ok(())
                    }
                },
                &mut close_dialog,
            );
        }

        ActiveDialog::SaveAs { name, error } => {
            show_text_entry_window(
                ctx,
                "Save Animation As",
                "Enter a name to save this animation clip:",
                "Animation Name",
                name,
                error,
                "Save",
                theme,
                &mut |submitted| {
                    let trimmed = submitted.trim().to_string();
                    if trimmed.is_empty() {
                        Err("Animation name cannot be empty.".to_string())
                    } else {
                        outcome = DialogOutcome::SaveAs(trimmed);
                        Ok(())
                    }
                },
                &mut close_dialog,
            );
        }

        ActiveDialog::OverwriteConfirm { name, .. } => {
            egui::Window::new("Overwrite Animation")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .fixed_size(Vec2::new(360.0, 120.0))
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(format!(
                            "An animation named \"{name}\" already exists. Overwrite it?"
                        ))
                        .color(theme.dialog_text),
                    );
                    ui.add_space(12.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Overwrite").color(theme.accent_text),
                                )
                                .fill(theme.accent)
                                .min_size(Vec2::new(90.0, 25.0)),
                            )
                            .clicked()
                        {
                            outcome = DialogOutcome::SaveAs(name.clone());
                            close_dialog = true;
                        }
                        if ui
                            .add(egui::Button::new("Cancel").min_size(Vec2::new(90.0, 25.0)))
                            .clicked()
                        {
                            close_dialog = true;
                        }
                    });
                });
        }

        ActiveDialog::RenameKeyframe { tick, name } => {
            let mut dummy_err = None;
            let t = *tick;
            show_text_entry_window(
                ctx,
                "Rename Keyframe",
                "Enter a name for this keyframe:",
                "Keyframe Name",
                name,
                &mut dummy_err,
                "Confirm",
                theme,
                &mut |submitted| {
                    outcome = DialogOutcome::RenameKeyframe(t, submitted.trim().to_string());
                    Ok(())
                },
                &mut close_dialog,
            );
        }

        ActiveDialog::ChangeDuration {
            text,
            current_duration_frames,
            error,
        } => {
            let current_frames = *current_duration_frames;
            show_text_entry_window(
                ctx,
                "Change Duration",
                &format!("Current duration: {current_frames} frames"),
                "New Duration (frames)",
                text,
                error,
                "Confirm",
                theme,
                &mut |submitted| match submitted.trim().parse::<f32>() {
                    Ok(v) if v >= 1.0 => {
                        let ticks = ((v * crate::anim_editor::constants::TICK_FREQUENCY as f32)
                            / state.frame_rate.max(1) as f32)
                            .round() as Tick;
                        state.playback.editing_length = ticks.clamp(
                            crate::anim_editor::constants::MIN_TIMELINE_RANGE,
                            crate::anim_editor::constants::MAX_ANIMATION_LENGTH,
                        );
                        Ok(())
                    }
                    _ => Err("Enter a valid positive number of frames.".to_string()),
                },
                &mut close_dialog,
            );
        }

        ActiveDialog::ChangeFps { text, error } => {
            show_text_entry_window(
                ctx,
                "Custom Frame Rate",
                " ",
                "Custom Frame Rate [FPS]",
                text,
                error,
                "Confirm",
                theme,
                &mut |submitted| {
                    let trimmed = submitted.trim();
                    if trimmed.is_empty() {
                        return Err("Enter a frame rate.".to_string());
                    }
                    let parsed = trimmed
                        .parse::<f64>()
                        .map_err(|_| "Enter a valid number.".to_string())?;
                    let ceiled = parsed.ceil() as i64;
                    if ceiled < MIN_FRAMERATE as i64 {
                        return Err("Minimum frame rate is 1 fps".to_string());
                    }
                    if ceiled > MAX_FRAMERATE as i64 {
                        return Err("Maximum frame rate is 120 fps".to_string());
                    }
                    let fps = ceiled as u32;
                    state.frame_rate = fps;
                    if let Some(anim) = &mut state.animation_data {
                        anim.metadata.frame_rate = fps;
                    }
                    Ok(())
                },
                &mut close_dialog,
            );
        }

        ActiveDialog::ChangeGridSpeed { text, error } => {
            show_text_entry_window(
                ctx,
                "Custom Grid Speed",
                " ",
                "Custom Grid Speed [studs/s]",
                text,
                error,
                "Confirm",
                theme,
                &mut |submitted| {
                    let trimmed = submitted.trim();
                    if trimmed.is_empty() {
                        return Err("Enter a grid speed.".to_string());
                    }
                    let parsed = trimmed
                        .parse::<f64>()
                        .map_err(|_| "Enter a valid number.".to_string())?;
                    let ceiled = parsed.ceil() as f32;
                    state.settings.grid_speed = GridSpeedSetting::Custom(ceiled);
                    let _ = state.settings.save();
                    Ok(())
                },
                &mut close_dialog,
            );
        }

        ActiveDialog::ChangePlaybackSpeed { text, error } => {
            show_text_entry_window(
                ctx,
                "Custom Playback Speed",
                " ",
                "Custom Playback Speed",
                text,
                error,
                "Confirm",
                theme,
                &mut |submitted| {
                    let trimmed = submitted.trim();
                    let stripped = trimmed
                        .strip_suffix('x')
                        .or_else(|| trimmed.strip_suffix('X'))
                        .unwrap_or(trimmed)
                        .trim();
                    if stripped.is_empty() {
                        return Err("Enter a playback speed.".to_string());
                    }
                    let parsed = stripped
                        .parse::<f64>()
                        .map_err(|_| "Enter a valid number.".to_string())?;
                    let floored = ((parsed * 100.0).floor() / 100.0) as f32;
                    if floored < MIN_PLAYBACK_SPEED {
                        return Err("Minimum playback speed is 0.01x".to_string());
                    }
                    if floored > MAX_PLAYBACK_SPEED {
                        return Err("Maximum playback speed is 10x".to_string());
                    }
                    state.playback.playback_speed = floored;
                    state.playback.start_time_secs = None;
                    Ok(())
                },
                &mut close_dialog,
            );
        }

        ActiveDialog::PromoteToCurves => {
            egui::Window::new("Promote to Curves")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .fixed_size(Vec2::new(340.0, 180.0))
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(
                            "Promoting this animation to curves will convert all tracks into curve channels.\n\nCurve animations cannot be downgraded back to a KeyframeSequence.",
                        )
                        .color(theme.dialog_text),
                    );
                    ui.add_space(16.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Confirm").color(theme.accent_text),
                                )
                                .fill(theme.accent)
                                .min_size(Vec2::new(100.0, 25.0)),
                            )
                            .clicked()
                        {
                            outcome = DialogOutcome::PromoteToCurves;
                            close_dialog = true;
                        }
                        if ui
                            .add(egui::Button::new("Cancel").min_size(Vec2::new(100.0, 25.0)))
                            .clicked()
                        {
                            close_dialog = true;
                        }
                    });
                });
        }

        ActiveDialog::ReduceKeyframes(rk) => {
            egui::Window::new("Optimize Keyframes")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .fixed_size(Vec2::new(400.0, 120.0))
                .show(ctx, |ui| {
                    let total = rk.prioritized_ticks.len().max(rk.initial_keyframes);
                    let max_slider = total.max(3);
                    let min_slider = 2usize;
                    let enabled = total > 2;
                    rk.target_keyframes = rk.target_keyframes.clamp(min_slider, max_slider);

                    ui.label(
                        RichText::new(format!(
                            "Keyframes: {} / {}",
                            rk.target_keyframes.min(total),
                            total
                        ))
                        .color(theme.dialog_text),
                    );
                    ui.add_space(6.0);

                    let mut val = rk.target_keyframes;
                    let slider_resp = ui.add_enabled(
                        enabled,
                        egui::Slider::new(&mut val, min_slider..=max_slider).show_value(false),
                    );
                    if slider_resp.changed() && val != rk.target_keyframes {
                        rk.target_keyframes = val;
                        rk.has_moved = true;
                        if let Some(source) = &rk.source_data {
                            let preview = crate::anim_editor::reduction::build_reduced_animation(
                                source,
                                &rk.prioritized_ticks,
                                rk.target_keyframes,
                            );
                            state.animation_data = Some(preview);
                        }
                    }

                    ui.add_space(12.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("OK").color(theme.accent_text))
                                    .fill(theme.accent)
                                    .min_size(Vec2::new(100.0, 25.0)),
                            )
                            .clicked()
                        {
                            if let Some(backup) = rk.animation_backup.take() {
                                state.history.push_waypoint(backup, state.tracks.clone());
                            }
                            if let Some(current) = state.animation_data.clone() {
                                outcome = DialogOutcome::ApplyReduction(current);
                            }
                            state.read_only = false;
                            state.is_dirty = true;
                            close_dialog = true;
                        }
                        if ui
                            .add(egui::Button::new("Cancel").min_size(Vec2::new(100.0, 25.0)))
                            .clicked()
                        {
                            if let Some(backup) = rk.animation_backup.take() {
                                state.animation_data = Some(backup);
                            }
                            state.read_only = false;
                            close_dialog = true;
                        }
                    });
                });
        }

        ActiveDialog::RigErrors { title, issues } => {
            egui::Window::new(title.as_str())
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .fixed_size(Vec2::new(420.0, 220.0))
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(155.0)
                        .show(ui, |ui| {
                            for (idx, issue) in issues.iter().enumerate() {
                                if idx > 0 {
                                    ui.add_space(8.0);
                                }
                                let (icon, col) = match issue.severity {
                                    ValidationSeverity::Error => ("✖", theme.dialog_error_text),
                                    ValidationSeverity::Warning => {
                                        ("⚠", Color32::from_rgb(240, 180, 50))
                                    }
                                };
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(RichText::new(icon).color(col).strong());
                                    ui.label(
                                        RichText::new(&issue.message).color(theme.dialog_text),
                                    );
                                });
                                for detail in &issue.details {
                                    ui.horizontal(|ui| {
                                        ui.add_space(22.0);
                                        ui.label(
                                            RichText::new(format!("- {detail}"))
                                                .size(theme.font_size_dialog_sub)
                                                .color(theme.dialog_sub_text),
                                        );
                                    });
                                }
                            }
                        });
                    ui.add_space(10.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new("OK").min_size(Vec2::new(100.0, 25.0)))
                            .clicked()
                        {
                            close_dialog = true;
                        }
                    });
                });
        }

        ActiveDialog::ImportSettings(imp) => {
            egui::Window::new("Import Animation Settings")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .fixed_size(Vec2::new(480.0, 250.0))
                .show(ctx, |ui| {
                    egui::Grid::new("ace_import_settings_grid")
                        .num_columns(2)
                        .spacing(Vec2::new(12.0, 10.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new("Rig Type:").color(theme.dialog_text));
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut imp.rig_type_index, 0, "R15");
                                ui.radio_value(&mut imp.rig_type_index, 1, "Custom");
                            });
                            ui.end_row();

                            ui.label(RichText::new("Scale Unit:").color(theme.dialog_text));
                            let current_unit = SCALE_UNITS
                                .get(imp.scale_unit_index)
                                .copied()
                                .unwrap_or(SCALE_UNITS[0]);
                            egui::ComboBox::from_id_salt("ace_import_scale_unit")
                                .selected_text(current_unit.name)
                                .show_ui(ui, |ui| {
                                    for (i, unit) in SCALE_UNITS.iter().enumerate() {
                                        ui.selectable_value(
                                            &mut imp.scale_unit_index,
                                            i,
                                            unit.name,
                                        );
                                    }
                                });
                            ui.end_row();

                            ui.label(RichText::new("Scale Factor:").color(theme.dialog_text));
                            ui.add(
                                egui::TextEdit::singleline(&mut imp.scale_factor_text)
                                    .desired_width(120.0),
                            );
                            ui.end_row();

                            if !imp.track_selection_items.is_empty() {
                                ui.label(
                                    RichText::new("Animation Track:").color(theme.dialog_text),
                                );
                                let sel_text = imp
                                    .track_selection_items
                                    .get(imp.selected_track_index)
                                    .cloned()
                                    .unwrap_or_default();
                                egui::ComboBox::from_id_salt("ace_import_track_sel")
                                    .selected_text(sel_text)
                                    .show_ui(ui, |ui| {
                                        for (i, item) in
                                            imp.track_selection_items.iter().enumerate()
                                        {
                                            ui.selectable_value(
                                                &mut imp.selected_track_index,
                                                i,
                                                item,
                                            );
                                        }
                                    });
                                ui.end_row();
                            }
                        });

                    ui.add_space(16.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Import").color(theme.accent_text))
                                    .fill(theme.accent)
                                    .min_size(Vec2::new(100.0, 25.0)),
                            )
                            .clicked()
                        {
                            let unit = SCALE_UNITS
                                .get(imp.scale_unit_index)
                                .copied()
                                .unwrap_or(SCALE_UNITS[0]);
                            let mult = imp
                                .scale_factor_text
                                .trim()
                                .parse::<f64>()
                                .ok()
                                .filter(|&v| v > 0.0)
                                .unwrap_or(1.0);
                            outcome = DialogOutcome::ConfirmFbxImport {
                                path: imp.fbx_path.clone().unwrap_or_default(),
                                scale_factor: (unit.factor * mult) as f32,
                                is_r15: imp.rig_type_index == 0,
                            };
                            close_dialog = true;
                        }
                        if ui
                            .add(egui::Button::new("Cancel").min_size(Vec2::new(100.0, 25.0)))
                            .clicked()
                        {
                            close_dialog = true;
                        }
                    });
                });
        }

        ActiveDialog::Migration { items } => {
            egui::Window::new("Migrate Animation Clips")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .fixed_size(Vec2::new(600.0, 280.0))
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(
                            "The Animation Clip Editor now stores animations in ServerStorage. The following animations have been found attached to your rig and should either be deleted or migrated to the new location.",
                        )
                        .color(theme.dialog_text),
                    );
                    ui.add_space(8.0);

                    // Bulk header selector
                    let all_choice = items.first().and_then(|first| {
                        items
                            .iter()
                            .all(|it| it.choice == first.choice)
                            .then_some(first.choice)
                    });
                    ui.horizontal(|ui| {
                        ui.strong("Animation Clip");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            for (choice, label) in [
                                (MigrationChoice::Ignore, "Ignore"),
                                (MigrationChoice::Migrate, "Migrate"),
                                (MigrationChoice::Delete, "Delete"),
                            ] {
                                if ui.radio(all_choice == Some(choice), label).clicked() {
                                    for it in items.iter_mut() {
                                        it.choice = choice;
                                    }
                                }
                            }
                            ui.label(RichText::new("All:").color(theme.dialog_sub_text));
                        });
                    });
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .max_height(130.0)
                        .show(ui, |ui| {
                            for item in items.iter_mut() {
                                ui.horizontal(|ui| {
                                    ui.label(&item.name);
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.radio_value(
                                            &mut item.choice,
                                            MigrationChoice::Ignore,
                                            "Ignore",
                                        );
                                        ui.radio_value(
                                            &mut item.choice,
                                            MigrationChoice::Migrate,
                                            "Migrate",
                                        );
                                        ui.radio_value(
                                            &mut item.choice,
                                            MigrationChoice::Delete,
                                            "Delete",
                                        );
                                    });
                                });
                            }
                        });

                    ui.add_space(10.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("OK").color(theme.accent_text))
                                    .fill(theme.accent)
                                    .min_size(Vec2::new(100.0, 25.0)),
                            )
                            .clicked()
                        {
                            let choices = items
                                .iter()
                                .map(|it| (it.anim_ref, it.choice))
                                .collect();
                            outcome = DialogOutcome::ApplyMigration(choices);
                            close_dialog = true;
                        }
                        if ui
                            .add(egui::Button::new("Skip").min_size(Vec2::new(100.0, 25.0)))
                            .clicked()
                        {
                            close_dialog = true;
                        }
                    });
                });
        }

        ActiveDialog::ImportFromRoblox {
            asset_id_text,
            error,
        } => {
            show_text_entry_window(
                ctx,
                "Import from Roblox",
                "Enter an Animation Asset ID:",
                "Asset ID",
                asset_id_text,
                error,
                "Import",
                theme,
                &mut |submitted| {
                    if submitted.trim().parse::<u64>().is_ok() {
                        Ok(())
                    } else {
                        Err("Enter a valid numeric Asset ID.".to_string())
                    }
                },
                &mut close_dialog,
            );
        }

        ActiveDialog::InProgress { text, cancellable } => {
            egui::Window::new("Please Wait")
                .title_bar(false)
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .fixed_size(Vec2::new(320.0, 120.0))
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(12.0);
                        ui.spinner();
                        ui.add_space(8.0);
                        ui.label(RichText::new(text.as_str()).color(theme.dialog_text));
                        if *cancellable {
                            ui.add_space(10.0);
                            if ui
                                .add(egui::Button::new("Cancel").min_size(Vec2::new(100.0, 25.0)))
                                .clicked()
                            {
                                close_dialog = true;
                            }
                        }
                    });
                });
        }
    }

    if !close_dialog {
        state.active_dialog = dialog;
    }

    outcome
}

#[allow(clippy::too_many_arguments)]
fn show_text_entry_window(
    ctx: &Context,
    title: &str,
    prompt_text: &str,
    input_label: &str,
    text: &mut String,
    error: &mut Option<String>,
    confirm_label: &str,
    theme: &AceTheme,
    on_submit: &mut dyn FnMut(&str) -> Result<(), String>,
    close_dialog: &mut bool,
) {
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .fixed_size(Vec2::new(380.0, 130.0))
        .show(ctx, |ui| {
            if !prompt_text.trim().is_empty() {
                ui.label(RichText::new(prompt_text).color(theme.dialog_text));
                ui.add_space(6.0);
            }

            let mut submit_now = false;
            ui.horizontal(|ui| {
                if !input_label.is_empty() {
                    ui.label(RichText::new(input_label).color(theme.dialog_text));
                }
                let edit =
                    ui.add(egui::TextEdit::singleline(text).desired_width(ui.available_width()));
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit_now = true;
                }
            });

            if let Some(err) = error.as_ref() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(err)
                        .size(theme.font_size_dialog_sub)
                        .color(theme.dialog_error_text),
                );
            }

            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new(confirm_label).color(theme.accent_text))
                            .fill(theme.accent)
                            .min_size(Vec2::new(100.0, 25.0)),
                    )
                    .clicked()
                {
                    submit_now = true;
                }
                if ui
                    .add(egui::Button::new("Cancel").min_size(Vec2::new(100.0, 25.0)))
                    .clicked()
                {
                    *close_dialog = true;
                }
            });

            if submit_now {
                match on_submit(text) {
                    Ok(()) => {
                        *error = None;
                        *close_dialog = true;
                    }
                    Err(msg) => {
                        *error = Some(msg);
                    }
                }
            }
        });
}

/// Renders `NoticeToast` (3.0s show + 0.5s fade) and `ActionToast` overlays inside `panel_rect`.
pub fn show_toasts(
    ctx: &Context,
    panel_rect: Rect,
    state: &mut EditorState,
    theme: &AceTheme,
    _dt: f32,
) {
    let now = ctx.input(|i| i.time);
    let mut clear_toast = false;

    if let Some(toast) = &mut state.toast.active {
        match toast {
            ToastKind::Notice {
                text,
                started_at_secs,
                show_secs,
                fade_secs,
            } => {
                if *started_at_secs <= 0.0 {
                    *started_at_secs = now;
                }
                let elapsed = (now - *started_at_secs).max(0.0);
                let total = (*show_secs).max(0.0);
                if elapsed >= total {
                    clear_toast = true;
                } else {
                    let fade_start = (total - *fade_secs).max(0.0);
                    let alpha = if elapsed >= fade_start && *fade_secs > 0.0 {
                        (1.0 - ((elapsed - fade_start) / *fade_secs) as f32).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };

                    // Keep repainting smoothly until toast expires.
                    ctx.request_repaint();

                    let bg = theme.background.gamma_multiply(alpha);
                    let border = theme.border.gamma_multiply(alpha);
                    let fg = theme.toast_text.gamma_multiply(alpha);

                    let anchor = Pos2::new(panel_rect.center().x, panel_rect.max.y - 28.0);
                    egui::Area::new(Id::new("ace_notice_toast"))
                        .order(egui::Order::Foreground)
                        .fixed_pos(anchor)
                        .pivot(Align2::CENTER_BOTTOM)
                        .interactable(false)
                        .show(ctx, |ui| {
                            egui::Frame::NONE
                                .fill(bg)
                                .stroke(Stroke::new(1.0, border))
                                .corner_radius(4.0)
                                .inner_margin(egui::Margin::symmetric(20, 8))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(text.as_str())
                                            .size(theme.font_size_dialog)
                                            .color(fg),
                                    );
                                });
                        });
                }
            }
            ToastKind::Action {
                text,
                primary_label,
                secondary_label,
                ..
            } => {
                let anchor = Pos2::new(panel_rect.max.x - 28.0, panel_rect.max.y - 28.0);
                egui::Area::new(Id::new("ace_action_toast"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(anchor)
                    .pivot(Align2::RIGHT_BOTTOM)
                    .show(ctx, |ui| {
                        egui::Frame::NONE
                            .fill(theme.background)
                            .stroke(Stroke::new(1.0, theme.border))
                            .corner_radius(4.0)
                            .inner_margin(egui::Margin::symmetric(20, 15))
                            .show(ui, |ui| {
                                ui.set_width(340.0);
                                ui.label(
                                    RichText::new(text.as_str())
                                        .size(theme.font_size_dialog)
                                        .color(theme.toast_text),
                                );
                                ui.add_space(10.0);
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(primary_label.as_str())
                                                    .color(theme.accent_text),
                                            )
                                            .fill(theme.accent)
                                            .min_size(Vec2::new(100.0, 25.0)),
                                        )
                                        .clicked()
                                    {
                                        clear_toast = true;
                                    }
                                    if ui
                                        .add(
                                            egui::Button::new(secondary_label.as_str())
                                                .min_size(Vec2::new(100.0, 25.0)),
                                        )
                                        .clicked()
                                    {
                                        clear_toast = true;
                                    }
                                });
                            });
                    });
            }
        }
    }

    if clear_toast {
        state.toast.clear();
    }
}

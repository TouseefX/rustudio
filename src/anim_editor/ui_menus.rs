//! 1:1 Menus for `AnimationClipEditor` (`AnimationClipMenu` and `SettingsMenu`).

use egui::Ui;
use rbx_dom_weak::types::Ref;

use crate::anim_editor::constants::{
    DEFAULT_ANIMATION_NAME, EULER_ANGLES_ORDER, FRAMERATES, PLAYBACK_SPEEDS,
};
use crate::anim_editor::model::{AnimationPriority, TrackType};
use crate::anim_editor::state::{
    ActiveDialog, EditorState, GridSpeedSetting, PlayState, SnapMode, TimelineUnit,
};

/// Host-executed commands emitted by `AnimationClipMenu`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuCommand {
    LoadSaved(Ref),
    Save,
    SaveAs(String),
    CreateNew(String),
    ImportFbx,
    ImportRbxm,
    ExportRbxm,
    OpenReduceKeyframes,
    PromoteToCurves,
    SanitizeAnimation,
    SetPriority(AnimationPriority),
    DeleteSaved(Ref),
}

/// Exact implementation of `Components/AnimationClipMenu`.
pub fn show_animation_clip_menu(ui: &mut Ui, state: &mut EditorState) -> Option<MenuCommand> {
    let mut cmd: Option<MenuCommand> = None;

    let has_saves =
        !state.rig_cache.anim_saves.is_empty() || !state.rig_cache.legacy_anim_saves.is_empty();

    // 1. Load submenu
    ui.add_enabled_ui(has_saves, |ui| {
        ui.menu_button("Load", |ui| {
            let saves = state.rig_cache.anim_saves.clone();
            let legacy = state.rig_cache.legacy_anim_saves.clone();
            for entry in saves.into_iter().chain(legacy.into_iter()) {
                let label = if entry.is_legacy_anim_saves {
                    format!("{} *", entry.name)
                } else {
                    entry.name.clone()
                };
                ui.horizontal(|ui| {
                    if ui.button(&label).clicked() {
                        cmd = Some(MenuCommand::LoadSaved(entry.anim_ref));
                        ui.close();
                    }
                    if ui
                        .small_button("×")
                        .on_hover_text("Delete saved animation")
                        .clicked()
                    {
                        cmd = Some(MenuCommand::DeleteSaved(entry.anim_ref));
                        ui.close();
                    }
                });
            }
        });
    });

    // 2. Save
    let current_name = state
        .animation_data
        .as_ref()
        .map(|a| a.metadata.name.clone())
        .unwrap_or_default();
    if ui.button("Save").clicked() {
        if current_name.is_empty() {
            state.active_dialog = ActiveDialog::SaveAs {
                name: DEFAULT_ANIMATION_NAME.to_string(),
                error: None,
            };
        } else {
            cmd = Some(MenuCommand::Save);
        }
        ui.close();
    }

    // 3. Save As...
    ui.menu_button("Save As...", |ui| {
        if ui.button("New...").clicked() {
            state.active_dialog = ActiveDialog::SaveAs {
                name: if current_name.is_empty() {
                    DEFAULT_ANIMATION_NAME.to_string()
                } else {
                    current_name.clone()
                },
                error: None,
            };
            ui.close();
        }
        if has_saves {
            ui.separator();
            let saves = state.rig_cache.anim_saves.clone();
            let legacy = state.rig_cache.legacy_anim_saves.clone();
            for entry in saves.into_iter().chain(legacy.into_iter()) {
                let label = if entry.is_legacy_anim_saves {
                    format!("{} *", entry.name)
                } else {
                    entry.name.clone()
                };
                if ui.button(label).clicked() {
                    state.active_dialog = ActiveDialog::OverwriteConfirm {
                        name: entry.name,
                        is_legacy: entry.is_legacy_anim_saves,
                    };
                    ui.close();
                }
            }
        }
    });

    ui.separator();

    // 4. Import submenu
    ui.menu_button("Import", |ui| {
        if ui.button("From FBX Animation...").clicked() {
            cmd = Some(MenuCommand::ImportFbx);
            ui.close();
        }
        if ui.button("From File (.rbxm/.rbxmx)...").clicked() {
            cmd = Some(MenuCommand::ImportRbxm);
            ui.close();
        }
    });

    // 5. Export submenu
    ui.menu_button("Export", |ui| {
        if ui.button("Export to File (.rbxm)...").clicked() {
            cmd = Some(MenuCommand::ExportRbxm);
            ui.close();
        }
    });

    ui.separator();

    // 6. Create New...
    if ui.button("Create New...").clicked() {
        state.active_dialog = ActiveDialog::CreateNewAnimation {
            name: DEFAULT_ANIMATION_NAME.to_string(),
            error: None,
        };
        ui.close();
    }

    ui.separator();

    // 7. Set Animation Priority submenu (all 7 Enum.AnimationPriority values)
    let current_priority = state.animation_data.as_ref().map(|a| a.metadata.priority);
    ui.menu_button("Set Animation Priority", |ui| {
        for &priority in AnimationPriority::MENU_ORDER {
            let checked = current_priority == Some(priority);
            let label = if checked {
                format!("✔ {}", priority.as_str())
            } else {
                format!("   {}", priority.as_str())
            };
            if ui.button(label).clicked() {
                state.add_waypoint();
                if let Some(anim) = &mut state.animation_data {
                    anim.metadata.priority = priority;
                }
                state.is_dirty = true;
                cmd = Some(MenuCommand::SetPriority(priority));
                ui.close();
            }
        }
    });

    ui.separator();

    // 8. Optimize Keyframes...
    let can_optimize = state
        .animation_data
        .as_ref()
        .is_some_and(|a| a.metadata.end_tick > 0)
        && state.playback.play_state != PlayState::Record;
    if ui
        .add_enabled(can_optimize, egui::Button::new("Optimize Keyframes..."))
        .clicked()
    {
        cmd = Some(MenuCommand::OpenReduceKeyframes);
        ui.close();
    }

    // Promote to Curves (for KeyframeSequence clips)
    if !state.is_channel_animation() && state.animation_data.is_some() {
        if ui.button("Promote to Curves...").clicked() {
            state.active_dialog = ActiveDialog::PromoteToCurves;
            ui.close();
        }
    }

    // 9. Sanitize Animation
    if ui.button("Sanitize Animation").clicked() {
        cmd = Some(MenuCommand::SanitizeAnimation);
        ui.close();
    }

    cmd
}

/// Exact implementation of `Components/SettingsMenu` plus viewport bone/grid toggles.
pub fn show_settings_menu(ui: &mut Ui, state: &mut EditorState) {
    let mut settings_changed = false;

    // 1. Timeline Unit
    ui.menu_button("Timeline Unit", |ui| {
        for &unit in TimelineUnit::ORDER {
            let checked = state.settings.timeline_unit == unit;
            let label = checked_label(checked, unit.label());
            if ui.button(label).clicked() {
                state.settings.timeline_unit = unit;
                settings_changed = true;
                ui.close();
            }
        }
    });

    ui.separator();

    // 2. Frame Rate
    let has_anim = state.animation_data.is_some();
    let fps_title = if has_anim {
        format!("Frame Rate: {} fps", state.frame_rate)
    } else {
        "Frame Rate".to_string()
    };
    ui.add_enabled_ui(has_anim, |ui| {
        ui.menu_button(fps_title, |ui| {
            let mut is_preset = false;
            for &fps in FRAMERATES {
                let checked = state.frame_rate == fps;
                if checked {
                    is_preset = true;
                }
                if ui
                    .button(checked_label(checked, &format!("{fps} fps")))
                    .clicked()
                {
                    state.frame_rate = fps;
                    if let Some(anim) = &mut state.animation_data {
                        anim.metadata.frame_rate = fps;
                    }
                    ui.close();
                }
            }
            if ui.button(checked_label(!is_preset, "Custom...")).clicked() {
                state.active_dialog = ActiveDialog::ChangeFps {
                    text: state.frame_rate.to_string(),
                    error: None,
                };
                ui.close();
            }
        });
    });

    // 3. Playback Speed
    let speed = state.playback.playback_speed;
    let speed_str = format_playback_speed(speed);
    ui.menu_button(format!("Playback Speed: {speed_str}x"), |ui| {
        let mut is_preset = false;
        for &preset in PLAYBACK_SPEEDS {
            let checked = (speed - preset).abs() < 1e-4;
            if checked {
                is_preset = true;
            }
            let preset_label = format!("{}x", format_playback_speed(preset));
            if ui.button(checked_label(checked, &preset_label)).clicked() {
                state.playback.playback_speed = preset;
                state.playback.start_time_secs = None;
                ui.close();
            }
        }
        if ui.button(checked_label(!is_preset, "Custom...")).clicked() {
            state.active_dialog = ActiveDialog::ChangePlaybackSpeed {
                text: speed_str.clone(),
                error: None,
            };
            ui.close();
        }
    });

    // 4. Grid Speed
    let resolved_grid_speed = state
        .settings
        .grid_speed
        .resolve_studs_per_sec(state.rig_cache.humanoid_walkspeed);
    ui.menu_button(
        format!(
            "Grid Speed: {} studs/s",
            format_grid_speed(resolved_grid_speed)
        ),
        |ui| {
            let gs = state.settings.grid_speed;
            if ui
                .button(checked_label(gs == GridSpeedSetting::Static, "Static (0)"))
                .clicked()
            {
                state.settings.grid_speed = GridSpeedSetting::Static;
                settings_changed = true;
                ui.close();
            }
            if ui
                .button(checked_label(
                    gs == GridSpeedSetting::Default,
                    "Default Walkspeed (16)",
                ))
                .clicked()
            {
                state.settings.grid_speed = GridSpeedSetting::Default;
                settings_changed = true;
                ui.close();
            }
            if state.rig_cache.has_humanoid {
                if ui
                    .button(checked_label(
                        gs == GridSpeedSetting::FromRig,
                        "From Humanoid",
                    ))
                    .clicked()
                {
                    state.settings.grid_speed = GridSpeedSetting::FromRig;
                    settings_changed = true;
                    ui.close();
                }
            }
            let is_custom = matches!(gs, GridSpeedSetting::Custom(_));
            if ui.button(checked_label(is_custom, "Custom...")).clicked() {
                state.active_dialog = ActiveDialog::ChangeGridSpeed {
                    text: format_grid_speed(resolved_grid_speed),
                    error: None,
                };
                ui.close();
            }
        },
    );

    ui.separator();

    // 5. Show Animation Events
    if ui
        .checkbox(&mut state.settings.show_events, "Show Animation Events")
        .changed()
    {
        settings_changed = true;
    }

    // 6. Snap Mode
    ui.menu_button("Snap Mode", |ui| {
        for mode in [SnapMode::Keyframes, SnapMode::Frames, SnapMode::None] {
            let checked = state.settings.snap_mode == mode;
            if ui.button(checked_label(checked, mode.label())).clicked() {
                state.settings.snap_mode = mode;
                settings_changed = true;
                ui.close();
            }
        }
    });

    ui.separator();

    // 7. Default Rotation Type
    ui.menu_button("Default Rotation Type", |ui| {
        let is_euler = state.settings.default_rotation_type == TrackType::EulerAngles;
        if ui.button(checked_label(is_euler, "Euler Angles")).clicked() {
            state.settings.default_rotation_type = TrackType::EulerAngles;
            settings_changed = true;
            ui.close();
        }
        let is_quat = state.settings.default_rotation_type == TrackType::Quaternion;
        if ui.button(checked_label(is_quat, "Quaternions")).clicked() {
            state.settings.default_rotation_type = TrackType::Quaternion;
            settings_changed = true;
            ui.close();
        }
    });

    // 8. Default Euler's Angles Order
    ui.menu_button("Default Euler's Angles Order", |ui| {
        for &order in EULER_ANGLES_ORDER {
            let checked = state.settings.default_euler_angles_order == order;
            if ui.button(checked_label(checked, order.as_str())).clicked() {
                state.settings.default_euler_angles_order = order;
                settings_changed = true;
                ui.close();
            }
        }
    });

    ui.separator();

    // 9. Show Bones / 10. Show Floor Grid
    if ui
        .checkbox(&mut state.settings.visualize_bones, "Show Bones")
        .changed()
    {
        settings_changed = true;
    }
    if ui
        .checkbox(&mut state.settings.visualize_grid, "Show Floor Grid")
        .changed()
    {
        settings_changed = true;
    }

    if settings_changed {
        let _ = state.settings.save();
    }
}

fn checked_label(checked: bool, text: &str) -> String {
    if checked {
        format!("✔ {text}")
    } else {
        format!("   {text}")
    }
}

fn format_playback_speed(speed: f32) -> String {
    let s = format!("{speed:.2}");
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
}

fn format_grid_speed(speed: f32) -> String {
    if (speed - speed.round()).abs() < 1e-3 {
        format!("{}", speed.round() as i32)
    } else {
        format!("{speed:.1}")
    }
}

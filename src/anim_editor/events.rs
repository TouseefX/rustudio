//! Animation Events selection, editing thunks, and `EditEventsDialog` modal UI
//! (derived from `Thunks/Events/*`, `Components/EventsController`,
//! and `Components/EditEventsDialog/*`).

use std::collections::{BTreeMap, BTreeSet};

use egui::{Align, Color32, Layout, RichText, ScrollArea, Stroke, TextEdit, Vec2, Window};

use crate::anim_editor::constants::MAX_ANIMATION_LENGTH;
use crate::anim_editor::history::add_waypoint;
use crate::anim_editor::model::{clamp_tick, nearest_frame, Events, Tick};
use crate::anim_editor::state::{
    ActiveDialog, Clipboard, ClipboardType, DragContext, EditEventsDialogState, EditorState,
    HistoryWaypoint, SnapMode,
};
use crate::anim_editor::theme::AceTheme;

// ============================================================================
// Event Selection & Filter Helpers
// ============================================================================

/// Selects the event marker at `tick` (clearing keyframe selection for mutual exclusivity).
pub fn select_event(state: &mut EditorState, tick: Tick, multi_select: bool) {
    state.selected_keyframes.clear();
    let exists = state
        .animation_data
        .as_ref()
        .is_some_and(|anim| anim.events.data.contains_key(&tick));
    if !exists {
        if !multi_select {
            state.selected_events.clear();
        }
        return;
    }
    if !multi_select {
        state.selected_events.clear();
    }
    state.selected_events.insert(tick);
}

/// Deselects the event marker at `tick`.
pub fn deselect_event(state: &mut EditorState, tick: Tick) {
    state.selected_events.remove(&tick);
}

/// Clears all selected events (and keyframes).
pub fn clear_event_selection(state: &mut EditorState) {
    state.selected_keyframes.clear();
    state.selected_events.clear();
}

/// Recomputes `state.available_events` from `animation_data.events`, automatically
/// enabling newly introduced event names in `state.event_filters` and pruning removed ones.
pub fn refresh_available_events(state: &mut EditorState) {
    let Some(anim) = state.animation_data.as_ref() else {
        state.available_events.clear();
        state.event_filters.clear();
        return;
    };

    let new_available = anim.events.all_event_names();
    let old_set: BTreeSet<&str> = state.available_events.iter().map(String::as_str).collect();
    let new_set: BTreeSet<&str> = new_available.iter().map(String::as_str).collect();

    // Auto-enable newly added event names if filters are active.
    if !state.event_filters.is_empty() {
        for name in &new_available {
            if !old_set.contains(name.as_str()) {
                state.event_filters.insert(name.clone());
            }
        }
    }
    state
        .event_filters
        .retain(|name| new_set.contains(name.as_str()));
    state.available_events = new_available;
}

/// Opens the `EditEventsDialog` modal for `tick`, cloning `Events` into a draft.
pub fn open_edit_events_dialog(state: &mut EditorState, tick: Tick) {
    let Some(anim) = state.animation_data.as_ref() else {
        return;
    };
    state.event_editing_tick = Some(tick);
    state.active_dialog = ActiveDialog::EditEvents(EditEventsDialogState {
        tick,
        draft_events: anim.events.clone(),
        editing_event_name: None,
        editing_name_buffer: String::new(),
        rename_all_prompt: None,
        delete_all_prompt: None,
    });
}

// ============================================================================
// Event Dragging, Deletion & Clipboard
// ============================================================================

/// Captures a `DragContext::Events` snapshot before dragging selected event markers.
pub fn begin_event_drag(state: &mut EditorState, pivot_tick: Tick) {
    let Some(anim) = state.animation_data.as_ref() else {
        return;
    };
    if state.selected_events.is_empty() {
        return;
    }
    state.drag_context = Some(DragContext::new_events(
        anim.clone(),
        state.selected_events.clone(),
        state.event_filters.clone(),
        pivot_tick,
    ));
}

/// Moves selected event markers cleanly from the pre-drag `DragContext::Events` snapshot
/// (`Thunks/Events/MoveSelectedEvents`).
pub fn update_event_drag(state: &mut EditorState, target_tick: Tick) {
    let Some(DragContext::Events {
        animation_data: orig_anim,
        selected_events: orig_selected,
        event_filters,
        pivot_tick,
        new_tick,
        has_waypoint,
    }) = state.drag_context.as_mut()
    else {
        return;
    };

    *new_tick = target_tick;
    let delta = target_tick - *pivot_tick;

    // Commit a single undo waypoint on the first actual movement.
    if delta != 0 && !*has_waypoint {
        state.history.past.insert(
            0,
            HistoryWaypoint {
                animation_data: orig_anim.clone(),
                tracks: state.tracks.clone(),
            },
        );
        state.history.past.truncate(state.history.max_waypoint_size);
        state.history.future.clear();
        state.is_dirty = true;
        *has_waypoint = true;
    }

    let Some((earliest, latest)) = Events::event_bounds(orig_selected) else {
        return;
    };

    let mut new_anim = orig_anim.clone();
    let mut new_selected = BTreeSet::new();

    let ticks: Vec<Tick> = orig_selected.iter().copied().collect();
    let iter_ticks: Vec<Tick> = if delta < 0 {
        ticks
    } else {
        ticks.into_iter().rev().collect()
    };

    let snap_mode = state.settings.snap_mode;
    let frame_rate = state.frame_rate;

    for old_tick in iter_ticks {
        let mut dest = old_tick + delta;
        if snap_mode != SnapMode::None {
            dest = nearest_frame(dest, frame_rate);
        }
        let min_allowed = old_tick - earliest;
        let max_allowed = MAX_ANIMATION_LENGTH - (latest - old_tick);
        dest = dest.clamp(min_allowed, max_allowed);

        if old_tick != dest {
            if event_filters.is_empty() {
                new_anim.events.move_events(old_tick, dest);
            } else if let Some(existing) = new_anim.events.data.get(&old_tick).cloned() {
                for (name, value) in existing {
                    if event_filters.contains(&name) {
                        new_anim.events.remove_event(old_tick, &name);
                        new_anim.events.add_event(dest, name, value);
                    }
                }
            }
        }
        new_selected.insert(dest);
    }

    new_anim.set_end_tick();
    state.animation_data = Some(new_anim);
    state.selected_events = new_selected;
}

/// Deletes all selected events (filtered by `state.event_filters` when non-empty).
pub fn delete_selected_events(state: &mut EditorState) {
    if state.selected_events.is_empty() || state.animation_data.is_none() {
        return;
    }
    add_waypoint(state);

    let selected: Vec<Tick> = state.selected_events.iter().copied().collect();
    let filters = state.event_filters.clone();

    if let Some(anim) = state.animation_data.as_mut() {
        for tick in selected {
            if filters.is_empty() {
                anim.events.delete_events(tick);
            } else {
                for name in &filters {
                    anim.events.remove_event(tick, name);
                }
            }
        }
        anim.set_end_tick();
    }

    state.selected_events.clear();
    refresh_available_events(state);
}

/// Copies all selected events into `state.clipboard` (`CopySelectedEvents`).
pub fn copy_selected_events(state: &mut EditorState) {
    let Some(anim) = state.animation_data.as_ref() else {
        return;
    };
    if state.selected_events.is_empty() {
        return;
    }

    let mut copied: BTreeMap<Tick, BTreeMap<String, String>> = BTreeMap::new();
    for &tick in &state.selected_events {
        let Some(event_map) = anim.events.data.get(&tick) else {
            continue;
        };
        if state.event_filters.is_empty() {
            copied.insert(tick, event_map.clone());
        } else {
            let mut filtered = BTreeMap::new();
            for name in &state.event_filters {
                if let Some(val) = event_map.get(name) {
                    filtered.insert(name.clone(), val.clone());
                }
            }
            if !filtered.is_empty() {
                copied.insert(tick, filtered);
            }
        }
    }

    if !copied.is_empty() {
        state.clipboard = Clipboard::Events(copied);
        state.clipboard_type = Some(ClipboardType::Events);
    }
}

/// Pastes copied events starting at `target_tick` (`PasteEvents`).
pub fn paste_events(state: &mut EditorState, target_tick: Tick) {
    let Clipboard::Events(copied) = &state.clipboard else {
        return;
    };
    if copied.is_empty() || state.animation_data.is_none() {
        return;
    }
    let copied = copied.clone();
    let Some(&earliest_tick) = copied.keys().next() else {
        return;
    };

    add_waypoint(state);

    if let Some(anim) = state.animation_data.as_mut() {
        for (orig_tick, event_map) in copied {
            let insert_tick = clamp_tick(target_tick + (orig_tick - earliest_tick));
            anim.events.delete_events(insert_tick);
            for (name, value) in event_map {
                anim.events.add_event(insert_tick, name, value);
            }
        }
        anim.set_end_tick();
    }

    refresh_available_events(state);
}

/// Replaces all events at `tick` with `events`, committing an undo waypoint.
pub fn set_events_at_tick(state: &mut EditorState, tick: Tick, events: BTreeMap<String, String>) {
    if state.animation_data.is_none() {
        return;
    }
    add_waypoint(state);
    if let Some(anim) = state.animation_data.as_mut() {
        anim.set_events_at_tick(tick, events);
        anim.set_end_tick();
    }
    refresh_available_events(state);
}

/// Renames `old_name` to `new_name` at every tick in the animation, committing a single waypoint.
pub fn rename_event_global(state: &mut EditorState, old_name: &str, new_name: &str) {
    if old_name == new_name || new_name.is_empty() || state.animation_data.is_none() {
        return;
    }
    add_waypoint(state);
    if let Some(anim) = state.animation_data.as_mut() {
        rename_event_in_events(&mut anim.events, old_name, new_name);
    }
    refresh_available_events(state);
}

/// Deletes `name` from every tick in the animation, committing a single waypoint.
pub fn delete_event_global(state: &mut EditorState, name: &str) {
    if state.animation_data.is_none() {
        return;
    }
    add_waypoint(state);
    if let Some(anim) = state.animation_data.as_mut() {
        delete_event_in_events(&mut anim.events, name);
        anim.set_end_tick();
    }
    refresh_available_events(state);
}

// ============================================================================
// EditEventsDialog Modal UI (`Components/EditEventsDialog/*`)
// ============================================================================

/// Renders the `EditEventsDialog` modal window when `state.active_dialog` is
/// `ActiveDialog::EditEvents`.
pub fn show_edit_events_dialog(ctx: &egui::Context, state: &mut EditorState, theme: &AceTheme) {
    let ActiveDialog::EditEvents(mut dialog) = std::mem::take(&mut state.active_dialog) else {
        return;
    };

    let mut close_dialog = false;
    let mut save_dialog = false;

    let tick = dialog.tick;
    let unused_events = collect_unused_events(&dialog.draft_events, tick);

    // Sort event names at `tick` alphabetically, with `""` (new unnamed event) always last.
    let mut row_names: Vec<String> = dialog
        .draft_events
        .data
        .get(&tick)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    row_names.sort_by(|a, b| match (a.is_empty(), b.is_empty()) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => a.cmp(b),
    });

    let mut pending_delete_at_tick: Option<String> = None;
    let mut pending_rename_at_tick: Option<(String, String)> = None;
    let mut pending_add_event = false;

    Window::new("Edit Animation Events")
        .id(egui::Id::new("ace_edit_events_dialog"))
        .collapsible(false)
        .resizable(true)
        .default_width(460.0)
        .min_width(420.0)
        .min_height(180.0)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            // Header row ("Event Name" | "Parameter").
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(220.0, 22.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.label(
                            RichText::new("Event Name")
                                .strong()
                                .color(theme.dialog_text)
                                .size(theme.font_size_dialog_sub),
                        );
                    },
                );
                ui.allocate_ui_with_layout(
                    Vec2::new(220.0, 22.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.label(
                            RichText::new("Parameter")
                                .strong()
                                .color(theme.dialog_text)
                                .size(theme.font_size_dialog_sub),
                        );
                    },
                );
            });
            ui.separator();

            // Event rows inside a scrollable area.
            ScrollArea::vertical()
                .max_height(260.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for name in &row_names {
                        let is_editing = dialog
                            .editing_event_name
                            .as_deref()
                            .is_some_and(|n| n == name.as_str());

                        ui.horizontal(|ui| {
                            // Left cell: EventNameEntry (220px).
                            ui.allocate_ui_with_layout(
                                Vec2::new(220.0, 22.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    // Delete button ('x').
                                    if ui
                                        .small_button("×")
                                        .on_hover_text("Delete Event")
                                        .clicked()
                                    {
                                        pending_delete_at_tick = Some(name.clone());
                                    }

                                    if is_editing {
                                        let te_id = ui.make_persistent_id(("ace_ev_name", name));
                                        let resp = ui.add_sized(
                                            Vec2::new(165.0, 20.0),
                                            TextEdit::singleline(&mut dialog.editing_name_buffer)
                                                .id(te_id)
                                                .hint_text("Event Name"),
                                        );
                                        if !resp.has_focus() && !resp.lost_focus() {
                                            resp.request_focus();
                                        }

                                        let enter_pressed = resp.lost_focus()
                                            && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                        let clicked_outside = resp.lost_focus() && !enter_pressed;

                                        if enter_pressed || clicked_outside {
                                            let trimmed =
                                                dialog.editing_name_buffer.trim().to_string();
                                            dialog.editing_event_name = None;
                                            if trimmed.is_empty() {
                                                pending_delete_at_tick = Some(name.clone());
                                            } else if name.is_empty() {
                                                pending_rename_at_tick =
                                                    Some((name.clone(), trimmed));
                                            } else if trimmed != *name {
                                                dialog.rename_all_prompt =
                                                    Some((name.clone(), trimmed));
                                            }
                                        }
                                    } else {
                                        let display = if name.is_empty() {
                                            "(Unnamed)"
                                        } else {
                                            name.as_str()
                                        };
                                        let label_resp = ui
                                            .add_sized(
                                                Vec2::new(165.0, 20.0),
                                                egui::Label::new(
                                                    RichText::new(display)
                                                        .color(theme.dialog_text)
                                                        .size(theme.font_size_track),
                                                )
                                                .truncate()
                                                .sense(egui::Sense::click()),
                                            )
                                            .on_hover_text("Click to rename event");
                                        if label_resp.clicked() {
                                            dialog.editing_event_name = Some(name.clone());
                                            dialog.editing_name_buffer = name.clone();
                                        }
                                    }

                                    // Context menu button ('⋯') matching EditEventMenu.
                                    ui.menu_button("⋯", |ui| {
                                        if ui.button("Edit Event Name").clicked() {
                                            dialog.editing_event_name = Some(name.clone());
                                            dialog.editing_name_buffer = name.clone();
                                            ui.close();
                                        }
                                        if ui.button("Delete All Same Events").clicked() {
                                            dialog.delete_all_prompt = Some(name.clone());
                                            ui.close();
                                        }
                                    });
                                },
                            );

                            // Right cell: Parameter TextBox (220px).
                            ui.allocate_ui_with_layout(
                                Vec2::new(220.0, 22.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    let mut param_val = dialog
                                        .draft_events
                                        .data
                                        .get(&tick)
                                        .and_then(|m| m.get(name))
                                        .cloned()
                                        .unwrap_or_default();
                                    let resp = ui.add_sized(
                                        Vec2::new(210.0, 20.0),
                                        TextEdit::singleline(&mut param_val)
                                            .hint_text("Parameter"),
                                    );
                                    if resp.changed() {
                                        dialog
                                            .draft_events
                                            .set_event_value(tick, name, param_val);
                                    }
                                },
                            );
                        });

                        // Autocomplete suggestions (FilteringTextBox, max 3 items from unused_events).
                        if is_editing {
                            let needle = dialog.editing_name_buffer.to_lowercase();
                            let matches: Vec<&String> = unused_events
                                .iter()
                                .filter(|cand| {
                                    needle.is_empty() || cand.to_lowercase().contains(&needle)
                                })
                                .take(3)
                                .collect();
                            if !matches.is_empty() {
                                ui.indent(("ace_ev_ac", name), |ui| {
                                    egui::Frame::NONE
                                        .fill(theme.track_shaded_bg)
                                        .stroke(Stroke::new(1.0, theme.border))
                                        .show(ui, |ui| {
                                            for cand in matches {
                                                if ui
                                                    .selectable_label(false, cand.as_str())
                                                    .clicked()
                                                {
                                                    dialog.editing_event_name = None;
                                                    if name.is_empty() {
                                                        pending_rename_at_tick =
                                                            Some((name.clone(), cand.clone()));
                                                    } else if *cand != *name {
                                                        dialog.rename_all_prompt =
                                                            Some((name.clone(), cand.clone()));
                                                    }
                                                }
                                            }
                                        });
                                });
                            }
                        }
                    }

                    // Bottom "Add Event" row (`AddEventEntry`).
                    ui.add_space(4.0);
                    if ui
                        .button(
                            RichText::new("+ Add Event")
                                .color(theme.accent)
                                .size(theme.font_size_track),
                        )
                        .clicked()
                    {
                        pending_add_event = true;
                    }
                });

            // Sub-prompt: ChangeThis vs ChangeAll rename confirmation.
            if let Some((old_name, new_name)) = dialog.rename_all_prompt.clone() {
                ui.separator();
                egui::Frame::NONE
                    .fill(theme.track_shaded_bg)
                    .stroke(Stroke::new(1.0, theme.border))
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!(
                                "Rename \"{old_name}\" to \"{new_name}\" at this frame only or across all frames?"
                            ))
                            .color(theme.dialog_text),
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button("Change This").clicked() {
                                rename_event_at_tick(
                                    &mut dialog.draft_events,
                                    tick,
                                    &old_name,
                                    &new_name,
                                );
                                dialog.rename_all_prompt = None;
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Change All").color(Color32::WHITE),
                                    )
                                    .fill(theme.accent),
                                )
                                .clicked()
                            {
                                rename_event_in_events(
                                    &mut dialog.draft_events,
                                    &old_name,
                                    &new_name,
                                );
                                dialog.rename_all_prompt = None;
                            }
                            if ui.button("Cancel").clicked() {
                                dialog.rename_all_prompt = None;
                            }
                        });
                    });
            }

            // Sub-prompt: DeleteAll confirmation.
            if let Some(target_name) = dialog.delete_all_prompt.clone() {
                ui.separator();
                egui::Frame::NONE
                    .fill(theme.track_shaded_bg)
                    .stroke(Stroke::new(1.0, theme.border))
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!(
                                "Delete all occurrences of \"{target_name}\" across the animation?"
                            ))
                            .color(theme.dialog_error_text),
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button("No").clicked() {
                                dialog.delete_all_prompt = None;
                            }
                            if ui
                                .add(
                                    egui::Button::new(RichText::new("Yes").color(Color32::WHITE))
                                        .fill(theme.accent),
                                )
                                .clicked()
                            {
                                delete_event_in_events(&mut dialog.draft_events, &target_name);
                                dialog.delete_all_prompt = None;
                            }
                        });
                    });
            }

            // Footer Save / Cancel buttons.
            ui.separator();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_sized(
                        Vec2::new(100.0, 25.0),
                        egui::Button::new(RichText::new("Save").color(Color32::WHITE))
                            .fill(theme.accent),
                    )
                    .clicked()
                {
                    save_dialog = true;
                    close_dialog = true;
                }
                if ui
                    .add_sized(Vec2::new(100.0, 25.0), egui::Button::new("Cancel"))
                    .clicked()
                {
                    close_dialog = true;
                }
            });
        });

    // Apply deferred row mutations to draft.
    if let Some(name) = pending_delete_at_tick {
        dialog.draft_events.remove_event(tick, &name);
        if dialog.editing_event_name.as_deref() == Some(name.as_str()) {
            dialog.editing_event_name = None;
        }
    }
    if let Some((old_name, new_name)) = pending_rename_at_tick {
        rename_event_at_tick(&mut dialog.draft_events, tick, &old_name, &new_name);
    }
    if pending_add_event {
        dialog.draft_events.add_event(tick, "", "");
        dialog.editing_event_name = Some(String::new());
        dialog.editing_name_buffer.clear();
    }

    if save_dialog {
        // Strip any unfinished empty-named event before committing.
        dialog.draft_events.remove_event(tick, "");
        if state.animation_data.is_some() {
            add_waypoint(state);
            if let Some(anim) = state.animation_data.as_mut() {
                anim.events = dialog.draft_events;
                anim.set_end_tick();
            }
            refresh_available_events(state);
        }
        state.event_editing_tick = None;
        state.active_dialog = ActiveDialog::None;
    } else if close_dialog {
        state.event_editing_tick = None;
        state.active_dialog = ActiveDialog::None;
    } else {
        state.active_dialog = ActiveDialog::EditEvents(dialog);
    }
}

// ============================================================================
// Internal Event Draft Helpers
// ============================================================================

fn collect_unused_events(events: &Events, tick: Tick) -> Vec<String> {
    let used_at_tick: BTreeSet<&str> = events
        .data
        .get(&tick)
        .map(|m| m.keys().map(String::as_str).collect())
        .unwrap_or_default();

    events
        .all_event_names()
        .into_iter()
        .filter(|name| !name.is_empty() && !used_at_tick.contains(name.as_str()))
        .collect()
}

fn rename_event_at_tick(events: &mut Events, tick: Tick, old_name: &str, new_name: &str) {
    if old_name == new_name || new_name.is_empty() {
        return;
    }
    let val = events
        .data
        .get(&tick)
        .and_then(|m| m.get(old_name))
        .cloned()
        .unwrap_or_default();
    events.remove_event(tick, old_name);
    events.add_event(tick, new_name.to_string(), val);
}

fn rename_event_in_events(events: &mut Events, old_name: &str, new_name: &str) {
    if old_name == new_name || new_name.is_empty() {
        return;
    }
    let ticks: Vec<Tick> = events.keyframes.clone();
    for tick in ticks {
        if events
            .data
            .get(&tick)
            .is_some_and(|m| m.contains_key(old_name))
        {
            rename_event_at_tick(events, tick, old_name, new_name);
        }
    }
}

fn delete_event_in_events(events: &mut Events, name: &str) {
    let ticks: Vec<Tick> = events.keyframes.clone();
    for tick in ticks {
        events.remove_event(tick, name);
    }
}

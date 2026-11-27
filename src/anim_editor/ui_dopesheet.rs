//! DopeSheet timeline grid, `EventTrack`, `SummaryTrack`, `KeyframeCluster`,
//! keyframe diamond rendering, marquee `SelectionBox`, drag/pan/zoom, and
//! `TimelineActions` / `EventActions` context menus derived from
//! `Components/DopeSheet*`, `SummaryTrack`, `EventTrack`,
//! `EventsController`, and `KeyframeCluster`.

use std::collections::BTreeSet;

use egui::{Color32, CornerRadius, CursorIcon, Pos2, Rect, Sense, Stroke, Vec2};

use crate::anim_editor::constants::{
    DEFAULT_KEYFRAME_NAME, EASING_DIRECTION_ORDER, EASING_STYLE_ORDER, EVENT_MARKER_BORDER_SIZE,
    EVENT_MARKER_SIZE, KEYFRAME_BORDER_SIZE, KEYFRAME_BORDER_SIZE_SELECTED, KEYFRAME_WIDTH,
    KEY_INTERPOLATION_MODE_ORDER, MAX_VISIBLE_KEYFRAMES, MIN_SPACE_BETWEEN_KEYS,
    SUMMARY_KEYFRAME_WIDTH, SUMMARY_TRACK_HEIGHT, TICK_FREQUENCY, TRACK_HEIGHT, ZOOM_INCREMENT,
};
use crate::anim_editor::model::{
    nearest_frame, nearest_tick, EasingStyle, KeyInterpolationMode, Keyframe, KeyframePatch,
    KeyframeValue, Tick, Track,
};
use crate::anim_editor::playback::{format_time, pause};
use crate::anim_editor::state::{
    has_multiple_selected_ticks, is_selection_empty, ActiveDialog, ClipboardType, DragMode,
    EditorState, SelectionNode, SnapMode,
};
use crate::anim_editor::theme::{AceTheme, KeyframeColors};
use crate::anim_editor::ui_timeline::{compute_frame_intervals, TimelineMetrics};
use crate::anim_editor::ui_tracklist::{collect_visible_rows, VisibleTrackRow};

/// High-level action emitted by [`show_dopesheet`].
#[derive(Debug, Clone, PartialEq)]
pub enum DopeSheetAction {
    None,
    SelectionChanged,
    KeyframesModified,
    EventsModified,
    OpenEditEventsDialog(Tick),
    OpenRenameKeyframeDialog(Tick),
    OpenChangeDurationDialog,
}

/// Renders the DopeSheet area (vertical gridlines, optional `EventTrack`,
/// `SummaryTrack`, per-track keyframe rows or `KeyframeCluster`s, marquee
/// `SelectionBox`, middle-mouse pan, `Ctrl+Wheel` zoom, and context menus).
pub fn show_dopesheet(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    metrics: &TimelineMetrics,
    theme: &AceTheme,
) -> DopeSheetAction {
    let mut action = DopeSheetAction::None;

    let avail = ui.available_size();
    let (sheet_rect, sheet_resp) = ui.allocate_exact_size(
        Vec2::new(avail.x.max(100.0), avail.y.max(TRACK_HEIGHT * 2.0)),
        Sense::click_and_drag(),
    );

    let show_events = state.settings.show_events && state.animation_data.is_some();
    let event_h = if show_events { TRACK_HEIGHT } else { 0.0 };
    let event_rect = Rect::from_min_size(sheet_rect.min, Vec2::new(sheet_rect.width(), event_h));
    let summary_rect = Rect::from_min_size(
        Pos2::new(sheet_rect.min.x, event_rect.max.y),
        Vec2::new(sheet_rect.width(), SUMMARY_TRACK_HEIGHT),
    );
    let tracks_rect = Rect::from_min_max(
        Pos2::new(sheet_rect.min.x, summary_rect.max.y),
        sheet_rect.max,
    );

    let rows = collect_visible_rows(state);
    let start_idx = state.top_track_index.saturating_sub(1);
    let visible_capacity = ((tracks_rect.height() / TRACK_HEIGHT).ceil() as usize + 1).max(1);

    // 1. Middle-mouse horizontal pan & Ctrl+Wheel cursor-anchored zoom
    let visible_rows = ((tracks_rect.height() / TRACK_HEIGHT).floor() as usize).max(1);
    handle_pan_and_zoom(ui, sheet_rect, &sheet_resp, state, rows.len(), visible_rows);

    // 2. Paint row backgrounds & vertical gridlines
    paint_dopesheet_background(
        ui.painter().with_clip_rect(sheet_rect),
        sheet_rect,
        event_rect,
        summary_rect,
        tracks_rect,
        start_idx,
        visible_capacity,
        metrics,
        state.frame_rate,
        theme,
    );

    // 3. EventTrack row
    if show_events {
        let ev_act = draw_event_track_row(ui, event_rect, state, metrics, theme);
        if ev_act != DopeSheetAction::None {
            action = ev_act;
        }
    }

    // 4. Count visible keyframes for KeyframeCluster threshold (> 1000)
    let visible_kf_count =
        count_visible_keyframes(state, &rows, start_idx, visible_capacity, metrics);
    let show_clusters = visible_kf_count > MAX_VISIBLE_KEYFRAMES;

    // 5. SummaryTrack row
    let mut hit_keyframe = false;
    let sum_act = draw_summary_track_row(
        ui,
        summary_rect,
        state,
        metrics,
        theme,
        show_clusters,
        &mut hit_keyframe,
    );
    if sum_act != DopeSheetAction::None {
        action = sum_act;
    }

    // 6. Per-track rows
    for slot in 0..visible_capacity {
        let Some(row) = rows.get(start_idx + slot) else {
            break;
        };
        let y = tracks_rect.min.y + (slot as f32) * TRACK_HEIGHT;
        if y >= tracks_rect.max.y {
            break;
        }
        let row_rect = Rect::from_min_size(
            Pos2::new(tracks_rect.min.x, y),
            Vec2::new(tracks_rect.width(), TRACK_HEIGHT),
        );
        let row_act = draw_dopesheet_track_row(
            ui,
            row_rect,
            row,
            state,
            metrics,
            theme,
            show_clusters,
            &mut hit_keyframe,
        );
        if row_act != DopeSheetAction::None {
            action = row_act;
        }
    }

    // 7. Empty-space marquee SelectionBox
    if !hit_keyframe && !state.read_only {
        let marquee_area = Rect::from_min_max(summary_rect.min, tracks_rect.max);
        if handle_marquee_selection(
            ui,
            marquee_area,
            summary_rect,
            tracks_rect,
            &sheet_resp,
            &rows,
            start_idx,
            state,
            metrics,
            theme,
        ) {
            action = DopeSheetAction::SelectionChanged;
        }
    }

    // 8. Right-click TimelineActions context menu on dopesheet background
    let menu_act = show_timeline_context_menu(&sheet_resp, state, metrics);
    if menu_act != DopeSheetAction::None {
        action = menu_act;
    }

    action
}

fn handle_pan_and_zoom(
    ui: &mut egui::Ui,
    sheet_rect: Rect,
    sheet_resp: &egui::Response,
    state: &mut EditorState,
    total_rows: usize,
    visible_rows: usize,
) {
    // Middle-mouse horizontal pan
    if sheet_resp.dragged_by(egui::PointerButton::Middle) {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        let dx = ui.input(|i| i.pointer.delta().x);
        let zoom = state.curve_editor.view.horizontal_zoom.max(0.01);
        let scroll_delta = (-dx / sheet_rect.width().max(1.0)) * (1.0 / zoom);
        state.curve_editor.view.horizontal_scroll =
            (state.curve_editor.view.horizontal_scroll + scroll_delta).clamp(0.0, 1.0);
    }

    // Ctrl+Wheel cursor-anchored horizontal zoom, plain Wheel vertical scroll
    if sheet_resp.hovered() {
        let (scroll_y, ctrl_held, pointer_pos) = ui.input(|i| {
            (
                i.smooth_scroll_delta.y,
                i.modifiers.ctrl || i.modifiers.command,
                i.pointer.hover_pos(),
            )
        });
        if scroll_y.abs() > 0.5 {
            if ctrl_held {
                let rel_x = pointer_pos
                    .map(|p| {
                        ((p.x - sheet_rect.min.x) / sheet_rect.width().max(1.0)).clamp(0.0, 1.0)
                    })
                    .unwrap_or(0.5);
                let cur_scroll = state.curve_editor.view.horizontal_scroll;
                let cur_zoom = state.curve_editor.view.horizontal_zoom;
                let new_scroll =
                    (cur_scroll + (rel_x - cur_scroll) * (1.0 - cur_zoom)).clamp(0.0, 1.0);
                let delta_zoom = if scroll_y > 0.0 {
                    ZOOM_INCREMENT
                } else {
                    -ZOOM_INCREMENT
                };
                state.curve_editor.view.horizontal_scroll = new_scroll;
                state.curve_editor.view.horizontal_zoom = (cur_zoom + delta_zoom).clamp(0.0, 1.0);
            } else {
                let step = if scroll_y > 0.0 { -1isize } else { 1isize };
                let cur = state.top_track_index.max(1) as isize;
                let max_top = total_rows.saturating_sub(visible_rows).saturating_add(1).max(1) as isize;
                state.top_track_index = (cur + step).clamp(1, max_top) as usize;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_dopesheet_background(
    painter: egui::Painter,
    sheet_rect: Rect,
    event_rect: Rect,
    summary_rect: Rect,
    tracks_rect: Rect,
    start_idx: usize,
    visible_capacity: usize,
    metrics: &TimelineMetrics,
    frame_rate: u32,
    theme: &AceTheme,
) {
    if event_rect.height() > 0.0 {
        painter.rect_filled(event_rect, CornerRadius::ZERO, theme.track_title_bg);
        painter.line_segment(
            [event_rect.left_bottom(), event_rect.right_bottom()],
            Stroke::new(1.0, theme.border),
        );
    }

    painter.rect_filled(summary_rect, CornerRadius::ZERO, theme.track_primary_bg);
    painter.line_segment(
        [summary_rect.left_bottom(), summary_rect.right_bottom()],
        Stroke::new(1.0, theme.border),
    );

    for slot in 0..visible_capacity {
        let y = tracks_rect.min.y + (slot as f32) * TRACK_HEIGHT;
        if y >= tracks_rect.max.y {
            break;
        }
        let row_rect = Rect::from_min_size(
            Pos2::new(tracks_rect.min.x, y),
            Vec2::new(tracks_rect.width(), TRACK_HEIGHT),
        );
        let bg = if (start_idx + slot) % 2 == 0 {
            theme.track_shaded_bg
        } else {
            theme.background
        };
        painter.rect_filled(row_rect, CornerRadius::ZERO, bg);
    }

    // Vertical gridlines aligned to major/minor timeline ticks
    let fps = frame_rate.max(1);
    let freq = TICK_FREQUENCY as f64;
    let (_major_frames, minor_frames) = compute_frame_intervals(
        metrics.start_tick,
        metrics.end_tick,
        metrics.usable_width(),
        fps,
    );
    let start_frame = ((metrics.start_tick as f64) * (fps as f64) / freq).floor() as i32;
    let end_frame = ((metrics.end_tick as f64) * (fps as f64) / freq).ceil() as i32;
    let first_frame = if start_frame > 0 && start_frame % minor_frames != 0 {
        start_frame + (minor_frames - (start_frame % minor_frames))
    } else {
        start_frame
    };

    let mut frame = first_frame;
    while frame <= end_frame {
        let tick = nearest_tick((frame as f64) * freq / (fps as f64));
        let x = metrics.tick_to_x(tick);
        let is_whole_second = frame.rem_euclid(fps as i32) == 0;
        let color = if is_whole_second {
            theme.timeline_grid_line_bright
        } else {
            theme.timeline_grid_line
        };
        painter.line_segment(
            [
                Pos2::new(x, sheet_rect.min.y),
                Pos2::new(x, sheet_rect.max.y),
            ],
            Stroke::new(1.0, color),
        );
        frame += minor_frames;
    }
}

fn draw_event_track_row(
    ui: &mut egui::Ui,
    rect: Rect,
    state: &mut EditorState,
    metrics: &TimelineMetrics,
    theme: &AceTheme,
) -> DopeSheetAction {
    let mut action = DopeSheetAction::None;

    // Left-gutter event filter button
    let filter_rect = Rect::from_center_size(
        Pos2::new(
            rect.min.x + (metrics.left_pad * 0.5).clamp(10.0, 20.0),
            rect.center().y,
        ),
        Vec2::splat(16.0),
    );
    let filter_resp = ui
        .interact(
            filter_rect,
            ui.id().with("event_filter_btn"),
            Sense::click(),
        )
        .on_hover_text("Filter Animation Events");
    if filter_resp.hovered() {
        ui.painter()
            .rect_filled(filter_rect, CornerRadius::same(2), theme.scrollbar_hover);
    }
    paint_filter_icon(ui.painter(), filter_rect.center(), theme.text);

    let filter_popup_id = ui.id().with("event_filter_popup");
    if filter_resp.clicked() {
        egui::Popup::toggle_id(ui.ctx(), filter_popup_id);
    }
    egui::Popup::from_toggle_button_response(&filter_resp)
        .id(filter_popup_id)
        .show(|ui| {
            let all_names = state
                .animation_data
                .as_ref()
                .map(|d| d.events.all_event_names())
                .unwrap_or_default();
            for name in &all_names {
                let mut enabled =
                    state.event_filters.is_empty() || state.event_filters.contains(name);
                if ui.checkbox(&mut enabled, name).changed() {
                    if state.event_filters.is_empty() {
                        state.event_filters = all_names.iter().cloned().collect();
                    }
                    if enabled {
                        state.event_filters.insert(name.clone());
                    } else {
                        state.event_filters.remove(name);
                    }
                }
            }
            ui.separator();
            if ui.button("Everything").clicked() {
                state.event_filters = all_names.into_iter().collect();
                ui.close();
            }
            if ui.button("Nothing").clicked() {
                state.event_filters.clear();
                ui.close();
            }
        });

    // Collect visible event markers
    let visible_events: Vec<(Tick, Vec<String>)> = state
        .animation_data
        .as_ref()
        .map(|anim| {
            anim.events
                .keyframes
                .iter()
                .copied()
                .filter(|&t| t >= metrics.start_tick && t <= metrics.end_tick)
                .filter_map(|t| {
                    let map = anim.events.data.get(&t)?;
                    let names: Vec<String> = map
                        .keys()
                        .filter(|k| {
                            state.event_filters.is_empty() || state.event_filters.contains(*k)
                        })
                        .cloned()
                        .collect();
                    if names.is_empty() {
                        None
                    } else {
                        Some((t, names))
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let multi = ui.input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.shift);
    let mut hit_marker = false;

    for (tick, names) in &visible_events {
        let x = metrics.tick_to_x(*tick);
        let selected = state.selected_events.contains(tick);
        let marker_rect = Rect::from_center_size(
            Pos2::new(x, rect.center().y),
            Vec2::new(EVENT_MARKER_SIZE.0, EVENT_MARKER_SIZE.1),
        );
        let border_rect = Rect::from_center_size(
            Pos2::new(x, rect.center().y),
            Vec2::new(EVENT_MARKER_BORDER_SIZE.0, EVENT_MARKER_BORDER_SIZE.1),
        );

        let painter = ui.painter().with_clip_rect(rect);
        if selected {
            painter.rect_stroke(
                border_rect,
                CornerRadius::same(1),
                Stroke::new(2.0, theme.event_marker_selected_border),
                egui::StrokeKind::Middle,
            );
        }
        painter.rect_filled(marker_rect, CornerRadius::same(1), theme.accent);
        painter.rect_stroke(
            marker_rect,
            CornerRadius::same(1),
            Stroke::new(1.0, theme.event_marker_border),
            egui::StrokeKind::Middle,
        );

        let hit_rect = border_rect.expand(2.0);
        let resp = ui
            .interact(
                hit_rect,
                ui.id().with(("event_marker", *tick)),
                Sense::click_and_drag(),
            )
            .on_hover_text(names.join(", "));

        if resp.double_clicked() {
            hit_marker = true;
            crate::anim_editor::events::select_event(state, *tick, false);
            state.event_editing_tick = Some(*tick);
            action = DopeSheetAction::OpenEditEventsDialog(*tick);
        } else if resp.clicked() {
            hit_marker = true;
            if selected && multi {
                crate::anim_editor::events::deselect_event(state, *tick);
            } else if !selected {
                crate::anim_editor::events::select_event(state, *tick, multi);
            }
            action = DopeSheetAction::SelectionChanged;
        }

        if resp.drag_started() && !state.read_only {
            hit_marker = true;
            if !selected {
                crate::anim_editor::events::select_event(state, *tick, multi);
            }
            crate::anim_editor::events::begin_event_drag(state, *tick);
        }
        if resp.dragged() && !state.read_only {
            hit_marker = true;
            if let Some(pos) = resp.interact_pointer_pos() {
                let raw = metrics.x_to_tick(pos.x);
                let snapped = snap_drag_tick(state, raw);
                crate::anim_editor::events::update_event_drag(state, snapped);
                action = DopeSheetAction::EventsModified;
            }
        }
        if resp.drag_stopped() {
            crate::anim_editor::ops::end_drag(state, true);
        }

        resp.context_menu(|ui| {
            pause(state);
            if !selected {
                crate::anim_editor::events::select_event(state, *tick, false);
            }
            let act = show_event_context_menu_items(ui, state, *tick, true);
            if act != DopeSheetAction::None {
                action = act;
            }
        });
    }

    // Empty event track interaction (double-click to add/edit, right-click menu, click to deselect)
    let track_resp = ui.interact(rect, ui.id().with("event_track_bg"), Sense::click());
    if !hit_marker {
        if track_resp.double_clicked() {
            if let Some(pos) = track_resp.interact_pointer_pos() {
                let tick = snap_drag_tick(state, metrics.x_to_tick(pos.x));
                crate::anim_editor::events::open_edit_events_dialog(state, tick);
                action = DopeSheetAction::OpenEditEventsDialog(tick);
            }
        } else if track_resp.clicked() {
            crate::anim_editor::events::clear_event_selection(state);
            action = DopeSheetAction::SelectionChanged;
        }

        track_resp.context_menu(|ui| {
            pause(state);
            let tick = ui
                .input(|i| i.pointer.interact_pos())
                .map(|p| snap_drag_tick(state, metrics.x_to_tick(p.x)))
                .unwrap_or(state.playback.playhead);
            let act = show_event_context_menu_items(ui, state, tick, false);
            if act != DopeSheetAction::None {
                action = act;
            }
        });
    }

    action
}

fn show_event_context_menu_items(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    tick: Tick,
    on_event: bool,
) -> DopeSheetAction {
    let mut action = DopeSheetAction::None;
    let has_sel = !state.selected_events.is_empty();
    let can_paste =
        !state.clipboard.is_empty() && state.clipboard_type == Some(ClipboardType::Events);

    let edit_label = if on_event {
        "Edit Events..."
    } else {
        "Add Event..."
    };
    if ui.button(edit_label).clicked() {
        state.event_editing_tick = Some(tick);
        action = DopeSheetAction::OpenEditEventsDialog(tick);
        ui.close();
    }
    if ui
        .add_enabled(has_sel, egui::Button::new("Delete"))
        .clicked()
    {
        crate::anim_editor::events::delete_selected_events(state);
        action = DopeSheetAction::EventsModified;
        ui.close();
    }
    ui.separator();
    if ui.add_enabled(has_sel, egui::Button::new("Cut")).clicked() {
        crate::anim_editor::events::copy_selected_events(state);
        crate::anim_editor::events::delete_selected_events(state);
        action = DopeSheetAction::EventsModified;
        ui.close();
    }
    if ui.add_enabled(has_sel, egui::Button::new("Copy")).clicked() {
        crate::anim_editor::events::copy_selected_events(state);
        ui.close();
    }
    if ui
        .add_enabled(can_paste, egui::Button::new("Paste"))
        .clicked()
    {
        crate::anim_editor::events::paste_events(state, tick);
        action = DopeSheetAction::EventsModified;
        ui.close();
    }

    action
}

fn draw_summary_track_row(
    ui: &mut egui::Ui,
    rect: Rect,
    state: &mut EditorState,
    metrics: &TimelineMetrics,
    theme: &AceTheme,
    show_clusters: bool,
    hit_keyframe: &mut bool,
) -> DopeSheetAction {
    let mut action = DopeSheetAction::None;
    let Some(anim) = state.animation_data.as_ref() else {
        return action;
    };
    let summary_ticks = anim.get_summary_ticks_in_range(metrics.start_tick, metrics.end_tick);
    if summary_ticks.is_empty() {
        return action;
    }

    let painter = ui.painter().with_clip_rect(rect);
    if show_clusters {
        let min_x = metrics.tick_to_x(*summary_ticks.first().unwrap());
        let max_x = metrics.tick_to_x(*summary_ticks.last().unwrap());
        paint_keyframe_cluster(
            &painter,
            min_x,
            max_x,
            rect.center().y,
            theme.keyframe_primary_cluster,
        );
        return action;
    }

    let multi = ui.input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.shift);
    let named_map = anim.events.named_keyframes.clone();
    let fps = state.frame_rate.max(1);
    let unit = state.settings.timeline_unit;

    for tick in summary_ticks {
        let x = metrics.tick_to_x(tick);
        let center = Pos2::new(x, rect.center().y);
        let is_selected = is_summary_tick_selected(state, tick);
        let custom_name = named_map.get(&tick);
        let is_named = custom_name.is_some();

        paint_keyframe_shape(
            &painter,
            center,
            SUMMARY_KEYFRAME_WIDTH,
            theme.keyframe_primary,
            theme.background,
            is_selected,
            true,
            is_named,
        );

        let hit_rect = Rect::from_center_size(center, Vec2::splat(SUMMARY_KEYFRAME_WIDTH + 8.0));
        let tooltip = if let Some(name) = custom_name {
            format!("{name} ({})", format_time(tick, fps, unit))
        } else {
            format!("Summary Keyframe ({})", format_time(tick, fps, unit))
        };
        let resp = ui
            .interact(
                hit_rect,
                ui.id().with(("sum_kf", tick)),
                Sense::click_and_drag(),
            )
            .on_hover_text(tooltip);

        if resp.clicked() {
            *hit_keyframe = true;
            if is_selected && multi {
                crate::anim_editor::selection::deselect_keyframes_at_tick(state, tick);
            } else if !is_selected {
                crate::anim_editor::selection::select_keyframes_at_tick(state, tick, multi);
            }
            action = DopeSheetAction::SelectionChanged;
        }

        if resp.drag_started() && !state.read_only {
            *hit_keyframe = true;
            if !is_selected {
                crate::anim_editor::selection::select_keyframes_at_tick(state, tick, multi);
            }
            crate::anim_editor::ops::begin_keyframe_drag(state, tick, None, DragMode::Keyframe);
        }
        if resp.dragged() && !state.read_only {
            *hit_keyframe = true;
            if let Some(pos) = resp.interact_pointer_pos() {
                let snapped = snap_drag_tick(state, metrics.x_to_tick(pos.x));
                crate::anim_editor::ops::update_keyframe_drag(state, snapped, None);
                action = DopeSheetAction::KeyframesModified;
            }
        }
        if resp.drag_stopped() {
            crate::anim_editor::ops::end_drag(state, true);
        }

        if resp.secondary_clicked() {
            *hit_keyframe = true;
            state.right_click_context.tick = Some(tick);
            state.right_click_context.on_keyframe = true;
            state.right_click_context.summary_keyframe = Some(tick);
            if is_selection_empty(&state.selected_keyframes) {
                crate::anim_editor::selection::select_keyframes_at_tick(state, tick, false);
            }
        }
        let m_act = show_timeline_context_menu(&resp, state, metrics);
        if m_act != DopeSheetAction::None {
            action = m_act;
        }
    }

    action
}

#[allow(clippy::too_many_arguments)]
fn draw_dopesheet_track_row(
    ui: &mut egui::Ui,
    rect: Rect,
    row: &VisibleTrackRow,
    state: &mut EditorState,
    metrics: &TimelineMetrics,
    theme: &AceTheme,
    show_clusters: bool,
    hit_keyframe: &mut bool,
) -> DopeSheetAction {
    let mut action = DopeSheetAction::None;

    // Gather visible ticks and aggregated component metadata matching Studio's
    // `TrackUtils.getComponentsInfo` + `DopeSheetTrack`.
    let (is_channel, visible_keys): (bool, Vec<(Tick, Option<Keyframe>, TrackTickInfo)>) = {
        let Some(anim) = state.animation_data.as_ref() else {
            return action;
        };
        let Some(track) = anim.get_track(&row.instance, &row.path) else {
            return action;
        };
        let is_channel = anim.is_channel_animation();
        let info_map = collect_track_components_info(track, metrics.start_tick, metrics.end_tick);
        let keys = info_map
            .into_iter()
            .map(|(t, info)| {
                let kf = track.data.get(&t).copied();
                (t, kf, info)
            })
            .collect();
        (is_channel, keys)
    };

    if visible_keys.is_empty() {
        return action;
    }

    let painter = ui.painter().with_clip_rect(rect);
    if show_clusters {
        let min_x = metrics.tick_to_x(visible_keys.first().unwrap().0);
        let max_x = metrics.tick_to_x(visible_keys.last().unwrap().0);
        paint_keyframe_cluster(
            &painter,
            min_x,
            max_x,
            rect.center().y,
            theme.keyframe_cluster,
        );
        return action;
    }

    let multi = ui.input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.shift);
    let fps = state.frame_rate.max(1);
    let unit = state.settings.timeline_unit;

    for (tick, kf_opt, info) in visible_keys {
        let x = metrics.tick_to_x(tick);
        let center = Pos2::new(x, rect.center().y);
        let is_selected = is_track_tick_selected(state, &row.instance, &row.path, tick);
        let filled = !is_channel || info.complete;
        let (easing, interp) = if info.complete {
            if is_channel {
                (None, info.interpolation_mode)
            } else {
                (info.easing_style, None)
            }
        } else {
            (None, None)
        };
        let colors = theme.keyframe_colors(easing, interp, false, row.has_error);

        paint_keyframe_shape(
            &painter,
            center,
            KEYFRAME_WIDTH,
            colors,
            theme.background,
            is_selected,
            filled,
            false,
        );

        let hit_rect = Rect::from_center_size(center, Vec2::splat(KEYFRAME_WIDTH + 8.0));
        let tooltip = format_keyframe_tooltip(tick, fps, unit, kf_opt, easing, interp);
        let resp = ui
            .interact(
                hit_rect,
                ui.id().with(("kf", &row.instance, &row.path, tick)),
                Sense::click_and_drag(),
            )
            .on_hover_text(tooltip);

        if resp.clicked() {
            *hit_keyframe = true;
            if is_selected && multi {
                crate::anim_editor::selection::deselect_keyframe(
                    state,
                    &row.instance,
                    &row.path,
                    tick,
                );
            } else if !is_selected {
                crate::anim_editor::selection::select_keyframe_range(
                    state,
                    &row.instance,
                    &row.path,
                    tick,
                    tick,
                    multi,
                );
            }
            action = DopeSheetAction::SelectionChanged;
        }

        if resp.drag_started() && !state.read_only {
            *hit_keyframe = true;
            if !is_selected {
                crate::anim_editor::selection::select_keyframe_range(
                    state,
                    &row.instance,
                    &row.path,
                    tick,
                    tick,
                    multi,
                );
            }
            crate::anim_editor::ops::begin_keyframe_drag(state, tick, None, DragMode::Keyframe);
        }
        if resp.dragged() && !state.read_only {
            *hit_keyframe = true;
            if let Some(pos) = resp.interact_pointer_pos() {
                let snapped = snap_drag_tick(state, metrics.x_to_tick(pos.x));
                crate::anim_editor::ops::update_keyframe_drag(state, snapped, None);
                action = DopeSheetAction::KeyframesModified;
            }
        }
        if resp.drag_stopped() {
            crate::anim_editor::ops::end_drag(state, true);
        }

        if resp.secondary_clicked() {
            *hit_keyframe = true;
            state.right_click_context.tick = Some(tick);
            state.right_click_context.on_keyframe = true;
            state.right_click_context.instance_name = Some(row.instance.clone());
            state.right_click_context.path = Some(row.path.clone());
            if is_selection_empty(&state.selected_keyframes) {
                crate::anim_editor::selection::select_keyframe_range(
                    state,
                    &row.instance,
                    &row.path,
                    tick,
                    tick,
                    false,
                );
            }
        }
        let m_act = show_timeline_context_menu(&resp, state, metrics);
        if m_act != DopeSheetAction::None {
            action = m_act;
        }
    }

    action
}

#[allow(clippy::too_many_arguments)]
fn handle_marquee_selection(
    ui: &mut egui::Ui,
    marquee_area: Rect,
    summary_rect: Rect,
    tracks_rect: Rect,
    sheet_resp: &egui::Response,
    rows: &[VisibleTrackRow],
    start_idx: usize,
    state: &mut EditorState,
    metrics: &TimelineMetrics,
    theme: &AceTheme,
) -> bool {
    let mut changed = false;
    let multi = ui.input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.shift);

    if sheet_resp.drag_started_by(egui::PointerButton::Primary) {
        if let Some(pos) = sheet_resp.interact_pointer_pos() {
            if marquee_area.contains(pos) {
                if !multi {
                    crate::anim_editor::selection::clear_selection(state);
                }
                state.curve_editor.is_dragging_marquee = true;
                state.curve_editor.marquee_start = Some((pos.x, pos.y));
                state.curve_editor.marquee_end = Some((pos.x, pos.y));
                changed = true;
            }
        }
    }

    if state.curve_editor.is_dragging_marquee {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            state.curve_editor.marquee_end = Some((pos.x, pos.y));
        }

        if let (Some((sx, sy)), Some((ex, ey))) = (
            state.curve_editor.marquee_start,
            state.curve_editor.marquee_end,
        ) {
            let box_rect =
                Rect::from_two_pos(Pos2::new(sx, sy), Pos2::new(ex, ey)).intersect(marquee_area);

            let painter = ui.painter().with_clip_rect(marquee_area);
            painter.rect_filled(box_rect, CornerRadius::ZERO, theme.selection_box_fill);
            painter.rect_stroke(
                box_rect,
                CornerRadius::ZERO,
                Stroke::new(1.0, theme.selection_box_border),
                egui::StrokeKind::Middle,
            );

            let min_tick = metrics.x_to_tick(box_rect.min.x);
            let max_tick = metrics.x_to_tick(box_rect.max.x);

            // Clear and re-select so shrinking the box deselects excluded keyframes
            if !multi {
                crate::anim_editor::selection::clear_selection(state);
            }

            // If box covers summary row (min_row == 0), select summary ticks
            if box_rect.min.y <= summary_rect.max.y && box_rect.max.y >= summary_rect.min.y {
                let sum_ticks = state
                    .animation_data
                    .as_ref()
                    .map(|d| d.get_summary_ticks_in_range(min_tick, max_tick))
                    .unwrap_or_default();
                for t in sum_ticks {
                    crate::anim_editor::selection::select_keyframes_at_tick(state, t, true);
                }
            }

            // Select keyframes on each track row overlapping box_rect
            for (slot, row) in rows.iter().skip(start_idx).enumerate() {
                let y0 = tracks_rect.min.y + (slot as f32) * TRACK_HEIGHT;
                let y1 = y0 + TRACK_HEIGHT;
                if y0 > box_rect.max.y {
                    break;
                }
                if y1 >= box_rect.min.y {
                    crate::anim_editor::selection::select_keyframe_range(
                        state,
                        &row.instance,
                        &row.path,
                        min_tick,
                        max_tick,
                        true,
                    );
                }
            }
            changed = true;
        }

        if sheet_resp.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
            state.curve_editor.is_dragging_marquee = false;
            state.curve_editor.marquee_start = None;
            state.curve_editor.marquee_end = None;
        }
    } else if sheet_resp.clicked_by(egui::PointerButton::Primary) {
        if let Some(pos) = sheet_resp.interact_pointer_pos() {
            if marquee_area.contains(pos) && !multi {
                crate::anim_editor::selection::clear_selection(state);
                changed = true;
            }
        }
    }

    changed
}

fn show_timeline_context_menu(
    resp: &egui::Response,
    state: &mut EditorState,
    metrics: &TimelineMetrics,
) -> DopeSheetAction {
    let mut action = DopeSheetAction::None;

    resp.context_menu(|ui| {
        pause(state);
        let has_sel = !is_selection_empty(&state.selected_keyframes);
        let multi_ticks = has_multiple_selected_ticks(&state.selected_keyframes);
        let is_channel = state.is_channel_animation();
        let can_paste = !state.clipboard.is_empty()
            && matches!(
                state.clipboard_type,
                Some(ClipboardType::Keyframes | ClipboardType::Channels)
            );
        let ctx_tick = state
            .right_click_context
            .tick
            .or_else(|| {
                ui.input(|i| i.pointer.interact_pos())
                    .map(|p| snap_drag_tick(state, metrics.x_to_tick(p.x)))
            })
            .unwrap_or(state.playback.playhead);

        if ui.button("Add Keyframe").clicked() {
            if let (Some(inst), Some(path)) = (
                state.right_click_context.instance_name.clone(),
                state.right_click_context.path.clone(),
            ) {
                let track_type = state
                    .animation_data
                    .as_ref()
                    .and_then(|d| d.get_track(&inst, &path))
                    .map(|t| t.track_type)
                    .unwrap_or(crate::anim_editor::model::TrackType::CFrame);
                let val = KeyframeValue::default_for(track_type);
                crate::anim_editor::ops::value_changed(
                    state, &inst, &path, track_type, ctx_tick, val, true,
                );
                action = DopeSheetAction::KeyframesModified;
            }
            ui.close();
        }

        ui.separator();

        if ui.add_enabled(has_sel, egui::Button::new("Cut")).clicked() {
            crate::anim_editor::ops::copy_selected_keyframes(state);
            crate::anim_editor::ops::delete_selected_keyframes(state);
            action = DopeSheetAction::KeyframesModified;
            ui.close();
        }
        if ui.add_enabled(has_sel, egui::Button::new("Copy")).clicked() {
            crate::anim_editor::ops::copy_selected_keyframes(state);
            ui.close();
        }
        if ui
            .add_enabled(can_paste, egui::Button::new("Paste"))
            .clicked()
        {
            crate::anim_editor::ops::paste_keyframes(state, ctx_tick);
            action = DopeSheetAction::KeyframesModified;
            ui.close();
        }
        if ui
            .add_enabled(has_sel, egui::Button::new("Delete"))
            .clicked()
        {
            crate::anim_editor::ops::delete_selected_keyframes(state);
            action = DopeSheetAction::KeyframesModified;
            ui.close();
        }
        if ui
            .add_enabled(has_sel, egui::Button::new("Reset Selected"))
            .clicked()
        {
            crate::anim_editor::ops::reset_selected_keyframes(state);
            action = DopeSheetAction::KeyframesModified;
            ui.close();
        }

        ui.separator();

        if is_channel {
            ui.add_enabled_ui(has_sel, |ui| {
                ui.menu_button("Interpolation Mode", |ui| {
                    for &mode in KEY_INTERPOLATION_MODE_ORDER {
                        if ui.button(mode.as_str()).clicked() {
                            let patch = KeyframePatch {
                                interpolation_mode: Some(Some(mode)),
                                ..Default::default()
                            };
                            crate::anim_editor::ops::set_selected_keyframe_data(state, patch);
                            action = DopeSheetAction::KeyframesModified;
                            ui.close();
                        }
                    }
                });
            });
        } else {
            ui.add_enabled_ui(has_sel, |ui| {
                ui.menu_button("Easing Style", |ui| {
                    for &style in EASING_STYLE_ORDER {
                        if ui.button(style.display_text()).clicked() {
                            let patch = KeyframePatch {
                                easing_style: Some(Some(style)),
                                ..Default::default()
                            };
                            crate::anim_editor::ops::set_selected_keyframe_data(state, patch);
                            action = DopeSheetAction::KeyframesModified;
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Easing Direction", |ui| {
                    for &dir in EASING_DIRECTION_ORDER {
                        if ui.button(dir.as_str()).clicked() {
                            let patch = KeyframePatch {
                                easing_direction: Some(Some(dir)),
                                ..Default::default()
                            };
                            crate::anim_editor::ops::set_selected_keyframe_data(state, patch);
                            action = DopeSheetAction::KeyframesModified;
                            ui.close();
                        }
                    }
                });
            });
        }

        ui.separator();

        if ui.button("Rename Keyframe...").clicked() {
            let existing = state
                .animation_data
                .as_ref()
                .and_then(|d| d.events.named_keyframes.get(&ctx_tick))
                .cloned()
                .unwrap_or_else(|| DEFAULT_KEYFRAME_NAME.to_string());
            state.active_dialog = ActiveDialog::RenameKeyframe {
                tick: ctx_tick,
                name: existing,
            };
            action = DopeSheetAction::OpenRenameKeyframeDialog(ctx_tick);
            ui.close();
        }
        if ui
            .add_enabled(multi_ticks, egui::Button::new("Change Duration..."))
            .clicked()
        {
            action = DopeSheetAction::OpenChangeDurationDialog;
            ui.close();
        }
        if ui.button("Select All").clicked() {
            crate::anim_editor::selection::select_all_keyframes(state);
            action = DopeSheetAction::SelectionChanged;
            ui.close();
        }
    });

    action
}

/// Paints a `KeyframeCluster` bar with 4x4 rotated diamond end caps
/// (`Components/KeyframeCluster`).
fn paint_keyframe_cluster(
    painter: &egui::Painter,
    min_x: f32,
    max_x: f32,
    center_y: f32,
    color: Color32,
) {
    let h = MIN_SPACE_BETWEEN_KEYS;
    let bar = Rect::from_min_max(
        Pos2::new(min_x, center_y - h * 0.5),
        Pos2::new(max_x.max(min_x + 2.0), center_y + h * 0.5),
    );
    painter.rect_filled(bar, CornerRadius::ZERO, color);

    let r = (h - 4.0) * 0.5;
    for ex in [bar.min.x, bar.max.x] {
        let pts = vec![
            Pos2::new(ex, center_y - r),
            Pos2::new(ex + r, center_y),
            Pos2::new(ex, center_y + r),
            Pos2::new(ex - r, center_y),
        ];
        painter.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
    }
}

/// Paints a keyframe marker shaped by easing style / named state.
#[allow(clippy::too_many_arguments)]
/// Paints a keyframe marker matching Studio's `Components/Timeline/Keyframe`:
/// - Named keyframes on the Summary row are upright squares (`Rotation = 0`)
/// - All easing / interpolation keyframes are 45-degree diamonds (`Rotation = 45`),
///   filled with the easing/interpolation color when `filled`, or opaque
///   `empty_fill` (`theme.background`) when partially keyed.
#[allow(clippy::too_many_arguments)]
fn paint_keyframe_shape(
    painter: &egui::Painter,
    center: Pos2,
    size: f32,
    colors: KeyframeColors,
    empty_fill: Color32,
    selected: bool,
    filled: bool,
    is_named: bool,
) {
    let half = size * 0.5;
    let fill = if filled {
        colors.fill(selected)
    } else {
        empty_fill
    };
    let stroke_w = if selected {
        KEYFRAME_BORDER_SIZE_SELECTED
    } else {
        KEYFRAME_BORDER_SIZE
    };
    let stroke = Stroke::new(stroke_w, colors.stroke(selected));

    if is_named {
        // Upright square for named Summary keyframes (`Rotation = 0`), sized to match
        // the visual weight of the 45-degree diamond (`size / sqrt(2)`).
        let r = Rect::from_center_size(center, Vec2::splat(size * std::f32::consts::FRAC_1_SQRT_2));
        painter.rect(
            r,
            CornerRadius::ZERO,
            fill,
            stroke,
            egui::StrokeKind::Middle,
        );
    } else {
        // 45-degree diamond (`Rotation = 45`)
        let pts = vec![
            Pos2::new(center.x, center.y - half),
            Pos2::new(center.x + half, center.y),
            Pos2::new(center.x, center.y + half),
            Pos2::new(center.x - half, center.y),
        ];
        painter.add(egui::Shape::convex_polygon(pts, fill, stroke));
    }
}

fn paint_filter_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let pts = vec![
        Pos2::new(c.x - 4.5, c.y - 3.5),
        Pos2::new(c.x + 4.5, c.y - 3.5),
        Pos2::new(c.x + 1.2, c.y + 0.5),
        Pos2::new(c.x + 1.2, c.y + 4.0),
        Pos2::new(c.x - 1.2, c.y + 4.0),
        Pos2::new(c.x - 1.2, c.y + 0.5),
    ];
    painter.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
}

fn snap_drag_tick(state: &EditorState, raw_tick: Tick) -> Tick {
    if state.settings.snap_mode != SnapMode::None {
        nearest_frame(raw_tick, state.frame_rate.max(1))
    } else {
        raw_tick
    }
}

fn count_visible_keyframes(
    state: &EditorState,
    rows: &[VisibleTrackRow],
    start_idx: usize,
    visible_capacity: usize,
    metrics: &TimelineMetrics,
) -> usize {
    let Some(anim) = state.animation_data.as_ref() else {
        return 0;
    };
    let mut total = 0usize;
    for row in rows.iter().skip(start_idx).take(visible_capacity) {
        if let Some(track) = anim.get_track(&row.instance, &row.path) {
            let mut ticks = BTreeSet::new();
            track.collect_ticks(&mut ticks);
            total += ticks
                .into_iter()
                .filter(|&t| t >= metrics.start_tick && t <= metrics.end_tick)
                .count();
            if total > MAX_VISIBLE_KEYFRAMES {
                break;
            }
        }
    }
    total
}

fn is_summary_tick_selected(state: &EditorState, tick: Tick) -> bool {
    let mut any = false;
    for tracks in state.selected_keyframes.values() {
        for node in tracks.values() {
            if node.contains_tick(tick) {
                any = true;
            }
        }
    }
    any
}

fn is_track_tick_selected(
    state: &EditorState,
    instance: &str,
    path: &[String],
    tick: Tick,
) -> bool {
    let Some(tracks) = state.selected_keyframes.get(instance) else {
        return false;
    };
    let Some((first, rest)) = path.split_first() else {
        return false;
    };
    let mut node: &SelectionNode = match tracks.get(first) {
        Some(n) => n,
        None => return false,
    };
    for seg in rest {
        node = match node.components.get(seg) {
            Some(child) => child,
            None => return false,
        };
    }
    node.contains_tick(tick)
}

/// Aggregated leaf-component keyframe info at a single tick, matching Studio's
/// `TrackUtils.getComponentsInfo` (`TrackUtils` `PROTO_44`/`PROTO_45`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrackTickInfo {
    pub count: usize,
    pub complete: bool,
    pub easing_style: Option<EasingStyle>,
    pub interpolation_mode: Option<KeyInterpolationMode>,
}

fn count_leaf_tracks(track: &Track) -> usize {
    if track.components.is_empty() {
        1
    } else {
        track.components.values().map(count_leaf_tracks).sum()
    }
}

fn accumulate_leaf_info(
    track: &Track,
    start_tick: Tick,
    end_tick: Tick,
    leaf_count: usize,
    out: &mut std::collections::BTreeMap<Tick, TrackTickInfo>,
) {
    if track.components.is_empty() {
        for (&tick, kf) in &track.data {
            if tick < start_tick || tick > end_tick {
                continue;
            }
            if let Some(entry) = out.get_mut(&tick) {
                entry.count += 1;
                entry.complete = entry.count == leaf_count;
                if entry.easing_style != kf.easing_style {
                    entry.easing_style = None;
                }
                if entry.interpolation_mode != kf.interpolation_mode {
                    entry.interpolation_mode = None;
                }
            } else {
                out.insert(
                    tick,
                    TrackTickInfo {
                        count: 1,
                        complete: leaf_count == 1,
                        easing_style: kf.easing_style,
                        interpolation_mode: kf.interpolation_mode,
                    },
                );
            }
        }
    } else {
        for child in track.components.values() {
            accumulate_leaf_info(child, start_tick, end_tick, leaf_count, out);
        }
    }
}

pub(crate) fn collect_track_components_info(
    track: &Track,
    start_tick: Tick,
    end_tick: Tick,
) -> std::collections::BTreeMap<Tick, TrackTickInfo> {
    let leaf_count = count_leaf_tracks(track);
    let mut out = std::collections::BTreeMap::new();
    accumulate_leaf_info(track, start_tick, end_tick, leaf_count, &mut out);
    out
}

fn format_keyframe_tooltip(
    tick: Tick,
    fps: u32,
    unit: crate::anim_editor::state::TimelineUnit,
    kf_opt: Option<Keyframe>,
    resolved_easing: Option<EasingStyle>,
    resolved_interp: Option<KeyInterpolationMode>,
) -> String {
    let time_str = format_time(tick, fps, unit);
    let Some(kf) = kf_opt else {
        return if let Some(style) = resolved_easing {
            format!("Tick {tick} ({time_str})\nEasing: {}", style.as_str())
        } else if let Some(mode) = resolved_interp {
            format!("Tick {tick} ({time_str})\nInterpolation: {}", mode.as_str())
        } else {
            format!("Tick {tick} ({time_str})")
        };
    };
    let val_str = match kf.value {
        KeyframeValue::Number(v) => format!("{v:.3}"),
        KeyframeValue::Angle(v) => format!("{:.2}°", v.to_degrees()),
        KeyframeValue::Facs(v) => format!("{v:.2}"),
        KeyframeValue::Position(v) => format!("({:.2}, {:.2}, {:.2})", v.x, v.y, v.z),
        KeyframeValue::Quaternion(q) => {
            format!("Quat({:.2}, {:.2}, {:.2}, {:.2})", q.x, q.y, q.z, q.w)
        }
        KeyframeValue::CFrame(xf) => {
            format!("Pos({:.2}, {:.2}, {:.2})", xf.pos.x, xf.pos.y, xf.pos.z)
        }
    };
    if let Some(style) = kf.easing_style {
        let dir = kf.easing_direction.map(|d| d.as_str()).unwrap_or("In");
        format!(
            "Tick {tick} ({time_str})\nValue: {val_str}\nEasing: {}, {dir}",
            style.as_str()
        )
    } else if let Some(mode) = kf.interpolation_mode {
        format!(
            "Tick {tick} ({time_str})\nValue: {val_str}\nInterpolation: {}",
            mode.as_str()
        )
    } else {
        format!("Tick {tick} ({time_str})\nValue: {val_str}")
    }
}

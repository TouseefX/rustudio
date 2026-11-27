//! Top-level `EditorController` panel assembly (`ui_main.rs`) for RusTudio's
//! native 1:1 port of Roblox Studio's `AnimationClipEditor`.

use egui::{CursorIcon, Key, Pos2, Rect, RichText, Sense, Stroke, Ui, UiBuilder, Vec2};

use crate::anim_editor::constants::{
    SCALE_CONTROLS_PADDING, SCALE_HANDLE_BAR_WIDTH, SCALE_HANDLE_HIT_WIDTH, SCROLL_BAR_SIZE,
    SUMMARY_TRACK_HEIGHT, TICK_FREQUENCY, TIMELINE_HEIGHT, TIMELINE_MIN_WIDTH, TRACK_HEIGHT,
    TRACK_LIST_MIN_WIDTH,
};
use crate::anim_editor::model::{clamp_tick, nearest_frame, Tick};
use crate::anim_editor::state::{
    has_multiple_selected_ticks, selection_bounds, ActiveDialog, EditorMode, EditorState,
    PlayState, ToolMode,
};
use crate::anim_editor::theme::AceTheme;
use crate::anim_editor::ui_curve_editor::{fit_curve_view, show_curve_editor};
use crate::anim_editor::ui_dialogs::{
    show_active_dialog, show_inactive_cover, show_start_screen, show_toasts, DialogOutcome,
};
use crate::anim_editor::ui_menus::{show_settings_menu, MenuCommand};
use crate::anim_editor::ui_timeline::TimelineMetrics;

/// Per-frame side effects emitted by [`show_animation_editor_panel`] for the host editor.
#[derive(Debug, Default)]
pub struct PanelFrameResult {
    pub needs_rig_step: bool,
    pub menu_cmd: Option<MenuCommand>,
    pub dialog_outcome: DialogOutcome,
    pub focus_face_camera: bool,
}

/// Renders the complete 4-column `EditorController` layout plus floating windows,
/// dialogs, shortcuts, and toasts.
pub fn show_animation_editor_panel(
    ui: &mut Ui,
    state: &mut EditorState,
    is_playing_game: bool,
    dt: f32,
) -> PanelFrameResult {
    let mut result = PanelFrameResult::default();
    let theme = AceTheme::from_egui(ui.visuals());
    let panel_rect = ui.available_rect_before_wrap();

    if is_playing_game {
        show_inactive_cover(ui, "Animation Editor is unavailable while playing.", &theme);
        return result;
    }

    if state.rig_cache.root_ref.is_none() || state.animation_data.is_none() {
        if let Some(name) = show_start_screen(ui, state, &theme) {
            result.menu_cmd = Some(MenuCommand::CreateNew(name));
        }
        result.dialog_outcome = show_active_dialog(ui.ctx(), state, &theme);
        show_toasts(ui.ctx(), panel_rect, state, &theme, dt);
        return result;
    }

    // Advance playback heartbeat if Play or Reverse.
    if advance_playback(state, dt) {
        result.needs_rig_step = true;
        ui.ctx().request_repaint();
    }

    // Handle keyboard shortcuts when panel is hovered/focused and no text box or modal is active.
    let panel_hovered = ui
        .ctx()
        .pointer_hover_pos()
        .is_some_and(|p| panel_rect.contains(p));
    if panel_hovered
        && !ui.ctx().egui_wants_keyboard_input()
        && matches!(state.active_dialog, ActiveDialog::None)
    {
        if handle_shortcuts(ui.ctx(), state) {
            result.needs_rig_step = true;
        }
    }

    let total_w = panel_rect.width().max(1.0);
    let total_h = panel_rect.height().max(1.0);
    let splitter_w = 2.0;
    let right_strip_w = SCROLL_BAR_SIZE.min((total_w * 0.25).max(1.0));
    let avail_for_split = (total_w - splitter_w - right_strip_w).max(2.0);
    let min_timeline_w = TIMELINE_MIN_WIDTH.min(avail_for_split * 0.45);
    let max_left_w = (avail_for_split - min_timeline_w).max(1.0);
    let min_left_w = TRACK_LIST_MIN_WIDTH.min(max_left_w * 0.6).min(max_left_w);
    let left_w = state.track_list_width.clamp(min_left_w, max_left_w);

    let left_rect = Rect::from_min_size(panel_rect.min, Vec2::new(left_w, total_h));
    let splitter_rect = Rect::from_min_size(
        Pos2::new(left_rect.max.x, panel_rect.min.y),
        Vec2::new(splitter_w, total_h),
    );
    let right_strip_rect = Rect::from_min_size(
        Pos2::new((panel_rect.max.x - right_strip_w).max(splitter_rect.max.x), panel_rect.min.y),
        Vec2::new(right_strip_w, total_h),
    );
    let center_rect = Rect::from_min_max(
        Pos2::new(splitter_rect.max.x, panel_rect.min.y),
        Pos2::new(right_strip_rect.min.x.max(splitter_rect.max.x + 1.0), panel_rect.max.y),
    );

    // Paint panel background.
    ui.painter().rect_filled(panel_rect, 0.0, theme.background);

    // ------------------------------------------------------------------------
    // Column 1: AnimationControlPanel + TrackList
    // ------------------------------------------------------------------------
    let control_rect =
        Rect::from_min_size(left_rect.min, Vec2::new(left_rect.width(), TIMELINE_HEIGHT));
    let mut open_clip_menu_pos: Option<Pos2> = None;
    let mut toggled_clip_menu = false;
    ui.scope_builder(UiBuilder::new().max_rect(control_rect), |ui| {
        match crate::anim_editor::ui_control_panel::show_control_panel(ui, state, &theme) {
            crate::anim_editor::ui_control_panel::ControlPanelAction::None => {}
            crate::anim_editor::ui_control_panel::ControlPanelAction::OpenClipMenu(pos) => {
                open_clip_menu_pos = Some(pos);
                toggled_clip_menu = true;
            }
            crate::anim_editor::ui_control_panel::ControlPanelAction::PlayheadChanged => {
                result.needs_rig_step = true;
            }
            crate::anim_editor::ui_control_panel::ControlPanelAction::EditingLengthChanged(len) => {
                crate::anim_editor::ops::update_editing_length(state, len);
                result.needs_rig_step = true;
            }
        }
    });
    if toggled_clip_menu {
        state.menus.show_clip_menu = !state.menus.show_clip_menu;
    }
    let clip_menu_id = ui.id().with("ace_clip_menu_popup");
    let menu_pos =
        open_clip_menu_pos.unwrap_or_else(|| Pos2::new(control_rect.min.x, control_rect.max.y));
    let mut clip_menu_open = state.menus.show_clip_menu;
    let mut close_clip_menu = false;
    egui::Popup::new(
        clip_menu_id,
        ui.ctx().clone(),
        egui::PopupAnchor::Position(menu_pos),
        ui.layer_id(),
    )
    .kind(egui::PopupKind::Menu)
    .open_bool(&mut clip_menu_open)
    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
    .show(|ui| {
        if let Some(cmd) = crate::anim_editor::ui_menus::show_animation_clip_menu(ui, state) {
            result.menu_cmd = Some(cmd);
            close_clip_menu = true;
            ui.close();
        }
    });
    state.menus.show_clip_menu = clip_menu_open && !close_clip_menu;

    let track_list_rect = Rect::from_min_max(
        Pos2::new(left_rect.min.x, control_rect.max.y),
        left_rect.max,
    );
    ui.scope_builder(UiBuilder::new().max_rect(track_list_rect), |ui| {
        match crate::anim_editor::ui_tracklist::show_track_list(ui, state, &theme, TRACK_HEIGHT) {
            crate::anim_editor::ui_tracklist::TrackListAction::None
            | crate::anim_editor::ui_tracklist::TrackListAction::SelectionChanged => {}
            crate::anim_editor::ui_tracklist::TrackListAction::ValueEdited
            | crate::anim_editor::ui_tracklist::TrackListAction::TrackListChanged => {
                result.needs_rig_step = true;
            }
            crate::anim_editor::ui_tracklist::TrackListAction::OpenAddEventAtPlayhead => {
                let t = state.playback.playhead;
                crate::anim_editor::events::open_edit_events_dialog(state, t);
            }
            crate::anim_editor::ui_tracklist::TrackListAction::ToggleEditorMode(mode) => {
                if mode == EditorMode::Curves && !state.is_channel_animation() {
                    state.active_dialog = ActiveDialog::PromoteToCurves;
                } else {
                    state.editor_mode = mode;
                    if mode == EditorMode::Curves {
                        fit_curve_view(state);
                    }
                }
            }
            crate::anim_editor::ui_tracklist::TrackListAction::ToggleIk => {
                state.ik.show_tree = !state.ik.show_tree;
            }
            crate::anim_editor::ui_tracklist::TrackListAction::ToggleFaceControls => {
                state.facs.show_panel = !state.facs.show_panel;
            }
        }
    });

    // ------------------------------------------------------------------------
    // Column 2: Draggable TrackListBorder splitter (2px visual / 5px hit area)
    // ------------------------------------------------------------------------
    ui.painter().rect_filled(splitter_rect, 0.0, theme.border);
    let splitter_hit = Rect::from_min_max(
        Pos2::new(splitter_rect.min.x - 2.0, splitter_rect.min.y),
        Pos2::new(splitter_rect.max.x + 1.0, splitter_rect.max.y),
    );
    let split_resp = ui.interact(
        splitter_hit,
        ui.id().with("ace_track_list_splitter"),
        Sense::drag(),
    );
    if split_resp.hovered() || split_resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    if split_resp.dragged() {
        let dx = split_resp.drag_delta().x;
        state.track_list_width = (left_w + dx).clamp(min_left_w, max_left_w);
    }

    // ------------------------------------------------------------------------
    // Column 3: TimelineHeader + (DopeSheet | CurveEditor) + ScaleControls + Scrubber + ZoomBar
    // ------------------------------------------------------------------------
    // Match Column 1's combined ControlPanel (TIMELINE_HEIGHT) + TrackList header toolbar (TIMELINE_HEIGHT)
    // so Events, Summary, and every Track row align 1:1 horizontally across TrackList and DopeSheet.
    let timeline_rect = Rect::from_min_size(
        center_rect.min,
        Vec2::new(center_rect.width(), TIMELINE_HEIGHT * 2.0),
    );
    let zoom_bar_rect = Rect::from_min_max(
        Pos2::new(
            center_rect.min.x,
            (center_rect.max.y - SCROLL_BAR_SIZE).max(timeline_rect.max.y),
        ),
        center_rect.max,
    );
    let body_rect = Rect::from_min_max(
        Pos2::new(center_rect.min.x, timeline_rect.max.y),
        Pos2::new(center_rect.max.x, zoom_bar_rect.min.y),
    );

    let metrics = TimelineMetrics::from_state(timeline_rect, state);
    let header_changed = ui
        .scope_builder(UiBuilder::new().max_rect(timeline_rect), |ui| {
            crate::anim_editor::ui_timeline::show_timeline_header(ui, state, &metrics, &theme)
        })
        .inner;
    if header_changed {
        result.needs_rig_step = true;
    }

    // 1px TimelineBorder separator
    ui.painter().line_segment(
        [
            Pos2::new(center_rect.min.x, timeline_rect.max.y - 1.0),
            Pos2::new(center_rect.max.x, timeline_rect.max.y - 1.0),
        ],
        Stroke::new(1.0, theme.border),
    );

    // Main editor body: DopeSheet or CurveEditor
    ui.scope_builder(UiBuilder::new().max_rect(body_rect), |ui| {
        match state.editor_mode {
            EditorMode::Curves => {
                if show_curve_editor(ui, state, &metrics, &theme) {
                    result.needs_rig_step = true;
                }
            }
            EditorMode::DopeSheet => {
                let ds_action =
                    crate::anim_editor::ui_dopesheet::show_dopesheet(ui, state, &metrics, &theme);
                match ds_action {
                    crate::anim_editor::ui_dopesheet::DopeSheetAction::None => {}
                    crate::anim_editor::ui_dopesheet::DopeSheetAction::OpenEditEventsDialog(
                        tick,
                    ) => {
                        crate::anim_editor::events::open_edit_events_dialog(state, tick);
                    }
                    crate::anim_editor::ui_dopesheet::DopeSheetAction::OpenChangeDurationDialog => {
                        let end_tick = state
                            .animation_data
                            .as_ref()
                            .map(|a| a.metadata.end_tick)
                            .unwrap_or(0)
                            .max(state.playback.editing_length);
                        let cur_frames = ((end_tick as f64) * (state.frame_rate.max(1) as f64)
                            / (TICK_FREQUENCY as f64))
                            .round() as i32;
                        state.active_dialog = ActiveDialog::ChangeDuration {
                            text: cur_frames.to_string(),
                            current_duration_frames: cur_frames,
                            error: None,
                        };
                    }
                    _ => {
                        result.needs_rig_step = true;
                    }
                }
            }
        }
    });

    // ScaleControls overlay (when >= 2 distinct ticks selected and not marquee-dragging)
    if has_multiple_selected_ticks(&state.selected_keyframes)
        && !state.curve_editor.is_dragging_marquee
    {
        if show_scale_controls_overlay(ui, body_rect, state, &metrics, &theme) {
            result.needs_rig_step = true;
        }
    }

    // Scrubber vertical line + head overlay
    draw_scrubber_overlay(ui, center_rect, body_rect, state, &metrics, &theme);

    // Horizontal ZoomBar at bottom of center column
    ui.scope_builder(UiBuilder::new().max_rect(zoom_bar_rect), |ui| {
        crate::anim_editor::ui_timeline::show_zoom_bar(ui, state, &theme);
    });

    // ------------------------------------------------------------------------
    // Column 4: SettingsButton gear icon + Vertical track scrollbar
    // ------------------------------------------------------------------------
    let settings_btn_rect = Rect::from_min_size(
        right_strip_rect.min,
        Vec2::new(SCROLL_BAR_SIZE, TIMELINE_HEIGHT * 2.0),
    );
    let settings_resp = ui.put(
        settings_btn_rect,
        egui::Button::new(RichText::new("⚙").size(12.0).color(theme.text))
            .fill(theme.background)
            .stroke(Stroke::new(1.0, theme.border)),
    );
    egui::Popup::menu(&settings_resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            show_settings_menu(ui, state);
        });

    let vscroll_rect = Rect::from_min_max(
        Pos2::new(right_strip_rect.min.x, settings_btn_rect.max.y),
        Pos2::new(
            right_strip_rect.max.x,
            (right_strip_rect.max.y - SCROLL_BAR_SIZE).max(settings_btn_rect.max.y),
        ),
    );
    if state.editor_mode == EditorMode::DopeSheet {
        let event_h = if state.settings.show_events && state.animation_data.is_some() {
            TRACK_HEIGHT
        } else {
            0.0
        };
        let tracks_viewport_h =
            (body_rect.height() - event_h - SUMMARY_TRACK_HEIGHT).max(TRACK_HEIGHT);
        show_vertical_track_scrollbar(ui, vscroll_rect, tracks_viewport_h, state, &theme);
    } else {
        ui.painter()
            .rect_filled(vscroll_rect, 0.0, theme.scrollbar_bg);
    }

    // ------------------------------------------------------------------------
    // Floating Windows, Dialogs & Toasts on ui.ctx()
    // ------------------------------------------------------------------------
    let ctx = ui.ctx();

    if state.ik.show_tree {
        crate::anim_editor::ui_ik::show_ik_window(ctx, state, &theme);
    }

    if state.facs.show_panel {
        if crate::anim_editor::ui_facs::show_face_controls_window(ctx, state, &theme) {
            result.focus_face_camera = true;
        }
    }

    if matches!(state.active_dialog, ActiveDialog::EditEvents(_)) {
        crate::anim_editor::events::show_edit_events_dialog(ctx, state, &theme);
    }

    result.dialog_outcome = show_active_dialog(ctx, state, &theme);
    show_toasts(ctx, panel_rect, state, &theme, dt);

    result
}

// ============================================================================
// Playback Heartbeat
// ============================================================================

fn advance_playback(state: &mut EditorState, dt: f32) -> bool {
    let play_state = state.playback.play_state;
    if !matches!(play_state, PlayState::Play | PlayState::Reverse) {
        return false;
    }
    let Some(anim) = &state.animation_data else {
        return false;
    };
    let end_tick = anim.metadata.end_tick;
    if end_tick <= 0 {
        state.playback.set_play_state(PlayState::Pause);
        return false;
    }

    let dir = if play_state == PlayState::Reverse {
        -1.0_f32
    } else {
        1.0_f32
    };
    let delta_ticks = dt * state.playback.playback_speed * (TICK_FREQUENCY as f32) * dir;
    let next = state.playback.playhead as f32 + delta_ticks;

    if anim.metadata.looping {
        let wrapped = next.rem_euclid(end_tick as f32);
        state.playback.playhead = wrapped.round() as Tick;
    } else if play_state == PlayState::Play && next >= end_tick as f32 {
        state.playback.playhead = end_tick;
        state.playback.set_play_state(PlayState::Pause);
    } else if play_state == PlayState::Reverse && next <= 0.0 {
        state.playback.playhead = 0;
        state.playback.set_play_state(PlayState::Pause);
    } else {
        state.playback.playhead = (next.round() as Tick).clamp(0, end_tick);
    }

    true
}

// ============================================================================
// Keyboard Shortcuts
// ============================================================================

fn handle_shortcuts(ctx: &egui::Context, state: &mut EditorState) -> bool {
    let mut needs_step = false;

    ctx.input(|i| {
        let ctrl = i.modifiers.ctrl || i.modifiers.command;
        let shift = i.modifiers.shift;

        // Space: TogglePlay
        if i.key_pressed(Key::Space) && !ctrl {
            if state.playback.play_state == PlayState::Pause {
                let end_tick = state
                    .animation_data
                    .as_ref()
                    .map(|a| a.metadata.end_tick)
                    .unwrap_or(0);
                if end_tick > 0 {
                    if state.playback.playhead >= end_tick {
                        state.playback.playhead = 0;
                    }
                    state.playback.set_play_state(PlayState::Play);
                }
            } else {
                state.playback.set_play_state(PlayState::Pause);
            }
        }

        // Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y: Undo / Redo
        if ctrl && i.key_pressed(Key::Z) {
            if shift {
                crate::anim_editor::history::redo(state);
            } else {
                crate::anim_editor::history::undo(state);
            }
            needs_step = true;
        } else if ctrl && i.key_pressed(Key::Y) {
            crate::anim_editor::history::redo(state);
            needs_step = true;
        }

        // Ctrl+C / Ctrl+V / Ctrl+X
        if ctrl && i.key_pressed(Key::C) {
            if !state.selected_events.is_empty() {
                crate::anim_editor::events::copy_selected_events(state);
            } else {
                crate::anim_editor::ops::copy_selected_keyframes(state);
            }
        }
        if ctrl && i.key_pressed(Key::X) {
            if !state.selected_events.is_empty() {
                crate::anim_editor::events::copy_selected_events(state);
                crate::anim_editor::events::delete_selected_events(state);
            } else {
                crate::anim_editor::ops::copy_selected_keyframes(state);
                crate::anim_editor::ops::delete_selected_keyframes(state);
            }
            needs_step = true;
        }
        if ctrl && i.key_pressed(Key::V) {
            let t = state.playback.playhead;
            if matches!(
                state.clipboard,
                crate::anim_editor::state::Clipboard::Events(_)
            ) {
                crate::anim_editor::events::paste_events(state, t);
            } else {
                crate::anim_editor::ops::paste_keyframes(state, t);
            }
            needs_step = true;
        }

        // Delete / Backspace
        if i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace) {
            if !state.selected_events.is_empty() {
                crate::anim_editor::events::delete_selected_events(state);
            } else {
                crate::anim_editor::ops::delete_selected_keyframes(state);
            }
            needs_step = true;
        }

        // Ctrl+A: Select all keyframes
        if ctrl && i.key_pressed(Key::A) {
            crate::anim_editor::selection::select_all_keyframes(state);
        }

        // R: Toggle Rotate / Move tool
        if i.key_pressed(Key::R) && !ctrl {
            state.viewport_dragger.tool = match state.viewport_dragger.tool {
                ToolMode::Rotate => ToolMode::Move,
                ToolMode::Move => ToolMode::Rotate,
            };
        }

        // Ctrl+L: Toggle Local / World space
        if ctrl && i.key_pressed(Key::L) {
            state.viewport_dragger.world_space = !state.viewport_dragger.world_space;
        }

        // K: Add keyframe at playhead for selected tracks
        if i.key_pressed(Key::K) && !ctrl {
            let paths = state.selected_tracks.clone();
            for path in paths {
                crate::anim_editor::ops::add_keyframe_at_playhead(state, "Root", &path);
            }
            needs_step = true;
        }

        // Left / Right / Shift+Left / Shift+Right
        let frame_step = (TICK_FREQUENCY / state.frame_rate.max(1) as i32).max(1);
        if i.key_pressed(Key::ArrowLeft) {
            if shift {
                if let Some(prev) = find_adjacent_summary_tick(state, false) {
                    state.playback.playhead = prev;
                    needs_step = true;
                }
            } else {
                state.playback.playhead = clamp_tick(nearest_frame(
                    state.playback.playhead - frame_step,
                    state.frame_rate,
                ));
                needs_step = true;
            }
        }
        if i.key_pressed(Key::ArrowRight) {
            if shift {
                if let Some(next) = find_adjacent_summary_tick(state, true) {
                    state.playback.playhead = next;
                    needs_step = true;
                }
            } else {
                state.playback.playhead = clamp_tick(nearest_frame(
                    state.playback.playhead + frame_step,
                    state.frame_rate,
                ));
                needs_step = true;
            }
        }

        // Home / End
        if i.key_pressed(Key::Home) {
            state.playback.playhead = 0;
            needs_step = true;
        }
        if i.key_pressed(Key::End) {
            if let Some(anim) = &state.animation_data {
                state.playback.playhead = anim.metadata.end_tick;
                needs_step = true;
            }
        }

        // F: Fit Curve Editor view
        if i.key_pressed(Key::F) && !ctrl && state.editor_mode == EditorMode::Curves {
            fit_curve_view(state);
        }
    });

    needs_step
}

fn find_adjacent_summary_tick(state: &EditorState, forward: bool) -> Option<Tick> {
    let anim = state.animation_data.as_ref()?;
    let ticks = anim.get_summary_ticks();
    let cur = state.playback.playhead;
    if forward {
        ticks.into_iter().find(|&t| t > cur)
    } else {
        ticks.into_iter().rev().find(|&t| t < cur)
    }
}

// ============================================================================
// Scrubber, ScaleControls & Vertical Track Scrollbar
// ============================================================================

fn draw_scrubber_overlay(
    ui: &Ui,
    center_rect: Rect,
    body_rect: Rect,
    state: &EditorState,
    metrics: &TimelineMetrics,
    theme: &AceTheme,
) {
    let playhead = state.playback.playhead;
    if playhead < metrics.start_tick || playhead > metrics.end_tick {
        return;
    }

    let pad = metrics.left_pad;
    let usable_w = (center_rect.width() - metrics.left_pad - metrics.right_pad).max(1.0);
    let span = (metrics.end_tick - metrics.start_tick).max(1) as f32;
    let x = center_rect.min.x + pad + ((playhead - metrics.start_tick) as f32 / span) * usable_w;

    let painter = ui.painter_at(Rect::from_min_max(
        center_rect.min,
        Pos2::new(center_rect.max.x, body_rect.max.y),
    ));

    // Scrubber head (5x5 inverted triangle/pentagon at top of timeline)
    let head_pts = vec![
        Pos2::new(x - 4.0, center_rect.min.y + 1.0),
        Pos2::new(x + 4.0, center_rect.min.y + 1.0),
        Pos2::new(x + 4.0, center_rect.min.y + 7.0),
        Pos2::new(x, center_rect.min.y + 12.0),
        Pos2::new(x - 4.0, center_rect.min.y + 7.0),
    ];
    painter.add(egui::Shape::convex_polygon(
        head_pts,
        theme.scrubber,
        Stroke::NONE,
    ));

    // 1px vertical scrubber line
    painter.line_segment(
        [
            Pos2::new(x, center_rect.min.y + 11.0),
            Pos2::new(x, body_rect.max.y),
        ],
        Stroke::new(1.0, theme.scrubber),
    );
}

fn show_scale_controls_overlay(
    ui: &mut Ui,
    body_rect: Rect,
    state: &mut EditorState,
    metrics: &TimelineMetrics,
    theme: &AceTheme,
) -> bool {
    let Some((earliest, latest)) = selection_bounds(&state.selected_keyframes) else {
        return false;
    };
    if earliest >= latest {
        return false;
    }

    let mut modified = false;
    let pad = metrics.left_pad;
    let usable_w = (body_rect.width() - metrics.left_pad - metrics.right_pad).max(1.0);
    let span = (metrics.end_tick - metrics.start_tick).max(1) as f32;
    let to_x =
        |t: Tick| body_rect.min.x + pad + ((t - metrics.start_tick) as f32 / span) * usable_w;

    let left_x = to_x(earliest) - SCALE_CONTROLS_PADDING;
    let right_x = to_x(latest) + SCALE_CONTROLS_PADDING;

    let painter = ui.painter_at(body_rect);
    let bar_col = theme.scale_controls_main;

    for (idx, hx) in [left_x, right_x].into_iter().enumerate() {
        let hit_rect = Rect::from_center_size(
            Pos2::new(hx, body_rect.center().y),
            Vec2::new(SCALE_HANDLE_HIT_WIDTH, body_rect.height()),
        );
        let vis_rect = Rect::from_center_size(
            Pos2::new(hx, body_rect.center().y),
            Vec2::new(SCALE_HANDLE_BAR_WIDTH, body_rect.height()),
        );
        painter.rect_filled(vis_rect, 0.0, bar_col);

        let resp = ui.interact(
            hit_rect,
            ui.id().with(("ace_scale_handle", idx)),
            Sense::drag(),
        );
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        if resp.drag_started() {
            let pivot = if idx == 0 { latest } else { earliest };
            crate::anim_editor::ops::begin_keyframe_drag(
                state,
                pivot,
                None,
                crate::anim_editor::state::DragMode::Keyframe,
            );
        }
        if resp.dragged() {
            if let Some(pos) = ui.ctx().pointer_latest_pos() {
                let raw_t = metrics.start_tick as f32
                    + ((pos.x - (body_rect.min.x + pad)) / usable_w) * span;
                let target_t = clamp_tick(raw_t.round() as Tick);
                crate::anim_editor::ops::update_keyframe_scale(state, target_t);
                modified = true;
            }
        }
        if resp.drag_stopped() {
            crate::anim_editor::ops::end_drag(state, true);
            modified = true;
        }
    }

    modified
}

fn show_vertical_track_scrollbar(
    ui: &mut Ui,
    rect: Rect,
    viewport_height: f32,
    state: &mut EditorState,
    theme: &AceTheme,
) {
    ui.painter().rect_filled(rect, 0.0, theme.scrollbar_bg);

    let total_rows: usize = crate::anim_editor::ui_tracklist::collect_visible_rows(state)
        .len()
        .max(1);
    let visible_rows = ((viewport_height / TRACK_HEIGHT).floor() as usize).max(1);

    if rect.height() <= SCROLL_BAR_SIZE * 2.0 + 4.0 {
        return;
    }

    let up_rect = Rect::from_min_size(rect.min, Vec2::splat(SCROLL_BAR_SIZE));
    let down_rect = Rect::from_min_size(
        Pos2::new(rect.min.x, rect.max.y - SCROLL_BAR_SIZE),
        Vec2::splat(SCROLL_BAR_SIZE),
    );
    let track_rect = Rect::from_min_max(
        Pos2::new(rect.min.x + 2.0, up_rect.max.y + 1.0),
        Pos2::new(rect.max.x - 2.0, down_rect.min.y - 1.0),
    );

    if ui
        .put(
            up_rect,
            egui::Button::new(RichText::new("▲").size(8.0).color(theme.scrollbar_arrow))
                .fill(theme.scrollbar_control)
                .frame(false),
        )
        .clicked()
    {
        state.top_track_index = state.top_track_index.saturating_sub(1).max(1);
    }

    if ui
        .put(
            down_rect,
            egui::Button::new(RichText::new("▼").size(8.0).color(theme.scrollbar_arrow))
                .fill(theme.scrollbar_control)
                .frame(false),
        )
        .clicked()
    {
        let max_top = total_rows.saturating_sub(visible_rows).saturating_add(1);
        state.top_track_index = (state.top_track_index + 1).min(max_top);
    }

    let max_top = total_rows.saturating_sub(visible_rows).saturating_add(1);
    state.top_track_index = state.top_track_index.clamp(1, max_top);

    let thumb_frac = (visible_rows as f32 / total_rows as f32).clamp(0.1, 1.0);
    let thumb_h = (track_rect.height() * thumb_frac).max(14.0);
    let travel = (track_rect.height() - thumb_h).max(0.0);
    let t = if max_top <= 1 {
        0.0
    } else {
        (state.top_track_index - 1) as f32 / (max_top - 1) as f32
    };
    let thumb_rect = Rect::from_min_size(
        Pos2::new(track_rect.min.x, track_rect.min.y + travel * t),
        Vec2::new(track_rect.width(), thumb_h),
    );

    let thumb_resp = ui.interact(thumb_rect, ui.id().with("ace_vscroll_thumb"), Sense::drag());
    let thumb_col = if thumb_resp.dragged() {
        theme.scrollbar_pressed
    } else if thumb_resp.hovered() {
        theme.scrollbar_hover
    } else {
        theme.scrollbar_control
    };
    ui.painter().rect_filled(thumb_rect, 2.0, thumb_col);

    if thumb_resp.dragged() && travel > 0.0 && max_top > 1 {
        let dy = thumb_resp.drag_delta().y;
        let new_t = (t + dy / travel).clamp(0.0, 1.0);
        state.top_track_index = 1 + (new_t * (max_top - 1) as f32).round() as usize;
    }
}

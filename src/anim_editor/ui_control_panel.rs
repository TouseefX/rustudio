//! Top-left `AnimationControlPanel` UI (`AnimationClipDropdown`, 7-button
//! `MediaControls`, and `TimeDisplay`) derived from
//! `Components/AnimationControlPanel/*`.

use egui::{Align2, Color32, CornerRadius, FontId, Id, Pos2, Rect, Sense, Stroke, Vec2};

use crate::anim_editor::constants::{
    CONTROLS_WIDTH, DEFAULT_ANIMATION_NAME, MAX_ANIMATION_LENGTH, MIN_TIMELINE_RANGE,
    TIMELINE_HEIGHT, TIME_DISPLAY_WIDTH,
};
use crate::anim_editor::model::Tick;
use crate::anim_editor::playback::{
    format_time, parse_time_input, set_playhead, skip_to_end, skip_to_next_keyframe,
    skip_to_prev_keyframe, skip_to_start, toggle_looping, toggle_play, toggle_reverse,
};
use crate::anim_editor::state::{EditorState, PlayState};
use crate::anim_editor::theme::AceTheme;

/// High-level action emitted by [`show_control_panel`].
#[derive(Debug, Clone, PartialEq)]
pub enum ControlPanelAction {
    None,
    OpenClipMenu(Pos2),
    PlayheadChanged,
    EditingLengthChanged(Tick),
}

/// Renders the 24px `AnimationControlPanel` bar:
/// - Left: `AnimationClipDropdown` (clip name + dirty `*` + `⋯` menu button)
/// - Center: 7 `24x24` `MediaControls` buttons with vector icons
/// - Right: `TimeDisplay` editable current playhead / editing length inputs
pub fn show_control_panel(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    theme: &AceTheme,
) -> ControlPanelAction {
    let mut action = ControlPanelAction::None;

    let avail_w = ui
        .available_width()
        .max(CONTROLS_WIDTH + TIME_DISPLAY_WIDTH + 40.0);
    let (panel_rect, _) =
        ui.allocate_exact_size(Vec2::new(avail_w, TIMELINE_HEIGHT), Sense::hover());

    let painter = ui.painter();
    painter.rect_filled(panel_rect, CornerRadius::ZERO, theme.background);
    painter.line_segment(
        [panel_rect.left_bottom(), panel_rect.right_bottom()],
        Stroke::new(1.0, theme.border),
    );

    let dropdown_w = (panel_rect.width() - CONTROLS_WIDTH - TIME_DISPLAY_WIDTH).max(40.0);
    let dropdown_rect = Rect::from_min_size(panel_rect.min, Vec2::new(dropdown_w, TIMELINE_HEIGHT));
    let controls_rect = Rect::from_min_size(
        Pos2::new(dropdown_rect.max.x, panel_rect.min.y),
        Vec2::new(CONTROLS_WIDTH, TIMELINE_HEIGHT),
    );
    let time_rect = Rect::from_min_size(
        Pos2::new(controls_rect.max.x, panel_rect.min.y),
        Vec2::new(TIME_DISPLAY_WIDTH, TIMELINE_HEIGHT),
    );

    // 1. AnimationClipDropdown
    if let Some(menu_pos) = draw_clip_dropdown(ui, dropdown_rect, state, theme) {
        action = ControlPanelAction::OpenClipMenu(menu_pos);
    }

    // 2. MediaControls (7 x 24x24 buttons)
    if draw_media_controls(ui, controls_rect, state, theme) && action == ControlPanelAction::None {
        action = ControlPanelAction::PlayheadChanged;
    }

    // 3. TimeDisplay
    let time_action = draw_time_display(ui, time_rect, state, theme);
    if time_action != ControlPanelAction::None {
        action = time_action;
    }

    action
}

fn draw_clip_dropdown(
    ui: &mut egui::Ui,
    rect: Rect,
    state: &EditorState,
    theme: &AceTheme,
) -> Option<Pos2> {
    let menu_btn_w = 22.0;
    let btn_rect = Rect::from_min_size(
        Pos2::new(rect.max.x - menu_btn_w, rect.min.y),
        Vec2::new(menu_btn_w, rect.height()),
    );
    let label_rect = Rect::from_min_max(
        Pos2::new(rect.min.x + 6.0, rect.min.y),
        Pos2::new(btn_rect.min.x - 2.0, rect.max.y),
    );

    let clip_name = state
        .animation_data
        .as_ref()
        .map(|d| {
            if d.metadata.name.is_empty() {
                DEFAULT_ANIMATION_NAME
            } else {
                d.metadata.name.as_str()
            }
        })
        .unwrap_or(DEFAULT_ANIMATION_NAME);

    let display_name = if state.is_dirty {
        format!("{clip_name}*")
    } else {
        clip_name.to_string()
    };

    let painter = ui.painter().with_clip_rect(label_rect);
    painter.text(
        Pos2::new(label_rect.min.x, label_rect.center().y),
        Align2::LEFT_CENTER,
        &display_name,
        FontId::proportional(theme.font_size_track - 1.0),
        theme.text,
    );

    let resp = ui
        .interact(btn_rect, ui.id().with("clip_menu_btn"), Sense::click())
        .on_hover_text("Animation Clip Menu");
    let p = ui.painter();
    if resp.hovered() || state.menus.show_clip_menu {
        p.rect_filled(
            btn_rect.shrink(2.0),
            CornerRadius::same(3),
            theme.scrollbar_hover,
        );
    }
    let cy = btn_rect.center().y;
    let cx = btn_rect.center().x;
    for dx in [-4.0, 0.0, 4.0] {
        p.circle_filled(Pos2::new(cx + dx, cy), 1.3, theme.text);
    }

    // Subtle right divider
    p.line_segment(
        [
            Pos2::new(rect.max.x, rect.min.y + 3.0),
            Pos2::new(rect.max.x, rect.max.y - 3.0),
        ],
        Stroke::new(1.0, theme.border),
    );

    if resp.clicked() {
        Some(Pos2::new(rect.min.x, rect.max.y))
    } else {
        None
    }
}

/// Draws the 7 `24x24` MediaControls buttons. Returns `true` if playhead moved.
fn draw_media_controls(
    ui: &mut egui::Ui,
    rect: Rect,
    state: &mut EditorState,
    theme: &AceTheme,
) -> bool {
    let mut playhead_changed = false;
    let btn_size = 24.0;
    let total_btns_w = btn_size * 7.0;
    let start_x = rect.min.x + ((rect.width() - total_btns_w) * 0.5).max(0.0);

    let has_anim = state.animation_data.is_some();
    let is_playing = state.playback.play_state == PlayState::Play;
    let is_reversing = state.playback.play_state == PlayState::Reverse;
    let is_looping = state
        .animation_data
        .as_ref()
        .is_some_and(|d| d.metadata.looping);

    let buttons: [(&str, &str, bool); 7] = [
        ("go_first", "Skip to Start", false),
        ("skip_prev", "Previous Keyframe", false),
        ("reverse", "Play in Reverse", is_reversing),
        (
            "play_pause",
            if is_playing { "Pause" } else { "Play" },
            is_playing,
        ),
        ("skip_next", "Next Keyframe", false),
        ("go_last", "Skip to End", false),
        ("loop", "Toggle Looping", is_looping),
    ];

    for (idx, (id_key, tooltip, active)) in buttons.iter().enumerate() {
        let b_rect = Rect::from_min_size(
            Pos2::new(start_x + (idx as f32) * btn_size, rect.min.y),
            Vec2::splat(btn_size),
        );
        let resp = ui
            .interact(b_rect, ui.id().with(*id_key), Sense::click())
            .on_hover_text(*tooltip);

        let painter = ui.painter();
        if *active {
            painter.rect_filled(b_rect.shrink(2.0), CornerRadius::same(3), theme.accent);
        } else if resp.hovered() && has_anim {
            painter.rect_filled(
                b_rect.shrink(2.0),
                CornerRadius::same(3),
                theme.scrollbar_hover,
            );
        }

        let icon_color = if !has_anim {
            theme.dimmed_text
        } else if *active {
            theme.accent_text
        } else {
            theme.text
        };

        paint_media_icon(painter, b_rect.center(), idx, is_playing, icon_color);

        if resp.clicked() && has_anim {
            match idx {
                0 => {
                    skip_to_start(state);
                    playhead_changed = true;
                }
                1 => {
                    skip_to_prev_keyframe(state);
                    playhead_changed = true;
                }
                2 => toggle_reverse(state),
                3 => toggle_play(state),
                4 => {
                    skip_to_next_keyframe(state);
                    playhead_changed = true;
                }
                5 => {
                    skip_to_end(state);
                    playhead_changed = true;
                }
                6 => toggle_looping(state),
                _ => {}
            }
        }
    }

    ui.painter().line_segment(
        [
            Pos2::new(rect.max.x, rect.min.y + 3.0),
            Pos2::new(rect.max.x, rect.max.y - 3.0),
        ],
        Stroke::new(1.0, theme.border),
    );

    playhead_changed
}

fn paint_media_icon(
    painter: &egui::Painter,
    c: Pos2,
    idx: usize,
    is_playing: bool,
    color: Color32,
) {
    let stroke = Stroke::new(1.5, color);
    match idx {
        // 0: Skip to Start |<
        0 => {
            painter.line_segment(
                [
                    Pos2::new(c.x - 4.5, c.y - 4.5),
                    Pos2::new(c.x - 4.5, c.y + 4.5),
                ],
                stroke,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(c.x - 3.0, c.y),
                    Pos2::new(c.x + 4.5, c.y - 4.5),
                    Pos2::new(c.x + 4.5, c.y + 4.5),
                ],
                color,
                Stroke::NONE,
            ));
        }
        // 1: Previous Keyframe <|
        1 => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(c.x - 4.5, c.y),
                    Pos2::new(c.x + 1.5, c.y - 4.5),
                    Pos2::new(c.x + 1.5, c.y + 4.5),
                ],
                color,
                Stroke::NONE,
            ));
            painter.line_segment(
                [
                    Pos2::new(c.x + 4.0, c.y - 4.5),
                    Pos2::new(c.x + 4.0, c.y + 4.5),
                ],
                stroke,
            );
        }
        // 2: Reverse <
        2 => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(c.x - 4.5, c.y),
                    Pos2::new(c.x + 4.0, c.y - 5.0),
                    Pos2::new(c.x + 4.0, c.y + 5.0),
                ],
                color,
                Stroke::NONE,
            ));
        }
        // 3: Play > or Pause ||
        3 => {
            if is_playing {
                let bar_l = Rect::from_min_max(
                    Pos2::new(c.x - 4.0, c.y - 4.5),
                    Pos2::new(c.x - 1.2, c.y + 4.5),
                );
                let bar_r = Rect::from_min_max(
                    Pos2::new(c.x + 1.2, c.y - 4.5),
                    Pos2::new(c.x + 4.0, c.y + 4.5),
                );
                painter.rect_filled(bar_l, CornerRadius::ZERO, color);
                painter.rect_filled(bar_r, CornerRadius::ZERO, color);
            } else {
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        Pos2::new(c.x - 3.5, c.y - 5.0),
                        Pos2::new(c.x + 5.0, c.y),
                        Pos2::new(c.x - 3.5, c.y + 5.0),
                    ],
                    color,
                    Stroke::NONE,
                ));
            }
        }
        // 4: Next Keyframe |>
        4 => {
            painter.line_segment(
                [
                    Pos2::new(c.x - 4.0, c.y - 4.5),
                    Pos2::new(c.x - 4.0, c.y + 4.5),
                ],
                stroke,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(c.x - 1.5, c.y - 4.5),
                    Pos2::new(c.x + 4.5, c.y),
                    Pos2::new(c.x - 1.5, c.y + 4.5),
                ],
                color,
                Stroke::NONE,
            ));
        }
        // 5: Skip to End >|
        5 => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(c.x - 4.5, c.y - 4.5),
                    Pos2::new(c.x + 3.0, c.y),
                    Pos2::new(c.x - 4.5, c.y + 4.5),
                ],
                color,
                Stroke::NONE,
            ));
            painter.line_segment(
                [
                    Pos2::new(c.x + 4.5, c.y - 4.5),
                    Pos2::new(c.x + 4.5, c.y + 4.5),
                ],
                stroke,
            );
        }
        // 6: Loop arrows
        6 => {
            let r = Rect::from_center_size(c, Vec2::new(10.0, 7.0));
            painter.rect_stroke(
                r,
                CornerRadius::same(2),
                Stroke::new(1.4, color),
                egui::StrokeKind::Middle,
            );
            // Small arrowhead on top-right
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(r.max.x + 0.5, r.min.y),
                    Pos2::new(r.max.x - 2.5, r.min.y - 2.2),
                    Pos2::new(r.max.x - 2.5, r.min.y + 2.2),
                ],
                color,
                Stroke::NONE,
            ));
        }
        _ => {}
    }
}

fn draw_time_display(
    ui: &mut egui::Ui,
    rect: Rect,
    state: &mut EditorState,
    theme: &AceTheme,
) -> ControlPanelAction {
    let mut action = ControlPanelAction::None;

    let box_w = 40.0;
    let sep_w = 12.0;
    let start_x = rect.min.x + ((rect.width() - (box_w * 2.0 + sep_w)) * 0.5).max(2.0);
    let cur_rect = Rect::from_min_size(
        Pos2::new(start_x, rect.min.y + 2.0),
        Vec2::new(box_w, rect.height() - 4.0),
    );
    let sep_rect = Rect::from_min_size(
        Pos2::new(cur_rect.max.x, rect.min.y),
        Vec2::new(sep_w, rect.height()),
    );
    let end_rect = Rect::from_min_size(
        Pos2::new(sep_rect.max.x, rect.min.y + 2.0),
        Vec2::new(box_w, rect.height() - 4.0),
    );

    let fps = state.frame_rate.max(1);
    let unit = state.settings.timeline_unit;
    let anim_end = state
        .animation_data
        .as_ref()
        .map(|d| d.metadata.end_tick)
        .unwrap_or(0);
    let editing_len = state.playback.editing_length.max(anim_end);

    // Current playhead TextEdit
    let cur_id = ui.id().with("time_display_cur");
    let formatted_cur = format_time(state.playback.playhead, fps, unit);
    if let Some(submitted) = draw_compact_time_edit(ui, cur_rect, cur_id, &formatted_cur, theme) {
        if let Some(parsed) = parse_time_input(&submitted, fps, unit) {
            let max_tick = editing_len.max(anim_end).max(0);
            let target = if max_tick > 0 {
                parsed.clamp(0, max_tick)
            } else {
                parsed
            };
            set_playhead(state, target);
            action = ControlPanelAction::PlayheadChanged;
        }
    }

    // "/" separator
    ui.painter().text(
        sep_rect.center(),
        Align2::CENTER_CENTER,
        "/",
        FontId::proportional(theme.font_size_timeline - 1.0),
        theme.dimmed_text,
    );

    // Editing length TextEdit
    let end_id = ui.id().with("time_display_end");
    let formatted_end = format_time(editing_len, fps, unit);
    if let Some(submitted) = draw_compact_time_edit(ui, end_rect, end_id, &formatted_end, theme) {
        if let Some(parsed) = parse_time_input(&submitted, fps, unit) {
            let clamped = parsed.clamp(MIN_TIMELINE_RANGE, MAX_ANIMATION_LENGTH);
            state.playback.editing_length = clamped;
            action = ControlPanelAction::EditingLengthChanged(clamped);
        }
    }

    action
}

fn draw_compact_time_edit(
    ui: &mut egui::Ui,
    rect: Rect,
    id: Id,
    display_value: &str,
    theme: &AceTheme,
) -> Option<String> {
    let buf_id = id.with("buf");
    let is_focused = ui.memory(|m| m.has_focus(id));
    let mut text = if is_focused {
        ui.data_mut(|d| {
            d.get_temp::<String>(buf_id)
                .unwrap_or_else(|| display_value.to_string())
        })
    } else {
        display_value.to_string()
    };

    let resp = ui.put(
        rect,
        egui::TextEdit::singleline(&mut text)
            .id(id)
            .font(FontId::proportional(theme.font_size_timeline - 2.0))
            .horizontal_align(egui::Align::Center)
            .text_color(theme.text)
            .margin(egui::Margin::symmetric(2, 1)),
    );

    if resp.gained_focus() || resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(buf_id, text.clone()));
    }

    let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if enter_pressed || resp.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(buf_id));
        if text != display_value {
            return Some(text);
        }
    }
    None
}

//! Timeline ruler, tick-interval math, scrubber, scale controls, and
//! horizontal `ZoomBar` derived from `Components/TimelineContainer`,
//! `Components/Timeline/*`, `Components/ScaleControls/*`, and
//! `Components/ZoomBar`.

use egui::{Align2, CornerRadius, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use crate::anim_editor::constants::{
    DEFAULT_KEYFRAME_NAME, LENGTH_PADDING, MAX_ANIMATION_LENGTH, MIN_TIMELINE_RANGE, NUM_TICKS,
    SCALE_CONTROLS_PADDING, SCALE_HANDLE_BAR_WIDTH, SCALE_HANDLE_HIT_WIDTH, SCROLL_BAR_PADDING,
    SCROLL_BAR_SIZE, SNAP_TO_KEYFRAME_THRESHOLD, TICK_FREQUENCY, TICK_SPACING, TIME_TAG_SIZE,
    TRACK_PADDING_LARGE, TRACK_PADDING_MEDIUM, TRACK_PADDING_SMALL, ZOOM_INCREMENT,
};
use crate::anim_editor::model::{nearest_frame, nearest_tick, Tick};
use crate::anim_editor::playback::{format_time, set_playhead};
use crate::anim_editor::state::{
    selection_bounds, ActiveDialog, DragMode, EditorMode, EditorState, SnapMode, TimelineUnit,
};
use crate::anim_editor::theme::AceTheme;

/// Coordinate-mapping geometry for the timeline and dopesheet horizontal axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineMetrics {
    pub rect: Rect,
    pub start_tick: Tick,
    pub end_tick: Tick,
    pub left_pad: f32,
    pub right_pad: f32,
}

impl TimelineMetrics {
    /// Exact `TrackEditor:getTrackPadding`: returns total horizontal padding
    /// (`72`, `84`, or `96` px) based on maximum frame count.
    pub fn dynamic_padding(max_frames: f32) -> f32 {
        if max_frames < 100.0 {
            TRACK_PADDING_SMALL
        } else if max_frames < 1000.0 {
            TRACK_PADDING_MEDIUM
        } else {
            TRACK_PADDING_LARGE
        }
    }

    /// Computes visible tick range and dynamic padding from `EditorState`
    /// matching `TrackUtils.getZoomRange` and `TrackEditor:getTrackPadding`.
    pub fn from_state(rect: Rect, state: &EditorState) -> Self {
        let anim_start = state
            .animation_data
            .as_ref()
            .map(|d| d.metadata.start_tick)
            .unwrap_or(0);
        let anim_end = state
            .animation_data
            .as_ref()
            .map(|d| d.metadata.end_tick)
            .unwrap_or(0)
            .max(state.playback.editing_length)
            .max(MIN_TIMELINE_RANGE);

        let length = (anim_end - anim_start).max(1) as f64;
        let padded_len = (length * (LENGTH_PADDING as f64)).min(MAX_ANIMATION_LENGTH as f64);

        let zoom = state.curve_editor.view.horizontal_zoom.clamp(0.0, 1.0) as f64;
        let scroll = state.curve_editor.view.horizontal_scroll.clamp(0.0, 1.0) as f64;

        let visible_range = (padded_len * (1.0 - zoom)).max(1.0);
        let range_start = (anim_start as f64) + (padded_len - visible_range) * scroll;
        let range_end = range_start + visible_range;

        let fps = state.frame_rate.max(1) as f32;
        let max_frames = (anim_end as f32) * fps / (TICK_FREQUENCY as f32);
        let total_pad = Self::dynamic_padding(max_frames).min((rect.width() * 0.4).max(0.0));

        Self {
            rect,
            start_tick: nearest_tick(range_start),
            end_tick: nearest_tick(range_end).max(nearest_tick(range_start) + 1),
            left_pad: total_pad * 0.5,
            right_pad: total_pad * 0.5,
        }
    }

    #[inline]
    pub fn usable_left(&self) -> f32 {
        self.rect.min.x + self.left_pad
    }

    #[inline]
    pub fn usable_width(&self) -> f32 {
        (self.rect.width() - self.left_pad - self.right_pad).max(1.0)
    }

    #[inline]
    pub fn px_per_tick(&self) -> f32 {
        self.usable_width() / ((self.end_tick - self.start_tick).max(1) as f32)
    }

    /// Exact `TrackUtils.getScaledKeyframePosition` offset by `usable_left()`.
    #[inline]
    pub fn tick_to_x(&self, tick: Tick) -> f32 {
        let span = (self.end_tick - self.start_tick).max(1) as f64;
        let dx = (((tick - self.start_tick) as f64) * (self.usable_width() as f64) / span).floor();
        self.usable_left() + (dx as f32)
    }

    /// Exact `TrackUtils.getKeyframeFromPosition`.
    #[inline]
    pub fn x_to_tick(&self, x: f32) -> Tick {
        let ppt = self.px_per_tick() as f64;
        let raw = (self.start_tick as f64) + ((x - self.usable_left()) as f64) / ppt;
        nearest_tick(raw)
    }
}

/// Computes `(major_frames, minor_frames)` using the exact `10^digits` /
/// `3 * 10^digits` algorithm from `Components/TimelineContainer`,
/// clamped to `[1, 300]`.
pub fn compute_frame_intervals(
    start_tick: Tick,
    end_tick: Tick,
    width_px: f32,
    frame_rate: u32,
) -> (i32, i32) {
    let fps = frame_rate.max(1) as f64;
    let freq = TICK_FREQUENCY as f64;
    let start_frame = (start_tick as f64) * fps / freq;
    let end_frame = (end_tick as f64) * fps / freq;
    let range = (end_frame - start_frame).max(1.0);
    let width = width_px.max(1.0) as f64;

    let units_per_spacing = (TICK_SPACING as f64) * range / width;
    let mut digits: i32 = -1;
    let mut tmp = units_per_spacing;
    while tmp > 1.0 {
        digits += 1;
        tmp /= 10.0;
    }

    let base = 10f64.powi(digits);
    let (major_f, minor_f) = if units_per_spacing < 3.0 * base {
        (base, (base / (NUM_TICKS as f64)).max(1.0))
    } else {
        (3.0 * base, (3.0 * base / (NUM_TICKS as f64)).max(1.0))
    };

    let major = (major_f.round() as i32).clamp(1, 300);
    let minor = (minor_f.round() as i32).clamp(1, 300);
    (major, minor)
}

/// Exact `TimelineContainer` major & minor tick intervals in ticks.
pub fn compute_tick_intervals(
    start_tick: Tick,
    end_tick: Tick,
    width_px: f32,
    frame_rate: u32,
    _unit: TimelineUnit,
) -> (Tick, Tick) {
    let (major_f, minor_f) = compute_frame_intervals(start_tick, end_tick, width_px, frame_rate);
    let fps = frame_rate.max(1) as f64;
    let freq = TICK_FREQUENCY as f64;
    let major_t = nearest_tick((major_f as f64) * freq / fps).max(1);
    let minor_t = nearest_tick((minor_f as f64) * freq / fps).max(1);
    (major_t, minor_t)
}

/// Renders the timeline ruler header (`TIMELINE_HEIGHT`), major/minor ticks,
/// time labels, named keyframe flags, and click/drag scrubbing.
///
/// Returns `true` if the playhead changed.
pub fn show_timeline_header(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    metrics: &TimelineMetrics,
    theme: &AceTheme,
) -> bool {
    let rect = metrics.rect;
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return false;
    }
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter().with_clip_rect(rect);

    // Background
    painter.rect_filled(rect, CornerRadius::ZERO, theme.timeline_bg);

    // Active animation bar [0, anim_end]
    let anim_end = state
        .animation_data
        .as_ref()
        .map(|d| d.metadata.end_tick)
        .unwrap_or(0);
    let (rx_min, rx_max) = (rect.min.x.min(rect.max.x), rect.min.x.max(rect.max.x));
    let bar_left = metrics.tick_to_x(0).clamp(rx_min, rx_max);
    let bar_right = metrics.tick_to_x(anim_end).clamp(rx_min, rx_max);
    if bar_right > bar_left {
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(bar_left, rect.min.y),
                Pos2::new(bar_right, rect.max.y),
            ),
            CornerRadius::ZERO,
            theme.timeline_bar,
        );
    }

    // 2px boundary bars at tick 0 and anim_end
    for bound_tick in [0, anim_end] {
        if bound_tick >= metrics.start_tick && bound_tick <= metrics.end_tick {
            let bx = metrics.tick_to_x(bound_tick);
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(bx - 1.0, rect.min.y),
                    Vec2::new(2.0, rect.height()),
                ),
                CornerRadius::ZERO,
                theme.timeline_dimmed,
            );
        }
    }

    // Major & minor ticks in frame space
    let fps = state.frame_rate.max(1);
    let freq = TICK_FREQUENCY as f64;
    let (major_frames, minor_frames) = compute_frame_intervals(
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

    let unit = state.settings.timeline_unit;
    let mut frame = first_frame;
    while frame <= end_frame {
        let tick = nearest_tick((frame as f64) * freq / (fps as f64));
        let x = metrics.tick_to_x(tick);
        let is_major = frame.rem_euclid(major_frames) == 0;
        let h_scale = if is_major { 0.7 } else { 0.3 };
        let tick_top = rect.max.y - rect.height() * h_scale;

        painter.line_segment(
            [Pos2::new(x, tick_top), Pos2::new(x, rect.max.y)],
            Stroke::new(1.0, theme.timeline_line),
        );

        if is_major {
            let label = format_time(tick, fps, unit);
            painter.text(
                Pos2::new(x + 4.0, rect.min.y + 2.0),
                Align2::LEFT_TOP,
                label,
                FontId::proportional(theme.font_size_timeline - 2.0),
                theme.timeline_text,
            );
        }

        frame += minor_frames;
    }

    // Named keyframe flags
    if let Some(anim) = state.animation_data.as_ref() {
        for (&tick, name) in &anim.events.named_keyframes {
            if tick < metrics.start_tick || tick > metrics.end_tick {
                continue;
            }
            let x = metrics.tick_to_x(tick);
            let flag = vec![
                Pos2::new(x, rect.max.y - 9.0),
                Pos2::new(x + 6.0, rect.max.y - 6.0),
                Pos2::new(x, rect.max.y - 3.0),
            ];
            painter.add(egui::Shape::convex_polygon(
                flag,
                theme.accent,
                Stroke::NONE,
            ));
            let flag_rect =
                Rect::from_center_size(Pos2::new(x + 2.0, rect.max.y - 6.0), Vec2::new(10.0, 10.0));
            ui.interact(flag_rect, ui.id().with(("named_kf", tick)), Sense::hover())
                .on_hover_text(name);
        }
    }

    // Bottom separator
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, theme.border),
    );

    // Click / drag scrubbing
    let mut playhead_changed = false;
    if (resp.clicked() || resp.dragged()) && !ui.input(|i| i.pointer.secondary_down()) {
        if let Some(pos) = resp.interact_pointer_pos() {
            let raw = metrics
                .x_to_tick(pos.x)
                .clamp(metrics.start_tick, metrics.end_tick);
            let snapped = snap_scrubber_tick(state, raw, metrics);
            if snapped != state.playback.playhead {
                set_playhead(state, snapped);
                playhead_changed = true;
            }
        }
    }

    // Right-click context menu: Rename Keyframe
    resp.context_menu(|ui| {
        let click_tick = ui
            .input(|i| i.pointer.interact_pos())
            .map(|p| nearest_frame(metrics.x_to_tick(p.x), fps))
            .unwrap_or(state.playback.playhead);
        if ui.button("Rename Keyframe...").clicked() {
            let existing = state
                .animation_data
                .as_ref()
                .and_then(|d| d.events.named_keyframes.get(&click_tick))
                .cloned()
                .unwrap_or_else(|| DEFAULT_KEYFRAME_NAME.to_string());
            state.active_dialog = ActiveDialog::RenameKeyframe {
                tick: click_tick,
                name: existing,
            };
            ui.close();
        }
    });

    playhead_changed
}

/// Applies `SnapMode` (`Keyframes` within 30px, `Frames`, or `None`) for
/// timeline scrubbing (`Thunks/SnapToNearestKeyframe`).
fn snap_scrubber_tick(state: &EditorState, raw_tick: Tick, metrics: &TimelineMetrics) -> Tick {
    let fps = state.frame_rate.max(1);
    match state.settings.snap_mode {
        SnapMode::None => raw_tick,
        SnapMode::Frames => nearest_frame(raw_tick, fps),
        SnapMode::Keyframes => {
            if let Some(anim) = state.animation_data.as_ref() {
                let target_x = metrics.tick_to_x(raw_tick);
                let mut best_tick: Option<Tick> = None;
                let mut best_dist = i32::MAX;

                let mut consider_tick = |t: Tick| {
                    let d = (t - raw_tick).abs();
                    if d < best_dist {
                        best_dist = d;
                        best_tick = Some(t);
                    }
                };

                if state.editor_mode == EditorMode::Curves && !state.selected_tracks.is_empty() {
                    for path in &state.selected_tracks {
                        if let Some(track) = anim.get_track("Root", path) {
                            let mut ticks = std::collections::BTreeSet::new();
                            track.collect_ticks(&mut ticks);
                            for t in ticks {
                                consider_tick(t);
                            }
                        }
                    }
                } else {
                    for t in anim.get_summary_ticks() {
                        consider_tick(t);
                    }
                }

                if let Some(closest) = best_tick {
                    if closest >= metrics.start_tick
                        && closest <= metrics.end_tick
                        && (metrics.tick_to_x(closest) - target_x).abs()
                            < SNAP_TO_KEYFRAME_THRESHOLD
                    {
                        return closest;
                    }
                }
            }
            nearest_frame(raw_tick, fps)
        }
    }
}

/// Paints the scrubber head pentagon/triangle and full-height vertical line
/// (`Components/Timeline/Scrubber`).
pub fn paint_scrubber(
    painter: &egui::Painter,
    full_rect: Rect,
    metrics: &TimelineMetrics,
    playhead: Tick,
    theme: &AceTheme,
) {
    if playhead < metrics.start_tick || playhead > metrics.end_tick {
        return;
    }
    let x = metrics.tick_to_x(playhead);
    if x < full_rect.min.x || x > full_rect.max.x {
        return;
    }

    let top = full_rect.min.y;
    let head_pts = vec![
        Pos2::new(x - 5.0, top),
        Pos2::new(x + 5.0, top),
        Pos2::new(x + 5.0, top + 6.0),
        Pos2::new(x, top + 11.0),
        Pos2::new(x - 5.0, top + 6.0),
    ];
    painter.add(egui::Shape::convex_polygon(
        head_pts,
        theme.scrubber,
        Stroke::NONE,
    ));

    painter.line_segment(
        [Pos2::new(x, top + 10.0), Pos2::new(x, full_rect.max.y)],
        Stroke::new(1.0, theme.scrubber),
    );
}

/// Renders `ScaleControls` (`Components/ScaleControls/*`) when >= 2
/// distinct ticks are selected.
pub fn show_scale_controls(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    metrics: &TimelineMetrics,
    theme: &AceTheme,
    top_y: f32,
    bottom_y: f32,
) {
    if state.curve_editor.is_dragging_marquee {
        return;
    }
    let Some((earliest, latest)) = selection_bounds(&state.selected_keyframes) else {
        return;
    };
    if earliest >= latest {
        return;
    }

    let left_x = metrics.tick_to_x(earliest) - SCALE_CONTROLS_PADDING;
    let right_x = metrics.tick_to_x(latest) + SCALE_CONTROLS_PADDING;
    let height = (bottom_y - top_y).max(1.0);

    let painter = ui.painter();
    let main_color = theme.scale_controls_main;

    // Top and bottom bracket lines connecting left and right handles
    painter.line_segment(
        [Pos2::new(left_x, top_y), Pos2::new(right_x, top_y)],
        Stroke::new(1.0, main_color),
    );
    painter.line_segment(
        [Pos2::new(left_x, bottom_y), Pos2::new(right_x, bottom_y)],
        Stroke::new(1.0, main_color),
    );

    let mut is_dragging = false;

    // Left handle (pivots on `latest`) and Right handle (pivots on `earliest`)
    for (is_left, hx, pivot_tick) in [(true, left_x, latest), (false, right_x, earliest)] {
        let hit_rect = Rect::from_min_size(
            Pos2::new(hx - SCALE_HANDLE_HIT_WIDTH * 0.5, top_y),
            Vec2::new(SCALE_HANDLE_HIT_WIDTH, height),
        );
        let bar_rect = Rect::from_min_size(
            Pos2::new(hx - SCALE_HANDLE_BAR_WIDTH * 0.5, top_y),
            Vec2::new(SCALE_HANDLE_BAR_WIDTH, height),
        );
        painter.rect_filled(bar_rect, CornerRadius::ZERO, main_color);

        let resp = ui
            .interact(
                hit_rect,
                ui.id()
                    .with(if is_left { "scale_left" } else { "scale_right" }),
                Sense::drag(),
            )
            .on_hover_cursor(CursorIcon::ResizeHorizontal);

        if resp.drag_started() {
            crate::anim_editor::ops::begin_keyframe_drag(
                state,
                pivot_tick,
                None,
                DragMode::Keyframe,
            );
        }
        if resp.dragged() {
            is_dragging = true;
            if let Some(pos) = resp.interact_pointer_pos() {
                let raw_tick = metrics.x_to_tick(pos.x);
                let snapped = if state.settings.snap_mode != SnapMode::None {
                    nearest_frame(raw_tick, state.frame_rate.max(1))
                } else {
                    raw_tick
                };
                crate::anim_editor::ops::update_keyframe_scale(state, snapped);
            }
        }
        if resp.drag_stopped() {
            crate::anim_editor::ops::end_drag(state, true);
        }
    }

    // TimeTag pills rendered while dragging
    if is_dragging {
        let fps = state.frame_rate.max(1);
        let unit = state.settings.timeline_unit;
        for (hx, tick) in [(left_x, earliest), (right_x, latest)] {
            let tag_rect = Rect::from_min_size(
                Pos2::new(hx - TIME_TAG_SIZE.0 * 0.5, top_y - TIME_TAG_SIZE.1 - 2.0),
                Vec2::new(TIME_TAG_SIZE.0, TIME_TAG_SIZE.1),
            );
            painter.rect_filled(tag_rect, CornerRadius::same(2), main_color);
            painter.text(
                tag_rect.center(),
                Align2::CENTER_CENTER,
                format_time(tick, fps, unit),
                FontId::proportional(theme.font_size_timeline - 2.0),
                theme.scale_controls_text,
            );
        }
    }
}

/// Renders the horizontal `ZoomBar` (`Components/ZoomBar`), mapping
/// `[min_val, max_val] ⊆ [0, 1]` <-> `(horizontal_scroll, horizontal_zoom)`.
pub fn show_zoom_bar(ui: &mut egui::Ui, state: &mut EditorState, theme: &AceTheme) {
    let width = ui.available_width().max(SCROLL_BAR_SIZE * 5.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, SCROLL_BAR_SIZE), Sense::hover());
    let painter = ui.painter();

    painter.rect_filled(rect, CornerRadius::ZERO, theme.scrollbar_bg);
    painter.line_segment(
        [rect.left_top(), rect.right_top()],
        Stroke::new(1.0, theme.border),
    );

    let scroll = state.curve_editor.view.horizontal_scroll.clamp(0.0, 1.0);
    let zoom = state.curve_editor.view.horizontal_zoom.clamp(0.0, 1.0);
    let mut min_val = (scroll * zoom).clamp(0.0, 1.0);
    let mut max_val = ((1.0 - zoom) + scroll * zoom).clamp(min_val, 1.0);
    let mut changed = false;

    // Outer left/right arrow buttons
    let left_arrow_rect = Rect::from_min_size(rect.min, Vec2::splat(SCROLL_BAR_SIZE));
    let right_arrow_rect = Rect::from_min_size(
        Pos2::new(rect.max.x - SCROLL_BAR_SIZE, rect.min.y),
        Vec2::splat(SCROLL_BAR_SIZE),
    );

    let l_resp = ui.interact(
        left_arrow_rect,
        ui.id().with("zoombar_left"),
        Sense::click(),
    );
    let r_resp = ui.interact(
        right_arrow_rect,
        ui.id().with("zoombar_right"),
        Sense::click(),
    );
    draw_arrow_button(painter, left_arrow_rect, true, l_resp.hovered(), theme);
    draw_arrow_button(painter, right_arrow_rect, false, r_resp.hovered(), theme);

    let bar_len = (max_val - min_val).max(0.01);
    if l_resp.clicked() {
        min_val = (min_val - ZOOM_INCREMENT).max(0.0);
        max_val = (min_val + bar_len).min(1.0);
        changed = true;
    }
    if r_resp.clicked() {
        max_val = (max_val + ZOOM_INCREMENT).min(1.0);
        min_val = (max_val - bar_len).max(0.0);
        changed = true;
    }

    // Track between arrows
    let track_left = left_arrow_rect.max.x + SCROLL_BAR_PADDING;
    let track_right = right_arrow_rect.min.x - SCROLL_BAR_PADDING;
    let grip_w = SCROLL_BAR_SIZE;
    let draggable_w = (track_right - track_left - 2.0 * grip_w).max(1.0);

    let thumb_left = track_left + grip_w + min_val * draggable_w;
    let thumb_right = track_left + grip_w + max_val * draggable_w;

    let left_grip_rect = Rect::from_min_max(
        Pos2::new(thumb_left - grip_w, rect.min.y + 2.0),
        Pos2::new(thumb_left, rect.max.y - 2.0),
    );
    let middle_rect = Rect::from_min_max(
        Pos2::new(thumb_left, rect.min.y + 2.0),
        Pos2::new(thumb_right.max(thumb_left + 4.0), rect.max.y - 2.0),
    );
    let right_grip_rect = Rect::from_min_max(
        Pos2::new(middle_rect.max.x, rect.min.y + 2.0),
        Pos2::new(middle_rect.max.x + grip_w, rect.max.y - 2.0),
    );

    let lg_resp = ui
        .interact(left_grip_rect, ui.id().with("zoombar_lgrip"), Sense::drag())
        .on_hover_cursor(CursorIcon::ResizeHorizontal);
    let mid_resp = ui
        .interact(middle_rect, ui.id().with("zoombar_mid"), Sense::drag())
        .on_hover_cursor(CursorIcon::Grab);
    let rg_resp = ui
        .interact(
            right_grip_rect,
            ui.id().with("zoombar_rgrip"),
            Sense::drag(),
        )
        .on_hover_cursor(CursorIcon::ResizeHorizontal);

    if lg_resp.dragged() {
        let dv = lg_resp.drag_delta().x / draggable_w;
        min_val = (min_val + dv).clamp(0.0, (max_val - 0.01).max(0.0));
        changed = true;
    }
    if rg_resp.dragged() {
        let dv = rg_resp.drag_delta().x / draggable_w;
        max_val = (max_val + dv).clamp((min_val + 0.01).min(1.0), 1.0);
        changed = true;
    }
    if mid_resp.dragged() {
        let dv = mid_resp.drag_delta().x / draggable_w;
        let len = max_val - min_val;
        min_val = (min_val + dv).clamp(0.0, (1.0 - len).max(0.0));
        max_val = (min_val + len).min(1.0);
        changed = true;
    }

    // Paint thumb & grips
    let mid_color = if mid_resp.dragged() {
        theme.scrollbar_pressed
    } else if mid_resp.hovered() {
        theme.scrollbar_hover
    } else {
        theme.scrollbar_control
    };
    painter.rect_filled(middle_rect, CornerRadius::ZERO, mid_color);

    for (g_rect, resp) in [(left_grip_rect, &lg_resp), (right_grip_rect, &rg_resp)] {
        let c = if resp.dragged() {
            theme.scrollbar_pressed
        } else if resp.hovered() {
            theme.scrollbar_hover
        } else {
            theme.scrollbar_control
        };
        painter.rect_filled(g_rect, CornerRadius::same(2), c);
        painter.rect_stroke(
            g_rect,
            CornerRadius::same(2),
            Stroke::new(1.0, theme.border),
            egui::StrokeKind::Middle,
        );
        // Grip vertical notch
        let cx = g_rect.center().x;
        painter.line_segment(
            [
                Pos2::new(cx, g_rect.min.y + 3.0),
                Pos2::new(cx, g_rect.max.y - 3.0),
            ],
            Stroke::new(1.0, theme.dimmed_text),
        );
    }

    if changed {
        let new_zoom = (1.0 - (max_val - min_val)).clamp(0.0, 1.0);
        let new_scroll = if new_zoom > 1e-6 {
            (min_val / new_zoom).clamp(0.0, 1.0)
        } else {
            0.0
        };
        state.curve_editor.view.horizontal_zoom = new_zoom;
        state.curve_editor.view.horizontal_scroll = new_scroll;
    }
}

fn draw_arrow_button(
    painter: &egui::Painter,
    rect: Rect,
    left: bool,
    hovered: bool,
    theme: &AceTheme,
) {
    if hovered {
        painter.rect_filled(rect, CornerRadius::ZERO, theme.scrollbar_hover);
    }
    let c = rect.center();
    let pts = if left {
        vec![
            Pos2::new(c.x - 3.0, c.y),
            Pos2::new(c.x + 2.0, c.y - 3.5),
            Pos2::new(c.x + 2.0, c.y + 3.5),
        ]
    } else {
        vec![
            Pos2::new(c.x + 3.0, c.y),
            Pos2::new(c.x - 2.0, c.y - 3.5),
            Pos2::new(c.x - 2.0, c.y + 3.5),
        ]
    };
    painter.add(egui::Shape::convex_polygon(
        pts,
        theme.scrollbar_arrow,
        Stroke::NONE,
    ));
}

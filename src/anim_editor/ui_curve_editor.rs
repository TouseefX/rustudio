//! 1:1 Curve Editor UI (`Components/Curves/*`: `CurveEditorController`,
//! `CurveCanvas`, `Cubic`, `Line`, `Keyframe`, `KeyframeActions`,
//! `TangentControl`, `TangentActions`, `Scale`, `ScaleTick`).

use std::collections::{BTreeMap, BTreeSet};

use egui::{
    Align2, Color32, CursorIcon, FontId, Id, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};

use crate::anim_editor::constants::{
    CANVAS_ZOOM_INCREMENT, CURVE_CANVAS_PADDING, CURVE_INTERVAL, CURVE_WIDTH, CURVE_WIDTH_SELECTED,
    EASING_DIRECTION_ORDER, KEYFRAME_BORDER_SIZE, KEYFRAME_BORDER_SIZE_SELECTED, KEYFRAME_WIDTH,
    SCALE_NUM_TICKS, SCALE_TICK_SPACING, SCRUBBER_MARKER_WIDTH, TANGENT_CONTROL_LENGTH,
    TANGENT_CONTROL_SIZE, TANGENT_CONTROL_WIDTH, TICK_FREQUENCY, ZOOM_INCREMENT,
};
use crate::anim_editor::curves::{evaluate_number_track, get_slopes, get_y_extents};
use crate::anim_editor::model::{
    clamp_tick, nearest_frame, nearest_tick, path_to_string, EasingDirection, EasingStyle,
    KeyInterpolationMode, KeyframeValue, Tick, Track, TrackPath, TrackType,
};
use crate::anim_editor::ops::set_keyframe_tangent;
use crate::anim_editor::state::{
    has_multiple_selected_ticks, is_selection_empty, DragContext, EditorState, SelectedKeyframes,
    SelectionNode, SnapMode, TangentSide,
};
use crate::anim_editor::theme::AceTheme;
use crate::anim_editor::ui_timeline::TimelineMetrics;

/// A flattened leaf curve track ready to be rendered in the Curve Editor.
struct VisibleCurveTrack<'a> {
    instance_name: String,
    path: TrackPath,
    #[allow(dead_code)]
    leaf_name: String,
    track: &'a Track,
    color: Color32,
}

/// Screen-space <-> Curve-space coordinate mapping matching
/// `CurveCanvas:toCanvasSpace`, `CurveEditorController.toCurveSpace`, and `CurveCanvas:scaleSlope`.
#[derive(Debug, Clone, Copy)]
struct CurveTransform {
    canvas_rect: Rect,
    start_tick: f32,
    end_tick: f32,
    min_value: f32,
    max_value: f32,
    vertical_scroll: f32,
    vertical_zoom: f32,
}

impl CurveTransform {
    fn new(canvas_rect: Rect, metrics: &TimelineMetrics, state: &EditorState) -> Self {
        let view = state.curve_editor.view;
        let start_tick = metrics.start_tick as f32;
        let end_tick = (metrics.end_tick.max(metrics.start_tick + 1)) as f32;
        let min_value = view.min_value;
        let max_value = if (view.max_value - view.min_value).abs() < 1e-5 {
            view.min_value + 1.0
        } else {
            view.max_value
        };
        Self {
            canvas_rect,
            start_tick,
            end_tick,
            min_value,
            max_value,
            vertical_scroll: view.vertical_scroll,
            vertical_zoom: view.vertical_zoom.min(0.99),
        }
    }

    #[inline]
    fn zoom_factor(&self) -> f32 {
        1.0 / (1.0 - self.vertical_zoom)
    }

    #[inline]
    fn active_height(&self) -> f32 {
        (self.canvas_rect.height() * (1.0 - 2.0 * CURVE_CANVAS_PADDING)).max(1.0)
    }

    #[inline]
    fn tick_to_x(&self, tick: f32) -> f32 {
        let span = (self.end_tick - self.start_tick).max(1.0);
        self.canvas_rect.min.x + ((tick - self.start_tick) / span) * self.canvas_rect.width()
    }

    #[inline]
    fn value_to_y(&self, value: f32) -> f32 {
        let val_span = (self.max_value - self.min_value).max(1e-5);
        let y_norm = ((self.max_value - value) / val_span
            - self.vertical_scroll * self.vertical_zoom)
            * self.zoom_factor();
        self.canvas_rect.min.y
            + self.canvas_rect.height() * CURVE_CANVAS_PADDING
            + y_norm * self.active_height()
    }

    #[inline]
    fn to_canvas(&self, tick: f32, value: f32) -> Pos2 {
        Pos2::new(self.tick_to_x(tick), self.value_to_y(value))
    }

    #[inline]
    fn x_to_tick(&self, x: f32) -> f32 {
        let span = (self.end_tick - self.start_tick).max(1.0);
        let width = self.canvas_rect.width().max(1.0);
        self.start_tick + ((x - self.canvas_rect.min.x) / width) * span
    }

    #[inline]
    fn y_to_value(&self, y: f32) -> f32 {
        let val_span = self.max_value - self.min_value;
        let ly = y - self.canvas_rect.min.y;
        let y_norm = (ly - self.canvas_rect.height() * CURVE_CANVAS_PADDING)
            / (self.active_height() * self.zoom_factor());
        self.max_value - val_span * (self.vertical_scroll * self.vertical_zoom + y_norm)
    }

    #[inline]
    fn to_curve(&self, pos: Pos2) -> (f32, f32) {
        (self.x_to_tick(pos.x), self.y_to_value(pos.y))
    }

    /// Converts curve-space slope (`dv / dt`) to screen-space slope (`dpy / dpx`).
    #[inline]
    fn scale_slope(&self, curve_slope: f32) -> f32 {
        let tick_per_px = (self.end_tick - self.start_tick) / self.canvas_rect.width().max(1.0);
        let val_per_px =
            (self.min_value - self.max_value) * (1.0 - self.vertical_zoom) / self.active_height();
        if val_per_px.abs() < 1e-9 {
            0.0
        } else {
            curve_slope * (tick_per_px / val_per_px)
        }
    }
}

/// Fits the Curve Editor vertical extents analytically to all visible tracks
/// using `curves::get_y_extents` (`CurveEditorController:updateCanvasExtents`).
pub fn fit_curve_view(state: &mut EditorState) {
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    if let Some(anim) = &state.animation_data {
        let tracks = collect_visible_tracks(anim, &state.selected_tracks, &AceTheme::current());
        for vt in &tracks {
            let track = vt.track;
            if track.keyframes.is_empty() {
                continue;
            }
            if track.track_type == TrackType::Quaternion {
                min_y = min_y.min(0.0);
                max_y = max_y.max(1.0);
                continue;
            }
            if track.keyframes.len() == 1 {
                if let Some(kf) = track.data.get(&track.keyframes[0]) {
                    if let Some(v) = kf.value.as_f32() {
                        let v = if track.track_type == TrackType::Facs {
                            v.clamp(0.0, 1.0)
                        } else {
                            v
                        };
                        min_y = min_y.min(v);
                        max_y = max_y.max(v);
                    }
                }
                continue;
            }
            for pair in track.keyframes.windows(2) {
                let t0 = pair[0];
                let t1 = pair[1];
                let (Some(k0), Some(k1)) = (track.data.get(&t0), track.data.get(&t1)) else {
                    continue;
                };
                let (Some(y0), Some(y1)) = (k0.value.as_f32(), k1.value.as_f32()) else {
                    continue;
                };
                let mode = k0.interpolation_mode.unwrap_or(KeyInterpolationMode::Cubic);
                let (seg_min, seg_max) = if mode == KeyInterpolationMode::Cubic {
                    let (_, s0) = get_slopes(track, t0);
                    let (s1, _) = get_slopes(track, t1);
                    get_y_extents(
                        t0 as f32,
                        y0,
                        s0.unwrap_or(0.0),
                        t1 as f32,
                        y1,
                        s1.unwrap_or(0.0),
                    )
                } else {
                    (y0.min(y1), y0.max(y1))
                };
                let (seg_min, seg_max) = if track.track_type == TrackType::Facs {
                    (seg_min.clamp(0.0, 1.0), seg_max.clamp(0.0, 1.0))
                } else {
                    (seg_min, seg_max)
                };
                min_y = min_y.min(seg_min);
                max_y = max_y.max(seg_max);
            }
        }
    }

    if !min_y.is_finite() || !max_y.is_finite() {
        min_y = 0.0;
        max_y = 1.0;
    } else if (max_y - min_y).abs() < 1e-5 {
        min_y -= 0.5;
        max_y += 0.5;
    }

    state.curve_editor.view.min_value = min_y;
    state.curve_editor.view.max_value = max_y;
    state.curve_editor.view.vertical_scroll = 0.0;
    state.curve_editor.view.vertical_zoom = 0.0;
}

/// Main Curve Editor view (`CurveEditorController` + `CurveCanvas` + `Scale`).
/// Returns `true` if any keyframe or tangent was modified this frame.
pub fn show_curve_editor(
    ui: &mut Ui,
    state: &mut EditorState,
    metrics: &TimelineMetrics,
    theme: &AceTheme,
) -> bool {
    let full_rect = ui.available_rect_before_wrap();
    if full_rect.width() <= 4.0 || full_rect.height() <= 4.0 {
        return false;
    }

    let scale_width = metrics.left_pad.max(36.0).min(full_rect.width() * 0.25);
    let left_scale_rect = Rect::from_min_max(
        full_rect.min,
        Pos2::new(full_rect.min.x + scale_width, full_rect.max.y),
    );
    let right_scale_rect = Rect::from_min_max(
        Pos2::new(full_rect.max.x - scale_width, full_rect.min.y),
        full_rect.max,
    );
    let canvas_rect = Rect::from_min_max(
        Pos2::new(left_scale_rect.max.x, full_rect.min.y),
        Pos2::new(right_scale_rect.min.x, full_rect.max.y),
    );

    let painter = ui.painter_at(full_rect);
    painter.rect_filled(full_rect, 0.0, theme.timeline_bg);

    // Reserve interaction area for the entire curve editor.
    let canvas_resp = ui.interact(
        canvas_rect,
        ui.id().with("curve_canvas"),
        Sense::click_and_drag(),
    );
    let left_scale_resp = ui.interact(
        left_scale_rect,
        ui.id().with("curve_left_scale"),
        Sense::hover(),
    );
    let right_scale_resp = ui.interact(
        right_scale_rect,
        ui.id().with("curve_right_scale"),
        Sense::hover(),
    );

    // Handle mouse wheel zoom (Ctrl+Wheel = horizontal zoom; Alt+Wheel or plain wheel on canvas/scales = vertical zoom).
    handle_wheel_zoom(
        ui,
        state,
        canvas_rect,
        metrics,
        canvas_resp.hovered(),
        left_scale_resp.hovered() || right_scale_resp.hovered(),
    );

    // Handle middle-mouse 2D pan.
    handle_middle_mouse_pan(ui, state, canvas_rect, &canvas_resp);

    let xform = CurveTransform::new(canvas_rect, metrics, state);

    // Collect visible tracks to check whether Angle / Number / Facs tracks are present.
    let (has_angle_tracks, has_only_angle_tracks, has_only_facs_tracks) =
        if let Some(anim) = &state.animation_data {
            let vts = collect_visible_tracks(anim, &state.selected_tracks, theme);
            let any_angle = vts.iter().any(|t| t.track.track_type == TrackType::Angle);
            let only_angle =
                !vts.is_empty() && vts.iter().all(|t| t.track.track_type == TrackType::Angle);
            let only_facs =
                !vts.is_empty() && vts.iter().all(|t| t.track.track_type == TrackType::Facs);
            (any_angle, only_angle, only_facs)
        } else {
            (false, false, false)
        };

    // Draw Left Y-axis Scale + horizontal gridlines across canvas.
    draw_scale_strip(
        &painter,
        left_scale_rect,
        canvas_rect,
        &xform,
        if has_only_angle_tracks {
            ScaleDisplayKind::Angle
        } else if has_only_facs_tracks {
            ScaleDisplayKind::Facs
        } else {
            ScaleDisplayKind::Number
        },
        true,
        theme,
    );

    // Draw Right Y-axis Angle Scale when angle tracks are visible alongside non-angle tracks.
    if has_angle_tracks && !has_only_angle_tracks {
        draw_scale_strip(
            &painter,
            right_scale_rect,
            canvas_rect,
            &xform,
            ScaleDisplayKind::Angle,
            false,
            theme,
        );
    } else {
        painter.rect_filled(right_scale_rect, 0.0, theme.background);
        painter.line_segment(
            [right_scale_rect.left_top(), right_scale_rect.left_bottom()],
            Stroke::new(1.0, theme.border),
        );
    }

    // Draw X-axis (y = 0) line inside canvas.
    let canvas_painter = ui.painter_at(canvas_rect);
    let y0 = xform.value_to_y(0.0);
    if y0 >= canvas_rect.min.y && y0 <= canvas_rect.max.y {
        canvas_painter.line_segment(
            [
                Pos2::new(canvas_rect.min.x, y0),
                Pos2::new(canvas_rect.max.x, y0),
            ],
            Stroke::new(1.0, theme.curve_x_axis),
        );
    }

    // Render all visible curves, scrubber markers, tangents, and keyframes,
    // and collect interactive hit targets.
    let mut hit_keyframes: Vec<KeyframeHit> = Vec::new();
    let mut hit_tangents: Vec<TangentHit> = Vec::new();

    if let Some(anim) = &state.animation_data {
        let visible_tracks = collect_visible_tracks(anim, &state.selected_tracks, theme);
        let arm_len = TANGENT_CONTROL_LENGTH * canvas_rect.height();

        for vt in &visible_tracks {
            draw_curve_segments(
                &canvas_painter,
                vt,
                &xform,
                &state.selected_keyframes,
                metrics,
                theme,
            );
            draw_scrubber_marker(&canvas_painter, vt, state.playback.playhead, &xform, theme);
        }

        // Layer 3: Tangent handles for selected keyframes.
        for vt in &visible_tracks {
            collect_and_draw_tangents(
                &canvas_painter,
                vt,
                &xform,
                &state.selected_keyframes,
                arm_len,
                theme,
                &mut hit_tangents,
            );
        }

        // Layer 4: Keyframe diamonds.
        for vt in &visible_tracks {
            collect_and_draw_keyframes(
                &canvas_painter,
                vt,
                &xform,
                &state.selected_keyframes,
                theme,
                &mut hit_keyframes,
            );
        }
    }

    // Tooltip on hovered keyframe.
    if let Some(pointer_pos) = ui.ctx().pointer_hover_pos() {
        if canvas_rect.contains(pointer_pos) && state.drag_context.is_none() {
            if let Some(hit) = find_keyframe_hit(&hit_keyframes, pointer_pos) {
                if let Some(anim) = &state.animation_data {
                    if let Some(track) = anim.get_track(&hit.instance_name, &hit.path) {
                        let tooltip = build_keyframe_tooltip(&hit.path, track, hit.tick);
                        egui::Tooltip::always_open(
                            ui.ctx().clone(),
                            ui.layer_id(),
                            Id::new("curve_kf_tooltip"),
                            egui::PopupAnchor::Pointer,
                        )
                        .show(|ui| {
                            ui.label(tooltip);
                        });
                    }
                }
            } else if find_tangent_hit(&hit_tangents, pointer_pos).is_some() {
                ui.ctx().set_cursor_icon(CursorIcon::Grab);
            }
        }
    }

    // Pointer interaction: dragging tangents, dragging keyframes, marquee selection, context menus.
    let modified = handle_canvas_pointer_interactions(
        ui,
        state,
        &xform,
        &canvas_resp,
        &hit_keyframes,
        &hit_tangents,
    );

    // Draw marquee selection box if active.
    if state.curve_editor.is_dragging_marquee {
        if let (Some((sx, sy)), Some((ex, ey))) = (
            state.curve_editor.marquee_start,
            state.curve_editor.marquee_end,
        ) {
            let mrect = Rect::from_two_pos(Pos2::new(sx, sy), Pos2::new(ex, ey));
            canvas_painter.rect_filled(mrect, 0.0, theme.selection_box_fill);
            canvas_painter.rect_stroke(
                mrect,
                0.0,
                Stroke::new(1.0, theme.selection_box_border),
                StrokeKind::Inside,
            );
        }
    }

    // Small "Fit" button in top-right corner of canvas.
    let fit_btn_rect = Rect::from_min_size(
        Pos2::new(canvas_rect.max.x - 40.0, canvas_rect.min.y + 4.0),
        Vec2::new(34.0, 18.0),
    );
    let fit_resp = ui.put(
        fit_btn_rect,
        egui::Button::new(egui::RichText::new("Fit").size(11.0).color(theme.text))
            .fill(theme.scrollbar_control),
    );
    if fit_resp.on_hover_text("Fit Curves to View (F)").clicked() {
        fit_curve_view(state);
    }

    // Context menus (KeyframeActions / TangentActions).
    let ctx_menu_id = ui.id().with("curve_context_menu");
    if canvas_resp.secondary_clicked() {
        if let Some(pos) = canvas_resp.interact_pointer_pos() {
            if let Some(thit) = find_tangent_hit(&hit_tangents, pos) {
                state.right_click_context.instance_name = Some(thit.instance_name.clone());
                state.right_click_context.path = Some(thit.path.clone());
                state.right_click_context.tick = Some(thit.tick);
                state.right_click_context.tangent_side = Some(thit.side);
                state.right_click_context.on_keyframe = false;
                state.menus.show_tangent_menu = true;
                state.menus.show_timeline_menu = false;
            } else {
                state.right_click_context.tangent_side = None;
                state.menus.show_tangent_menu = false;
                if let Some(khit) = find_keyframe_hit(&hit_keyframes, pos) {
                    if !is_keyframe_selected(
                        &state.selected_keyframes,
                        &khit.instance_name,
                        &khit.path,
                        khit.tick,
                    ) {
                        state.selected_keyframes.clear();
                        select_keyframe(
                            &mut state.selected_keyframes,
                            &khit.instance_name,
                            &khit.path,
                            khit.tick,
                        );
                    }
                    state.right_click_context.on_keyframe = true;
                    state.right_click_context.tick = Some(khit.tick);
                    state.right_click_context.instance_name = Some(khit.instance_name.clone());
                    state.right_click_context.path = Some(khit.path.clone());
                } else {
                    state.right_click_context.on_keyframe = false;
                    state.right_click_context.tick =
                        Some(clamp_tick(nearest_tick(xform.x_to_tick(pos.x) as f64)));
                }
                state.menus.show_timeline_menu = true;
            }
            egui::Popup::open_id(ui.ctx(), ctx_menu_id);
        }
    }

    let anchor_pos = ui
        .ctx()
        .pointer_latest_pos()
        .unwrap_or(canvas_rect.center());
    egui::Popup::new(
        ctx_menu_id,
        ui.ctx().clone(),
        egui::PopupAnchor::Position(anchor_pos),
        ui.layer_id(),
    )
    .kind(egui::PopupKind::Menu)
    .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
    .show(|ui| {
        if state.menus.show_tangent_menu {
            show_tangent_actions_menu(ui, state);
        } else {
            show_keyframe_actions_menu(ui, state);
        }
    });

    modified
}

// ============================================================================
// Scale Strip & Gridlines (`Scale` & `ScaleTick`)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScaleDisplayKind {
    Number,
    Angle,
    Facs,
}

fn draw_scale_strip(
    painter: &egui::Painter,
    strip_rect: Rect,
    canvas_rect: Rect,
    xform: &CurveTransform,
    kind: ScaleDisplayKind,
    is_left: bool,
    theme: &AceTheme,
) {
    painter.rect_filled(strip_rect, 0.0, theme.background);

    let border_x = if is_left {
        strip_rect.max.x
    } else {
        strip_rect.min.x
    };
    painter.line_segment(
        [
            Pos2::new(border_x, strip_rect.min.y),
            Pos2::new(border_x, strip_rect.max.y),
        ],
        Stroke::new(1.0, theme.border),
    );

    let (display_min, display_max) = if kind == ScaleDisplayKind::Angle {
        (xform.min_value.to_degrees(), xform.max_value.to_degrees())
    } else {
        (xform.min_value, xform.max_value)
    };

    let zoom = xform.vertical_zoom.min(0.99);
    let range = (display_max - display_min).abs().max(1e-4) * (1.0 - zoom);
    let raw_step = SCALE_TICK_SPACING * range / canvas_rect.height().max(1.0);
    let pow10 = 10.0_f32.powf(raw_step.max(1e-6).log10().floor());
    let five_step = SCALE_NUM_TICKS as f32 * pow10;
    let major_interval = if raw_step < five_step {
        pow10
    } else {
        five_step
    };
    let minor_interval = major_interval / SCALE_NUM_TICKS as f32;

    if major_interval <= 0.0 || !major_interval.is_finite() {
        return;
    }

    let mut vis_min = xform.y_to_value(canvas_rect.max.y);
    let mut vis_max = xform.y_to_value(canvas_rect.min.y);
    if kind == ScaleDisplayKind::Angle {
        vis_min = vis_min.to_degrees();
        vis_max = vis_max.to_degrees();
    }
    if vis_min > vis_max {
        std::mem::swap(&mut vis_min, &mut vis_max);
    }

    let first_major = (vis_min / major_interval).floor() * major_interval;
    let max_steps = 64;
    let mut step_idx = 0;

    let mut val = first_major;
    while val <= vis_max + major_interval && step_idx < max_steps {
        step_idx += 1;
        let curve_val = if kind == ScaleDisplayKind::Angle {
            val.to_radians()
        } else {
            val
        };
        let py = xform.value_to_y(curve_val);

        if py >= strip_rect.min.y - 2.0 && py <= strip_rect.max.y + 2.0 {
            // Major tick mark (width scale 0.7).
            let tick_w = strip_rect.width() * 0.28;
            let (tx0, tx1) = if is_left {
                (strip_rect.max.x - tick_w, strip_rect.max.x)
            } else {
                (strip_rect.min.x, strip_rect.min.x + tick_w)
            };
            painter.line_segment(
                [Pos2::new(tx0, py), Pos2::new(tx1, py)],
                Stroke::new(1.0, theme.timeline_line),
            );

            // Horizontal gridline across canvas (only from left scale to avoid duplicates).
            if is_left {
                painter.line_segment(
                    [
                        Pos2::new(canvas_rect.min.x, py),
                        Pos2::new(canvas_rect.max.x, py),
                    ],
                    Stroke::new(1.0, theme.timeline_grid_line),
                );
            }

            let label = match kind {
                ScaleDisplayKind::Angle => format!("{}°", val.round() as i32),
                ScaleDisplayKind::Facs => format!("{:.2}", val),
                ScaleDisplayKind::Number => {
                    if major_interval >= 1.0 {
                        format!("{:.1}", val)
                    } else {
                        format!("{:.3}", val)
                    }
                }
            };
            let label_pos = if is_left {
                Pos2::new(strip_rect.min.x + 3.0, py)
            } else {
                Pos2::new(strip_rect.max.x - 3.0, py)
            };
            let align = if is_left {
                Align2::LEFT_CENTER
            } else {
                Align2::RIGHT_CENTER
            };
            painter.text(
                label_pos,
                align,
                label,
                FontId::proportional(10.5),
                theme.timeline_text,
            );
        }

        // 4 minor ticks between majors (width scale 0.3).
        for m in 1..SCALE_NUM_TICKS {
            let mval = val + minor_interval * m as f32;
            let mcurve = if kind == ScaleDisplayKind::Angle {
                mval.to_radians()
            } else {
                mval
            };
            let mpy = xform.value_to_y(mcurve);
            if mpy >= strip_rect.min.y && mpy <= strip_rect.max.y {
                let tick_w = strip_rect.width() * 0.14;
                let (tx0, tx1) = if is_left {
                    (strip_rect.max.x - tick_w, strip_rect.max.x)
                } else {
                    (strip_rect.min.x, strip_rect.min.x + tick_w)
                };
                painter.line_segment(
                    [Pos2::new(tx0, mpy), Pos2::new(tx1, mpy)],
                    Stroke::new(1.0, theme.timeline_dimmed),
                );
            }
        }

        val += major_interval;
    }
}

// ============================================================================
// Curve, Keyframe, and Tangent Rendering
// ============================================================================

#[derive(Debug, Clone)]
struct KeyframeHit {
    instance_name: String,
    path: TrackPath,
    tick: Tick,
    pos: Pos2,
    #[allow(dead_code)]
    track_type: TrackType,
}

#[derive(Debug, Clone)]
struct TangentHit {
    instance_name: String,
    path: TrackPath,
    tick: Tick,
    side: TangentSide,
    handle_pos: Pos2,
    ref_value: f32,
    #[allow(dead_code)]
    track_type: TrackType,
}

fn collect_visible_tracks<'a>(
    anim: &'a crate::anim_editor::model::AnimationData,
    selected_tracks: &[TrackPath],
    theme: &AceTheme,
) -> Vec<VisibleCurveTrack<'a>> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<(String, TrackPath)> = BTreeSet::new();

    if !selected_tracks.is_empty() {
        for (inst_name, inst) in &anim.instances {
            for sel_path in selected_tracks {
                if sel_path.is_empty() {
                    continue;
                }
                if let Some(root_track) = inst.tracks.get(&sel_path[0]) {
                    let mut cur = root_track;
                    let mut valid = true;
                    for seg in &sel_path[1..] {
                        if let Some(next) = cur.components.get(seg) {
                            cur = next;
                        } else {
                            valid = false;
                            break;
                        }
                    }
                    if valid {
                        let mut base = sel_path.clone();
                        cur.traverse_leaves(&mut base, &mut |leaf_path, leaf_track| {
                            let path_vec = leaf_path.to_vec();
                            if seen.insert((inst_name.clone(), path_vec.clone())) {
                                let leaf_name = path_vec.last().cloned().unwrap_or_default();
                                let color =
                                    theme.curve_color_for_track(leaf_track.track_type, &leaf_name);
                                out.push(VisibleCurveTrack {
                                    instance_name: inst_name.clone(),
                                    path: path_vec,
                                    leaf_name,
                                    track: leaf_track,
                                    color,
                                });
                            }
                        });
                    }
                }
            }
        }
    }

    if out.is_empty() {
        for (inst_name, inst) in &anim.instances {
            for (top_name, track) in &inst.tracks {
                let mut base = vec![top_name.clone()];
                track.traverse_leaves(&mut base, &mut |leaf_path, leaf_track| {
                    let path_vec = leaf_path.to_vec();
                    if seen.insert((inst_name.clone(), path_vec.clone())) {
                        let leaf_name = path_vec.last().cloned().unwrap_or_default();
                        let color = theme.curve_color_for_track(leaf_track.track_type, &leaf_name);
                        out.push(VisibleCurveTrack {
                            instance_name: inst_name.clone(),
                            path: path_vec,
                            leaf_name,
                            track: leaf_track,
                            color,
                        });
                    }
                });
            }
        }
    }

    out
}

fn draw_curve_segments(
    painter: &egui::Painter,
    vt: &VisibleCurveTrack<'_>,
    xform: &CurveTransform,
    selected: &SelectedKeyframes,
    metrics: &TimelineMetrics,
    theme: &AceTheme,
) {
    let track = vt.track;
    if track.keyframes.len() < 2 {
        return;
    }

    let is_quat = track.track_type == TrackType::Quaternion;
    let is_facs = track.track_type == TrackType::Facs;
    let min_clamp_y = xform.value_to_y(1.0);
    let max_clamp_y = xform.value_to_y(0.0);

    for pair in track.keyframes.windows(2) {
        let t0 = pair[0];
        let t1 = pair[1];
        if t1 < metrics.start_tick || t0 > metrics.end_tick {
            continue;
        }
        let (Some(k0), Some(k1)) = (track.data.get(&t0), track.data.get(&t1)) else {
            continue;
        };

        let mode = k0
            .interpolation_mode
            .or_else(|| k0.easing_style.map(EasingStyle::to_key_interpolation))
            .unwrap_or(KeyInterpolationMode::Cubic);

        let sel0 = is_keyframe_selected(selected, &vt.instance_name, &vt.path, t0);
        let sel1 = is_keyframe_selected(selected, &vt.instance_name, &vt.path, t1);
        let highlighted = sel0 || (sel1 && mode != KeyInterpolationMode::Constant);

        let stroke = Stroke::new(
            if highlighted {
                CURVE_WIDTH_SELECTED
            } else {
                CURVE_WIDTH
            },
            if highlighted {
                theme.curve_selected
            } else {
                vt.color
            },
        );

        let (v0, v1) = if is_quat {
            (0.0_f32, 1.0_f32)
        } else {
            let Some(a) = k0.value.as_f32() else { continue };
            let Some(b) = k1.value.as_f32() else { continue };
            (a, b)
        };

        let a = xform.to_canvas(t0 as f32, v0);
        let b = xform.to_canvas(t1 as f32, v1);

        match mode {
            KeyInterpolationMode::Constant => {
                let corner = Pos2::new(b.x, a.y);
                painter.line_segment([a, corner], stroke);
                painter.line_segment([corner, b], stroke);
            }
            KeyInterpolationMode::Linear => {
                painter.line_segment([a, b], stroke);
            }
            KeyInterpolationMode::Cubic => {
                let dx = b.x - a.x;
                if dx.abs() < 1e-3 {
                    painter.line_segment([a, b], stroke);
                    continue;
                }
                let (_, s0_curve) = get_slopes(track, t0);
                let (s1_curve, _) = get_slopes(track, t1);
                let s0 = xform.scale_slope(s0_curve.unwrap_or(0.0));
                let s1 = xform.scale_slope(s1_curve.unwrap_or(0.0));

                let mut points = Vec::new();
                let mut x = a.x;
                while x < b.x {
                    let u = ((x - a.x) / dx).clamp(0.0, 1.0);
                    let mut y = if !is_quat {
                        let sample_tick = (t0 as f32 + u * (t1 - t0) as f32).round() as Tick;
                        let val = evaluate_number_track(track, sample_tick)
                            .unwrap_or_else(|| v0 + (v1 - v0) * u);
                        xform.value_to_y(val)
                    } else {
                        eval_hermite_y(a.y, b.y, dx, s0, s1, u)
                    };
                    if is_facs {
                        y = y.clamp(min_clamp_y.min(max_clamp_y), min_clamp_y.max(max_clamp_y));
                    }
                    if x >= xform.canvas_rect.min.x - CURVE_INTERVAL
                        && x <= xform.canvas_rect.max.x + CURVE_INTERVAL
                    {
                        points.push(Pos2::new(x, y));
                    }
                    x += CURVE_INTERVAL;
                }
                let mut end_y = b.y;
                if is_facs {
                    end_y = end_y.clamp(min_clamp_y.min(max_clamp_y), min_clamp_y.max(max_clamp_y));
                }
                points.push(Pos2::new(b.x, end_y));

                if points.len() >= 2 {
                    painter.add(Shape::line(points, stroke));
                }
            }
        }
    }
}

#[inline]
fn eval_hermite_y(ay: f32, by: f32, dx: f32, s0: f32, s1: f32, u: f32) -> f32 {
    let u2 = u * u;
    let h00 = u2 * (2.0 * u - 3.0) + 1.0;
    let h10 = u * (u * (u - 2.0) + 1.0);
    let h01 = u2 * (3.0 - 2.0 * u);
    let h11 = u2 * (u - 1.0);
    h00 * ay + h10 * dx * s0 + h01 * by + h11 * dx * s1
}

fn draw_scrubber_marker(
    painter: &egui::Painter,
    vt: &VisibleCurveTrack<'_>,
    playhead: Tick,
    xform: &CurveTransform,
    theme: &AceTheme,
) {
    if vt.track.keyframes.is_empty() || vt.track.track_type == TrackType::Quaternion {
        return;
    }
    let Some(mut val) = evaluate_number_track(vt.track, playhead) else {
        return;
    };
    if vt.track.track_type == TrackType::Facs {
        val = val.clamp(0.0, 1.0);
    }
    let pos = xform.to_canvas(playhead as f32, val);
    if xform.canvas_rect.contains(pos) {
        draw_diamond(
            painter,
            pos,
            SCRUBBER_MARKER_WIDTH,
            theme.scrubber,
            theme.scrubber,
            1.0,
        );
    }
}

fn collect_and_draw_tangents(
    painter: &egui::Painter,
    vt: &VisibleCurveTrack<'_>,
    xform: &CurveTransform,
    selected: &SelectedKeyframes,
    arm_len: f32,
    theme: &AceTheme,
    out_hits: &mut Vec<TangentHit>,
) {
    let track = vt.track;
    let len = track.keyframes.len();
    if len < 2 {
        return;
    }
    let is_quat = track.track_type == TrackType::Quaternion;

    for (idx, &tick) in track.keyframes.iter().enumerate() {
        if !is_keyframe_selected(selected, &vt.instance_name, &vt.path, tick) {
            continue;
        }
        let Some(kf) = track.data.get(&tick) else {
            continue;
        };
        let (left_slope_curve, right_slope_curve) = get_slopes(track, tick);

        // Left tangent handle (when idx > 0).
        if idx > 0 {
            let prev_tick = track.keyframes[idx - 1];
            let prev_mode = track
                .data
                .get(&prev_tick)
                .and_then(|pk| pk.interpolation_mode)
                .unwrap_or(KeyInterpolationMode::Cubic);
            let is_auto = prev_mode != KeyInterpolationMode::Cubic || kf.left_slope.is_none();
            let ref_val = if is_quat {
                1.0
            } else {
                kf.value.as_f32().unwrap_or(0.0)
            };
            let origin = xform.to_canvas(tick as f32, ref_val);
            let cs = xform.scale_slope(left_slope_curve.unwrap_or(0.0));
            let dir = Vec2::new(-1.0, -cs).normalized();
            let handle_pos = origin + dir * arm_len;

            draw_tangent_arm_and_handle(painter, origin, handle_pos, is_auto, theme);
            out_hits.push(TangentHit {
                instance_name: vt.instance_name.clone(),
                path: vt.path.clone(),
                tick,
                side: TangentSide::Left,
                handle_pos,
                ref_value: ref_val,
                track_type: track.track_type,
            });
        }

        // Right tangent handle (when idx + 1 < len).
        if idx + 1 < len {
            let mode = kf.interpolation_mode.unwrap_or(KeyInterpolationMode::Cubic);
            let is_auto = mode != KeyInterpolationMode::Cubic || kf.right_slope.is_none();
            let ref_val = if is_quat {
                0.0
            } else {
                kf.value.as_f32().unwrap_or(0.0)
            };
            let origin = xform.to_canvas(tick as f32, ref_val);
            let cs = xform.scale_slope(right_slope_curve.unwrap_or(0.0));
            let dir = Vec2::new(1.0, cs).normalized();
            let handle_pos = origin + dir * arm_len;

            draw_tangent_arm_and_handle(painter, origin, handle_pos, is_auto, theme);
            out_hits.push(TangentHit {
                instance_name: vt.instance_name.clone(),
                path: vt.path.clone(),
                tick,
                side: TangentSide::Right,
                handle_pos,
                ref_value: ref_val,
                track_type: track.track_type,
            });
        }
    }
}

fn draw_tangent_arm_and_handle(
    painter: &egui::Painter,
    origin: Pos2,
    handle_pos: Pos2,
    is_auto: bool,
    theme: &AceTheme,
) {
    painter.line_segment(
        [origin, handle_pos],
        Stroke::new(TANGENT_CONTROL_WIDTH, theme.tangent_control),
    );
    let half = TANGENT_CONTROL_SIZE * 0.5;
    let handle_rect = Rect::from_center_size(handle_pos, Vec2::splat(TANGENT_CONTROL_SIZE));
    if is_auto {
        // Auto tangent: hollow 9x9 circle/box with accent border.
        painter.circle(
            handle_pos,
            half,
            theme.background,
            Stroke::new(1.5, theme.tangent_control),
        );
    } else {
        // User-defined tangent: filled 9x9 square.
        painter.rect_filled(handle_rect, 1.0, theme.tangent_control);
        painter.rect_stroke(
            handle_rect,
            1.0,
            Stroke::new(1.0, theme.border),
            StrokeKind::Inside,
        );
    }
}

fn collect_and_draw_keyframes(
    painter: &egui::Painter,
    vt: &VisibleCurveTrack<'_>,
    xform: &CurveTransform,
    selected: &SelectedKeyframes,
    theme: &AceTheme,
    out_hits: &mut Vec<KeyframeHit>,
) {
    let track = vt.track;
    let len = track.keyframes.len();
    let is_quat = track.track_type == TrackType::Quaternion;

    for (idx, &tick) in track.keyframes.iter().enumerate() {
        let Some(kf) = track.data.get(&tick) else {
            continue;
        };
        let is_sel = is_keyframe_selected(selected, &vt.instance_name, &vt.path, tick);
        let border_w = if is_sel {
            KEYFRAME_BORDER_SIZE_SELECTED
        } else {
            KEYFRAME_BORDER_SIZE
        };
        let border_col = if is_sel {
            theme.keyframe_default.selected_border
        } else {
            vt.color
        };

        if is_quat {
            // Dual-diamond rendering: incoming at y=1 (idx > 0), outgoing at y=0 (idx + 1 < len).
            if idx > 0 || len == 1 {
                let pos = xform.to_canvas(tick as f32, 1.0);
                if xform.canvas_rect.expand(8.0).contains(pos) {
                    draw_diamond(painter, pos, KEYFRAME_WIDTH, vt.color, border_col, border_w);
                    out_hits.push(KeyframeHit {
                        instance_name: vt.instance_name.clone(),
                        path: vt.path.clone(),
                        tick,
                        pos,
                        track_type: track.track_type,
                    });
                }
            }
            if idx + 1 < len {
                let pos_b = xform.to_canvas(tick as f32, 0.0);
                if xform.canvas_rect.expand(8.0).contains(pos_b) {
                    draw_diamond(
                        painter,
                        pos_b,
                        KEYFRAME_WIDTH,
                        vt.color,
                        border_col,
                        border_w,
                    );
                    out_hits.push(KeyframeHit {
                        instance_name: vt.instance_name.clone(),
                        path: vt.path.clone(),
                        tick,
                        pos: pos_b,
                        track_type: track.track_type,
                    });
                }
            }
        } else if let Some(val) = kf.value.as_f32() {
            let pos = xform.to_canvas(tick as f32, val);
            if xform.canvas_rect.expand(8.0).contains(pos) {
                draw_diamond(painter, pos, KEYFRAME_WIDTH, vt.color, border_col, border_w);
                out_hits.push(KeyframeHit {
                    instance_name: vt.instance_name.clone(),
                    path: vt.path.clone(),
                    tick,
                    pos,
                    track_type: track.track_type,
                });
            }
        }
    }
}

fn draw_diamond(
    painter: &egui::Painter,
    center: Pos2,
    size: f32,
    fill: Color32,
    border: Color32,
    border_width: f32,
) {
    let r = size * 0.72;
    let pts = vec![
        Pos2::new(center.x, center.y - r),
        Pos2::new(center.x + r, center.y),
        Pos2::new(center.x, center.y + r),
        Pos2::new(center.x - r, center.y),
    ];
    painter.add(Shape::convex_polygon(
        pts,
        fill,
        Stroke::new(border_width, border),
    ));
}

fn build_keyframe_tooltip(path: &[String], track: &Track, tick: Tick) -> String {
    let mut lines = vec![path_to_string(path)];
    if let Some(kf) = track.data.get(&tick) {
        let mode = kf.interpolation_mode.unwrap_or(KeyInterpolationMode::Cubic);
        lines.push(format!("Interpolation: {}", mode.as_str()));

        let prev_is_cubic = track
            .keyframes
            .binary_search(&tick)
            .ok()
            .and_then(|idx| idx.checked_sub(1))
            .and_then(|pi| track.data.get(&track.keyframes[pi]))
            .is_some_and(|pk| {
                pk.interpolation_mode.unwrap_or(KeyInterpolationMode::Cubic)
                    == KeyInterpolationMode::Cubic
            });

        if prev_is_cubic {
            let left_str = match kf.left_slope {
                Some(s) => {
                    let v = s * TICK_FREQUENCY as f32;
                    format!("{:.3}", if v == 0.0 { 0.0 } else { v })
                }
                None => "Auto".to_string(),
            };
            lines.push(format!("Left Tangent: {left_str}"));
        }
        if mode == KeyInterpolationMode::Cubic {
            let right_str = match kf.right_slope {
                Some(s) => {
                    let v = s * TICK_FREQUENCY as f32;
                    format!("{:.3}", if v == 0.0 { 0.0 } else { v })
                }
                None => "Auto".to_string(),
            };
            lines.push(format!("Right Tangent: {right_str}"));
        }
    }
    lines.join("\n")
}

// ============================================================================
// Pointer, Drag, Pan, Zoom & Marquee Interactions
// ============================================================================

fn find_keyframe_hit<'a>(hits: &'a [KeyframeHit], pos: Pos2) -> Option<&'a KeyframeHit> {
    let radius = (KEYFRAME_WIDTH + 4.0).max(8.0);
    hits.iter().rev().find(|h| h.pos.distance(pos) <= radius)
}

fn find_tangent_hit<'a>(hits: &'a [TangentHit], pos: Pos2) -> Option<&'a TangentHit> {
    let radius = TANGENT_CONTROL_SIZE * 0.75 + 2.0;
    hits.iter()
        .rev()
        .find(|h| h.handle_pos.distance(pos) <= radius)
}

fn handle_wheel_zoom(
    ui: &Ui,
    state: &mut EditorState,
    canvas_rect: Rect,
    metrics: &TimelineMetrics,
    canvas_hovered: bool,
    scale_hovered: bool,
) {
    if !canvas_hovered && !scale_hovered {
        return;
    }
    let (scroll_y, ctrl, alt, pointer_pos) = ui.ctx().input(|i| {
        (
            i.smooth_scroll_delta.y,
            i.modifiers.ctrl || i.modifiers.command,
            i.modifiers.alt,
            i.pointer.hover_pos(),
        )
    });
    if scroll_y.abs() < 0.01 {
        return;
    }

    if ctrl && canvas_hovered {
        // Cursor-anchored horizontal timeline zoom.
        let rel_x = pointer_pos
            .map(|p| ((p.x - canvas_rect.min.x) / canvas_rect.width().max(1.0)).clamp(0.0, 1.0))
            .unwrap_or(0.5);
        let hscroll = state.curve_editor.view.horizontal_scroll;
        let hzoom = state.curve_editor.view.horizontal_zoom;
        let new_scroll = (hscroll + (rel_x - hscroll) * (1.0 - hzoom)).clamp(0.0, 1.0);
        let delta = if scroll_y > 0.0 {
            ZOOM_INCREMENT
        } else {
            -ZOOM_INCREMENT
        };
        state.curve_editor.view.horizontal_scroll = new_scroll;
        state.curve_editor.view.horizontal_zoom = (hzoom + delta).clamp(0.0, 0.99);
    } else if alt || scale_hovered || canvas_hovered {
        // Cursor-anchored vertical zoom (1.1x per notch).
        let xform = CurveTransform::new(canvas_rect, metrics, state);
        let cursor_y = pointer_pos
            .map(|p| xform.y_to_value(p.y))
            .unwrap_or(0.5 * (xform.min_value + xform.max_value));
        let k = 1.0 + CANVAS_ZOOM_INCREMENT;
        let factor = if scroll_y > 0.0 { 1.0 / k } else { k };
        let new_min = cursor_y + (state.curve_editor.view.min_value - cursor_y) * factor;
        let new_max = cursor_y + (state.curve_editor.view.max_value - cursor_y) * factor;
        if (new_max - new_min).abs() > 1e-4 && (new_max - new_min).abs() < 1e7 {
            state.curve_editor.view.min_value = new_min;
            state.curve_editor.view.max_value = new_max;
        }
    }
}

fn handle_middle_mouse_pan(
    ui: &Ui,
    state: &mut EditorState,
    canvas_rect: Rect,
    canvas_resp: &egui::Response,
) {
    if canvas_resp.dragged_by(egui::PointerButton::Middle) {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        let delta = ui.ctx().input(|i| i.pointer.delta());
        if delta.x.abs() > 0.0 {
            let hzoom = state.curve_editor.view.horizontal_zoom.max(0.01);
            let scroll_delta = (-delta.x / canvas_rect.width().max(1.0)) / hzoom;
            state.curve_editor.view.horizontal_scroll =
                (state.curve_editor.view.horizontal_scroll + scroll_delta).clamp(0.0, 1.0);
        }
        if delta.y.abs() > 0.0 {
            let val_span = state.curve_editor.view.max_value - state.curve_editor.view.min_value;
            let active_h = (canvas_rect.height() * (1.0 - 2.0 * CURVE_CANVAS_PADDING)).max(1.0);
            let dv = (delta.y / active_h)
                * val_span
                * (1.0 - state.curve_editor.view.vertical_zoom.min(0.99));
            state.curve_editor.view.min_value += dv;
            state.curve_editor.view.max_value += dv;
        }
    }
}

fn handle_canvas_pointer_interactions(
    ui: &Ui,
    state: &mut EditorState,
    xform: &CurveTransform,
    canvas_resp: &egui::Response,
    hit_keyframes: &[KeyframeHit],
    hit_tangents: &[TangentHit],
) -> bool {
    if state.read_only {
        return false;
    }
    let mut modified = false;

    let modifiers = ui.ctx().input(|i| i.modifiers);
    let multi_select = modifiers.shift || modifiers.ctrl || modifiers.command;

    // 1. Primary press / drag start.
    if canvas_resp.drag_started_by(egui::PointerButton::Primary) {
        if let Some(pos) = canvas_resp.interact_pointer_pos() {
            if let Some(thit) = find_tangent_hit(hit_tangents, pos) {
                state.drag_context = Some(DragContext::new_tangent(
                    thit.instance_name.clone(),
                    thit.path.clone(),
                    thit.tick,
                    thit.ref_value,
                    thit.side,
                    thit.track_type,
                ));
            } else if let Some(khit) = find_keyframe_hit(hit_keyframes, pos) {
                if !is_keyframe_selected(
                    &state.selected_keyframes,
                    &khit.instance_name,
                    &khit.path,
                    khit.tick,
                ) {
                    if !multi_select {
                        state.selected_keyframes.clear();
                    }
                    select_keyframe(
                        &mut state.selected_keyframes,
                        &khit.instance_name,
                        &khit.path,
                        khit.tick,
                    );
                }
                if let Some(anim) = state.animation_data.clone() {
                    let pivot_val = anim
                        .get_track(&khit.instance_name, &khit.path)
                        .and_then(|t| t.data.get(&khit.tick))
                        .and_then(|kf| kf.value.as_f32());
                    state.drag_context = Some(DragContext::new_keyframes(
                        anim,
                        state.selected_keyframes.clone(),
                        khit.tick,
                        pivot_val,
                    ));
                }
            } else {
                if !multi_select {
                    state.selected_keyframes.clear();
                }
                state.curve_editor.is_dragging_marquee = true;
                state.curve_editor.marquee_start = Some((pos.x, pos.y));
                state.curve_editor.marquee_end = Some((pos.x, pos.y));
            }
        }
    } else if canvas_resp.clicked_by(egui::PointerButton::Primary) {
        if let Some(pos) = canvas_resp.interact_pointer_pos() {
            if let Some(khit) = find_keyframe_hit(hit_keyframes, pos) {
                if multi_select {
                    toggle_keyframe_selection(
                        &mut state.selected_keyframes,
                        &khit.instance_name,
                        &khit.path,
                        khit.tick,
                    );
                } else {
                    state.selected_keyframes.clear();
                    select_keyframe(
                        &mut state.selected_keyframes,
                        &khit.instance_name,
                        &khit.path,
                        khit.tick,
                    );
                }
            } else if !multi_select {
                state.selected_keyframes.clear();
            }
        }
    }

    // 2. Active drag update.
    if canvas_resp.dragged_by(egui::PointerButton::Primary) {
        if let Some(pos) = ui.ctx().pointer_latest_pos() {
            let (cursor_tick_f, cursor_val) = xform.to_curve(pos);

            if state.curve_editor.is_dragging_marquee {
                state.curve_editor.marquee_end = Some((pos.x, pos.y));
                if let Some((sx, sy)) = state.curve_editor.marquee_start {
                    let (t_a, v_a) = xform.to_curve(Pos2::new(sx, sy));
                    let min_t = t_a.min(cursor_tick_f);
                    let max_t = t_a.max(cursor_tick_f);
                    let min_v = v_a.min(cursor_val);
                    let max_v = v_a.max(cursor_val);
                    if !multi_select {
                        state.selected_keyframes.clear();
                    }
                    if let Some(anim) = &state.animation_data {
                        let vts = collect_visible_tracks(
                            anim,
                            &state.selected_tracks,
                            &AceTheme::current(),
                        );
                        for vt in &vts {
                            for &t in &vt.track.keyframes {
                                let tf = t as f32;
                                if tf < min_t || tf > max_t {
                                    continue;
                                }
                                let hit_y = if vt.track.track_type == TrackType::Quaternion {
                                    (min_v <= 0.0 && max_v >= 0.0) || (min_v <= 1.0 && max_v >= 1.0)
                                } else if let Some(val) =
                                    vt.track.data.get(&t).and_then(|k| k.value.as_f32())
                                {
                                    val >= min_v && val <= max_v
                                } else {
                                    false
                                };
                                if hit_y {
                                    select_keyframe(
                                        &mut state.selected_keyframes,
                                        &vt.instance_name,
                                        &vt.path,
                                        t,
                                    );
                                }
                            }
                        }
                    }
                }
            } else if let Some(mut ctx) = state.drag_context.take() {
                match &mut ctx {
                    DragContext::Tangent {
                        instance_name,
                        path,
                        tick,
                        value,
                        side,
                        has_waypoint,
                        ..
                    } => {
                        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                        let valid = match side {
                            TangentSide::Left => cursor_tick_f < (*tick as f32) - 0.5,
                            TangentSide::Right => cursor_tick_f > (*tick as f32) + 0.5,
                        };
                        if valid {
                            let slope = (cursor_val - *value) / (cursor_tick_f - *tick as f32);
                            if !*has_waypoint {
                                state.add_waypoint();
                                *has_waypoint = true;
                            }
                            set_keyframe_tangent(
                                state,
                                instance_name,
                                path,
                                *tick,
                                *side,
                                Some(slope),
                            );
                            // Linked tangents when Shift is NOT held (broken when Shift IS held).
                            if !modifiers.shift {
                                let opposite = match *side {
                                    TangentSide::Left => TangentSide::Right,
                                    TangentSide::Right => TangentSide::Left,
                                };
                                set_keyframe_tangent(
                                    state,
                                    instance_name,
                                    path,
                                    *tick,
                                    opposite,
                                    Some(slope),
                                );
                            }
                            modified = true;
                        }
                    }
                    DragContext::Keyframes {
                        animation_data,
                        selected_keyframes,
                        pivot_tick,
                        new_tick,
                        pivot_value,
                        new_value,
                        has_waypoint,
                        ..
                    } => {
                        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                        let raw_tick = clamp_tick(nearest_tick(cursor_tick_f as f64));
                        let snapped_tick = match state.settings.snap_mode {
                            SnapMode::Frames => nearest_frame(raw_tick, state.frame_rate),
                            SnapMode::Keyframes | SnapMode::None => raw_tick,
                        };
                        let delta_tick = snapped_tick - *pivot_tick;
                        let delta_val = pivot_value.map(|pv| cursor_val - pv).unwrap_or(0.0);

                        if snapped_tick != *new_tick
                            || new_value.is_none_or(|nv| (nv - cursor_val).abs() > 1e-4)
                        {
                            if !*has_waypoint {
                                state.add_waypoint();
                                *has_waypoint = true;
                            }
                            *new_tick = snapped_tick;
                            *new_value = Some(cursor_val);

                            let mut updated = animation_data.clone();
                            let mut new_selection = SelectedKeyframes::new();
                            apply_curve_keyframe_drag(
                                &mut updated,
                                selected_keyframes,
                                delta_tick,
                                delta_val,
                                &mut new_selection,
                            );
                            updated.set_end_tick();
                            state.animation_data = Some(updated);
                            state.selected_keyframes = new_selection;
                            state.is_dirty = true;
                            modified = true;
                        }
                    }
                    DragContext::Events { .. } => {}
                }
                state.drag_context = Some(ctx);
            }
        }
    }

    // 3. Drag release.
    if canvas_resp.drag_stopped() {
        state.drag_context = None;
        state.curve_editor.is_dragging_marquee = false;
        state.curve_editor.marquee_start = None;
        state.curve_editor.marquee_end = None;
    }

    modified
}

fn apply_curve_keyframe_drag(
    anim: &mut crate::anim_editor::model::AnimationData,
    selected: &SelectedKeyframes,
    delta_tick: Tick,
    delta_val: f32,
    out_selection: &mut SelectedKeyframes,
) {
    for (inst_name, tracks) in selected {
        for (top_name, node) in tracks {
            let mut path = vec![top_name.clone()];
            apply_drag_recursive(
                anim,
                inst_name,
                &mut path,
                node,
                delta_tick,
                delta_val,
                out_selection,
            );
        }
    }
}

fn apply_drag_recursive(
    anim: &mut crate::anim_editor::model::AnimationData,
    inst_name: &str,
    path: &mut TrackPath,
    node: &SelectionNode,
    delta_tick: Tick,
    delta_val: f32,
    out_selection: &mut SelectedKeyframes,
) {
    if node.components.is_empty() {
        if let Some(track) = anim.get_track_mut(inst_name, path) {
            let mut moved = Vec::new();
            for &old_tick in &node.selection {
                if let Some(mut kf) = track.data.remove(&old_tick) {
                    if let Ok(idx) = track.keyframes.binary_search(&old_tick) {
                        track.keyframes.remove(idx);
                    }
                    let new_tick = clamp_tick(old_tick + delta_tick);
                    // Vertical drag skips Quaternion and clamps Facs to [0, 1].
                    match &mut kf.value {
                        KeyframeValue::Number(v) | KeyframeValue::Angle(v) => {
                            *v += delta_val;
                        }
                        KeyframeValue::Facs(v) => {
                            *v = (*v + delta_val).clamp(0.0, 1.0);
                        }
                        KeyframeValue::Quaternion(_)
                        | KeyframeValue::CFrame(_)
                        | KeyframeValue::Position(_) => {}
                    }
                    moved.push((new_tick, kf));
                }
            }
            for (nt, kf) in moved {
                track.add_keyframe(nt, kf);
                select_keyframe(out_selection, inst_name, path, nt);
            }
        }
    } else {
        for (comp_name, child) in &node.components {
            path.push(comp_name.clone());
            apply_drag_recursive(
                anim,
                inst_name,
                path,
                child,
                delta_tick,
                delta_val,
                out_selection,
            );
            path.pop();
        }
    }
}

// ============================================================================
// Context Menus (`KeyframeActions` & `TangentActions`)
// ============================================================================

fn show_tangent_actions_menu(ui: &mut Ui, state: &mut EditorState) {
    let Some(inst) = state.right_click_context.instance_name.clone() else {
        return;
    };
    let Some(path) = state.right_click_context.path.clone() else {
        return;
    };
    let Some(tick) = state.right_click_context.tick else {
        return;
    };
    let Some(side) = state.right_click_context.tangent_side else {
        return;
    };

    if ui.button("Clear Tangent (Auto)").clicked() {
        state.add_waypoint();
        set_keyframe_tangent(state, &inst, &path, tick, side, None);
        ui.close();
    }
    if ui.button("Zero Tangent").clicked() {
        state.add_waypoint();
        set_keyframe_tangent(state, &inst, &path, tick, side, Some(0.0));
        ui.close();
    }
    ui.separator();
    if ui.button("Clear Both Tangents").clicked() {
        state.add_waypoint();
        set_keyframe_tangent(state, &inst, &path, tick, TangentSide::Left, None);
        set_keyframe_tangent(state, &inst, &path, tick, TangentSide::Right, None);
        ui.close();
    }
}

fn show_keyframe_actions_menu(ui: &mut Ui, state: &mut EditorState) {
    let has_sel = !is_selection_empty(&state.selected_keyframes);

    if ui
        .add_enabled(has_sel, egui::Button::new("Delete Selected"))
        .clicked()
    {
        crate::anim_editor::ops::delete_selected_keyframes(state);
        ui.close();
    }
    ui.separator();
    if ui.add_enabled(has_sel, egui::Button::new("Cut")).clicked() {
        crate::anim_editor::ops::copy_selected_keyframes(state);
        crate::anim_editor::ops::delete_selected_keyframes(state);
        ui.close();
    }
    if ui.add_enabled(has_sel, egui::Button::new("Copy")).clicked() {
        crate::anim_editor::ops::copy_selected_keyframes(state);
        ui.close();
    }
    let can_paste = !state.clipboard.is_empty();
    if ui
        .add_enabled(can_paste, egui::Button::new("Paste"))
        .clicked()
    {
        let target_tick = state
            .right_click_context
            .tick
            .unwrap_or(state.playback.playhead);
        crate::anim_editor::ops::paste_keyframes(state, target_tick);
        ui.close();
    }

    if has_sel {
        ui.separator();
        if ui.button("Reset Selected").clicked() {
            reset_selected_curve_keyframes(state);
            ui.close();
        }
        ui.separator();

        // Interpolation mode submenu.
        let uniform_mode = common_interpolation_mode(state);
        ui.menu_button("Interpolation Mode", |ui| {
            for mode in [
                KeyInterpolationMode::Cubic,
                KeyInterpolationMode::Linear,
                KeyInterpolationMode::Constant,
            ] {
                let checked = uniform_mode == Some(mode);
                let label = if checked {
                    format!("✔ {}", mode.as_str())
                } else {
                    format!("   {}", mode.as_str())
                };
                if ui.button(label).clicked() {
                    set_selected_interpolation_mode(state, mode);
                    ui.close();
                }
            }
        });

        if ui.button("Clear Both Tangents").clicked() {
            clear_selected_tangents(state);
            ui.close();
        }

        if has_multiple_selected_ticks(&state.selected_keyframes) {
            ui.menu_button("Generate Curve from Easing", |ui| {
                for style in [EasingStyle::Bounce, EasingStyle::Elastic] {
                    ui.menu_button(style.as_str(), |ui| {
                        for &dir in EASING_DIRECTION_ORDER {
                            if ui.button(dir.as_str()).clicked() {
                                generate_curve_from_easing(state, style, dir);
                                ui.close();
                            }
                        }
                    });
                }
            });
        }
    }

    ui.separator();
    if ui.button("Fit View (F)").clicked() {
        fit_curve_view(state);
        ui.close();
    }
}

// ============================================================================
// Selection & Edit Helpers
// ============================================================================

fn is_keyframe_selected(
    selected: &SelectedKeyframes,
    instance_name: &str,
    path: &[String],
    tick: Tick,
) -> bool {
    let Some(inst) = selected.get(instance_name) else {
        return false;
    };
    let Some(first) = path.first() else {
        return false;
    };
    let Some(mut node) = inst.get(first) else {
        return false;
    };
    for seg in &path[1..] {
        let Some(child) = node.components.get(seg) else {
            return false;
        };
        node = child;
    }
    node.contains_tick(tick)
}

fn select_keyframe(
    selected: &mut SelectedKeyframes,
    instance_name: &str,
    path: &[String],
    tick: Tick,
) {
    let Some(first) = path.first() else {
        return;
    };
    let inst = selected.entry(instance_name.to_string()).or_default();
    let mut node = inst.entry(first.clone()).or_default();
    node.selection.insert(tick);
    for seg in &path[1..] {
        node = node.components.entry(seg.clone()).or_default();
        node.selection.insert(tick);
    }
}

fn toggle_keyframe_selection(
    selected: &mut SelectedKeyframes,
    instance_name: &str,
    path: &[String],
    tick: Tick,
) {
    if is_keyframe_selected(selected, instance_name, path, tick) {
        if let Some(inst) = selected.get_mut(instance_name) {
            if let Some(first) = path.first() {
                if let Some(node) = inst.get_mut(first) {
                    remove_tick_from_node(node, &path[1..], tick);
                    node.prune_empty();
                }
            }
        }
    } else {
        select_keyframe(selected, instance_name, path, tick);
    }
}

fn remove_tick_from_node(node: &mut SelectionNode, rel_path: &[String], tick: Tick) {
    if rel_path.is_empty() {
        node.selection.remove(&tick);
    } else if let Some(child) = node.components.get_mut(&rel_path[0]) {
        remove_tick_from_node(child, &rel_path[1..], tick);
        let still_in_children = node.components.values().any(|c| c.contains_tick(tick));
        if !still_in_children {
            node.selection.remove(&tick);
        }
    }
}

fn common_interpolation_mode(state: &EditorState) -> Option<KeyInterpolationMode> {
    let anim = state.animation_data.as_ref()?;
    let mut found: Option<KeyInterpolationMode> = None;
    for (inst_name, tracks) in &state.selected_keyframes {
        for (top_name, node) in tracks {
            let mut path = vec![top_name.clone()];
            if !inspect_common_mode(anim, inst_name, &mut path, node, &mut found) {
                return None;
            }
        }
    }
    found
}

fn inspect_common_mode(
    anim: &crate::anim_editor::model::AnimationData,
    inst_name: &str,
    path: &mut TrackPath,
    node: &SelectionNode,
    found: &mut Option<KeyInterpolationMode>,
) -> bool {
    if node.components.is_empty() {
        if let Some(track) = anim.get_track(inst_name, path) {
            for &tick in &node.selection {
                if let Some(kf) = track.data.get(&tick) {
                    let mode = kf.interpolation_mode.unwrap_or(KeyInterpolationMode::Cubic);
                    match *found {
                        None => *found = Some(mode),
                        Some(prev) if prev != mode => return false,
                        _ => {}
                    }
                }
            }
        }
        true
    } else {
        for (k, child) in &node.components {
            path.push(k.clone());
            let ok = inspect_common_mode(anim, inst_name, path, child, found);
            path.pop();
            if !ok {
                return false;
            }
        }
        true
    }
}

fn set_selected_interpolation_mode(state: &mut EditorState, mode: KeyInterpolationMode) {
    state.add_waypoint();
    let sel = state.selected_keyframes.clone();
    if let Some(anim) = &mut state.animation_data {
        for (inst_name, tracks) in &sel {
            for (top_name, node) in tracks {
                let mut path = vec![top_name.clone()];
                visit_selected_leaves(anim, inst_name, &mut path, node, &mut |track, tick| {
                    if let Some(kf) = track.data.get_mut(&tick) {
                        kf.interpolation_mode = Some(mode);
                        if mode != KeyInterpolationMode::Cubic {
                            kf.right_slope = None;
                        }
                    }
                });
            }
        }
        state.is_dirty = true;
    }
}

fn clear_selected_tangents(state: &mut EditorState) {
    state.add_waypoint();
    let sel = state.selected_keyframes.clone();
    if let Some(anim) = &mut state.animation_data {
        for (inst_name, tracks) in &sel {
            for (top_name, node) in tracks {
                let mut path = vec![top_name.clone()];
                visit_selected_leaves(anim, inst_name, &mut path, node, &mut |track, tick| {
                    if let Some(kf) = track.data.get_mut(&tick) {
                        kf.left_slope = None;
                        kf.right_slope = None;
                    }
                });
            }
        }
        state.is_dirty = true;
    }
}

fn reset_selected_curve_keyframes(state: &mut EditorState) {
    state.add_waypoint();
    let sel = state.selected_keyframes.clone();
    if let Some(anim) = &mut state.animation_data {
        for (inst_name, tracks) in &sel {
            for (top_name, node) in tracks {
                let mut path = vec![top_name.clone()];
                visit_selected_leaves(anim, inst_name, &mut path, node, &mut |track, tick| {
                    let default_val = KeyframeValue::default_for(track.track_type);
                    if let Some(kf) = track.data.get_mut(&tick) {
                        kf.value = default_val;
                        kf.left_slope = None;
                        kf.right_slope = None;
                    }
                });
            }
        }
        state.is_dirty = true;
    }
}

fn generate_curve_from_easing(
    state: &mut EditorState,
    style: EasingStyle,
    direction: EasingDirection,
) {
    let Some((earliest, latest)) =
        crate::anim_editor::state::selection_bounds(&state.selected_keyframes)
    else {
        return;
    };
    if earliest >= latest {
        return;
    }
    state.add_waypoint();
    let sel = state.selected_keyframes.clone();
    if let Some(anim) = &mut state.animation_data {
        let mut leaf_paths: Vec<(String, TrackPath)> = Vec::new();
        for (inst_name, tracks) in &sel {
            for (top_name, node) in tracks {
                let mut path = vec![top_name.clone()];
                collect_selected_leaf_paths(inst_name, &mut path, node, &mut leaf_paths);
            }
        }
        for (inst_name, path) in leaf_paths {
            if let Some(track) = anim.get_track_mut(&inst_name, &path) {
                if !track.data.contains_key(&earliest) {
                    let val = evaluate_number_track(track, earliest).unwrap_or(0.0);
                    let kf = crate::anim_editor::model::Keyframe::new_curve(scalar_value_for(
                        track.track_type,
                        val,
                    ));
                    track.add_keyframe(earliest, kf);
                }
                if !track.data.contains_key(&latest) {
                    let val = evaluate_number_track(track, latest).unwrap_or(0.0);
                    let kf = crate::anim_editor::model::Keyframe::new_curve(scalar_value_for(
                        track.track_type,
                        val,
                    ));
                    track.add_keyframe(latest, kf);
                }
                // Remove existing interior keyframes in (earliest, latest).
                let to_remove: Vec<Tick> = track
                    .keyframes
                    .iter()
                    .copied()
                    .filter(|&t| t > earliest && t < latest)
                    .collect();
                for t in to_remove {
                    track.delete_keyframe(t);
                }
                let (Some(mut low_kf), Some(mut high_kf)) = (
                    track.data.get(&earliest).copied(),
                    track.data.get(&latest).copied(),
                ) else {
                    continue;
                };
                let generated: BTreeMap<Tick, crate::anim_editor::model::Keyframe> =
                    crate::anim_editor::curves::generate_curve(
                        track.track_type,
                        style,
                        direction,
                        earliest,
                        &mut low_kf,
                        latest,
                        &mut high_kf,
                    );
                track.add_keyframe(earliest, low_kf);
                track.add_keyframe(latest, high_kf);
                for (t, kf) in generated {
                    track.add_keyframe(t, kf);
                }
            }
        }
        state.selected_keyframes.clear();
        state.is_dirty = true;
    }
}

fn scalar_value_for(track_type: TrackType, v: f32) -> KeyframeValue {
    match track_type {
        TrackType::Angle => KeyframeValue::Angle(v),
        TrackType::Facs => KeyframeValue::Facs(v.clamp(0.0, 1.0)),
        _ => KeyframeValue::Number(v),
    }
}

fn collect_selected_leaf_paths(
    inst_name: &str,
    path: &mut TrackPath,
    node: &SelectionNode,
    out: &mut Vec<(String, TrackPath)>,
) {
    if node.components.is_empty() {
        out.push((inst_name.to_string(), path.clone()));
    } else {
        for (k, child) in &node.components {
            path.push(k.clone());
            collect_selected_leaf_paths(inst_name, path, child, out);
            path.pop();
        }
    }
}

fn visit_selected_leaves<F>(
    anim: &mut crate::anim_editor::model::AnimationData,
    inst_name: &str,
    path: &mut TrackPath,
    node: &SelectionNode,
    f: &mut F,
) where
    F: FnMut(&mut Track, Tick),
{
    if node.components.is_empty() {
        if let Some(track) = anim.get_track_mut(inst_name, path) {
            for &tick in &node.selection {
                f(track, tick);
            }
        }
    } else {
        for (k, child) in &node.components {
            path.push(k.clone());
            visit_selected_leaves(anim, inst_name, path, child, f);
            path.pop();
        }
    }
}

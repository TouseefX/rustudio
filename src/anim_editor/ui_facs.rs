//! Floating Face Controls (FACS) editor window (`Components/FaceControlsEditor/*`).

use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};

use crate::anim_editor::constants::{
    facs_controls_in_region, facs_symmetry_partner, FacsRegion, FACS_CONTROLS,
};
use crate::anim_editor::facs::{
    add_all_facs_tracks, get_dual_slider_value, get_eyes_drag_box, get_facs_value,
    has_facs_keyframe_at_playhead, reset_all_facs, reset_region_facs, set_dual_slider_value,
    set_eyes_drag_box, set_facs_value,
};
use crate::anim_editor::state::EditorState;
use crate::anim_editor::theme::AceTheme;

const REGION_TABS: &[FacsRegion] = &[
    FacsRegion::Eyes,
    FacsRegion::Brows,
    FacsRegion::Mouth,
    FacsRegion::Jaw,
    FacsRegion::Tongue,
];

/// Dual-slider definitions grouped by `FacsRegion`: `(label, neg_control, pos_control)`.
fn dual_sliders_for_region(
    region: FacsRegion,
) -> &'static [(&'static str, &'static str, &'static str)] {
    match region {
        FacsRegion::Eyes => &[
            ("Eyes Look L / R", "EyesLookLeft", "EyesLookRight"),
            ("Eyes Look D / U", "EyesLookDown", "EyesLookUp"),
        ],
        FacsRegion::Jaw => &[("Jaw L / R", "JawLeft", "JawRight")],
        FacsRegion::Mouth => &[("Mouth L / R", "MouthLeft", "MouthRight")],
        FacsRegion::Tongue => &[("Tongue D / U", "TongueDown", "TongueUp")],
        _ => &[],
    }
}

/// Renders the floating `"Face Controls"` window when `state.facs.show_panel` is true.
/// Returns `true` if the user requested `"Focus Face"` (or turned on Auto-Focus Face).
pub fn show_face_controls_window(
    ctx: &egui::Context,
    state: &mut EditorState,
    theme: &AceTheme,
) -> bool {
    if !state.facs.show_panel {
        return false;
    }

    let mut open = state.facs.show_panel;
    let mut focus_requested = false;

    egui::Window::new("Face Controls")
        .id(egui::Id::new("ace_face_controls_window"))
        .open(&mut open)
        .default_size([310.0, 560.0])
        .min_size([240.0, 320.0])
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            focus_requested = show_face_controls_contents(ui, state, theme);
        });

    state.facs.show_panel = open;
    focus_requested
}

fn show_face_controls_contents(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    theme: &AceTheme,
) -> bool {
    let mut focus_requested = false;

    // Top toolbar row 1: Symmetry, Auto-Focus Face, Focus Face.
    ui.horizontal_wrapped(|ui| {
        let sym_btn = ui.selectable_label(state.facs.symmetry_enabled, "⇄ Symmetry");
        if sym_btn.clicked() {
            state.facs.symmetry_enabled = !state.facs.symmetry_enabled;
        }
        sym_btn.on_hover_text("Mirror left/right FACS control edits symmetrically.");

        let prev_auto = state.facs.auto_focus_face_enabled;
        ui.checkbox(&mut state.facs.auto_focus_face_enabled, "Auto-Focus")
            .on_hover_text("Automatically frame the rig's face when opening Face Controls.");
        if state.facs.auto_focus_face_enabled && !prev_auto {
            focus_requested = true;
        }

        if ui
            .button("Focus Face")
            .on_hover_text("Move viewport camera to frame the character's head.")
            .clicked()
        {
            focus_requested = true;
        }
    });

    // Top toolbar row 2: Reset All, Add All FACS Tracks.
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Reset All")
            .on_hover_text("Reset all 50 FACS controls to 0 at the current playhead.")
            .clicked()
        {
            reset_all_facs(state);
        }
        if ui
            .button("Add All FACS Tracks")
            .on_hover_text("Create tracks for all 50 FaceControls properties.")
            .clicked()
        {
            add_all_facs_tracks(state);
        }
    });

    ui.separator();

    // Search filter stored in egui temporary memory.
    let search_id = egui::Id::new("ace_facs_search_filter");
    let mut search_text: String = ui
        .ctx()
        .data(|d| d.get_temp::<String>(search_id))
        .unwrap_or_default();

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Search:")
                .small()
                .color(theme.dimmed_text),
        );
        ui.add(
            egui::TextEdit::singleline(&mut search_text)
                .hint_text("Filter 50 FACS controls...")
                .desired_width(ui.available_width() - 24.0),
        );
        if !search_text.is_empty() && ui.small_button("×").clicked() {
            search_text.clear();
        }
    });
    ui.ctx()
        .data_mut(|d| d.insert_temp(search_id, search_text.clone()));

    ui.add_space(4.0);

    // 5 Region tabs: Eyes, Brows, Mouth, Jaw, Tongue.
    ui.horizontal_wrapped(|ui| {
        for &region in REGION_TABS {
            let selected = state.facs.active_region == region;
            if ui.selectable_label(selected, region.as_str()).clicked() {
                state.facs.active_region = region;
            }
        }
    });

    ui.separator();

    let query = search_text.trim().to_lowercase();
    let active_region = state.facs.active_region;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if query.is_empty() {
                // Region header with quick Reset Region button.
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(active_region.as_str())
                            .strong()
                            .color(theme.bright_text),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Reset Region").clicked() {
                            reset_region_facs(state, active_region);
                        }
                    });
                });
                ui.add_space(4.0);

                // Interactive 2D EyesDragBox on Eyes tab.
                if active_region == FacsRegion::Eyes {
                    show_eyes_drag_box(ui, state, theme);
                    ui.add_space(6.0);
                }

                // Center-filled dual sliders for this region.
                let duals = dual_sliders_for_region(active_region);
                if !duals.is_empty() {
                    for &(label, neg, pos) in duals {
                        show_dual_slider(ui, state, theme, label, neg, pos);
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);
                }

                // All individual [0, 1] FACS sliders in this region.
                for name in facs_controls_in_region(active_region) {
                    show_single_facs_slider(ui, state, theme, name);
                }
            } else {
                // Search across all 50 FACS controls.
                let mut matched = 0usize;
                for &(name, region) in FACS_CONTROLS {
                    if name.to_lowercase().contains(&query)
                        || region.as_str().to_lowercase().contains(&query)
                    {
                        show_single_facs_slider(ui, state, theme, name);
                        matched += 1;
                    }
                }
                if matched == 0 {
                    ui.label(
                        egui::RichText::new("No matching FACS controls.")
                            .italics()
                            .color(theme.dimmed_text),
                    );
                }
            }
        });

    focus_requested
}

/// Interactive 2D `EyesDragBox` (`[-1, 1]^2` painted canvas with crosshair, draggable dot,
/// and double-click reset to `(0, 0)`).
fn show_eyes_drag_box(ui: &mut egui::Ui, state: &mut EditorState, theme: &AceTheme) {
    let (cur_x, cur_y) = get_eyes_drag_box(state);

    ui.horizontal(|ui| {
        let box_size = Vec2::new(136.0, 96.0);
        let (rect, resp) = ui.allocate_exact_size(box_size, Sense::click_and_drag());
        let painter = ui.painter();

        // Canvas background and border.
        painter.rect_filled(rect, 4.0, theme.track_primary_bg);
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0, theme.border),
            egui::StrokeKind::Inside,
        );

        // Crosshair axes.
        let center = rect.center();
        let grid_stroke = Stroke::new(1.0, theme.dimmed_text.gamma_multiply(0.4));
        painter.line_segment(
            [
                Pos2::new(rect.min.x + 4.0, center.y),
                Pos2::new(rect.max.x - 4.0, center.y),
            ],
            grid_stroke,
        );
        painter.line_segment(
            [
                Pos2::new(center.x, rect.min.y + 4.0),
                Pos2::new(center.x, rect.max.y - 4.0),
            ],
            grid_stroke,
        );

        // Subtle axis labels (L / R / U / D).
        let label_font = egui::FontId::proportional(9.5);
        painter.text(
            Pos2::new(rect.min.x + 8.0, center.y),
            egui::Align2::LEFT_CENTER,
            "L",
            label_font.clone(),
            theme.dimmed_text,
        );
        painter.text(
            Pos2::new(rect.max.x - 8.0, center.y),
            egui::Align2::RIGHT_CENTER,
            "R",
            label_font.clone(),
            theme.dimmed_text,
        );
        painter.text(
            Pos2::new(center.x, rect.min.y + 8.0),
            egui::Align2::CENTER_TOP,
            "U",
            label_font.clone(),
            theme.dimmed_text,
        );
        painter.text(
            Pos2::new(center.x, rect.max.y - 8.0),
            egui::Align2::CENTER_BOTTOM,
            "D",
            label_font,
            theme.dimmed_text,
        );

        let pad = 8.0;
        let inner = rect.shrink(pad);

        // Handle double-click reset or drag/click updates.
        if resp.double_clicked() {
            set_eyes_drag_box(state, 0.0, 0.0, true);
        } else if resp.dragged() || resp.clicked() {
            if let Some(pointer) = resp.interact_pointer_pos() {
                let nx = (((pointer.x - inner.min.x) / inner.width().max(1.0)) * 2.0 - 1.0)
                    .clamp(-1.0, 1.0);
                // Screen Y increases downward, so invert so +1 is Up and -1 is Down.
                let ny = (1.0 - ((pointer.y - inner.min.y) / inner.height().max(1.0)) * 2.0)
                    .clamp(-1.0, 1.0);
                let add_wp = resp.drag_started() || resp.clicked();
                set_eyes_drag_box(state, nx, ny, add_wp);
            }
        }

        // Paint gaze dot at current (x, y).
        let (dx, dy) = get_eyes_drag_box(state);
        let dot_pos = Pos2::new(
            inner.min.x + (dx * 0.5 + 0.5) * inner.width(),
            inner.min.y + (0.5 - dy * 0.5) * inner.height(),
        );
        painter.line_segment(
            [center, dot_pos],
            Stroke::new(1.5, theme.accent.gamma_multiply(0.7)),
        );
        painter.circle_filled(dot_pos, 5.5, theme.accent);
        painter.circle_stroke(dot_pos, 5.5, Stroke::new(1.2, theme.bright_text));

        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("Eye Gaze")
                    .strong()
                    .size(12.0)
                    .color(theme.text),
            );
            ui.label(
                egui::RichText::new(format!("X: {:+.2}", cur_x))
                    .monospace()
                    .small()
                    .color(theme.dimmed_text),
            );
            ui.label(
                egui::RichText::new(format!("Y: {:+.2}", cur_y))
                    .monospace()
                    .small()
                    .color(theme.dimmed_text),
            );
            ui.add_space(4.0);
            if ui.small_button("Center").clicked() {
                set_eyes_drag_box(state, 0.0, 0.0, true);
            }
            ui.label(
                egui::RichText::new("Double-click to reset")
                    .size(10.0)
                    .color(theme.dimmed_text),
            );
        });
    });
}

/// Center-filled signed dual slider (`[-1.0, 1.0]`).
fn show_dual_slider(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    theme: &AceTheme,
    label: &str,
    neg_control: &str,
    pos_control: &str,
) {
    let val = get_dual_slider_value(state, neg_control, pos_control);
    let has_kf = has_facs_keyframe_at_playhead(state, neg_control)
        || has_facs_keyframe_at_playhead(state, pos_control);

    ui.horizontal(|ui| {
        paint_keyframe_diamond(ui, has_kf, theme);

        ui.allocate_ui_with_layout(
            Vec2::new(105.0, 18.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(
                    egui::RichText::new(label)
                        .size(11.5)
                        .color(theme.bright_text),
                );
            },
        );

        let slider_w = (ui.available_width() - 44.0).max(60.0);
        let (rect, resp) =
            ui.allocate_exact_size(Vec2::new(slider_w, 16.0), Sense::click_and_drag());
        let painter = ui.painter();

        // Track background
        painter.rect_filled(rect, 3.0, theme.track_primary_bg);
        painter.rect_stroke(
            rect,
            3.0,
            Stroke::new(1.0, theme.border),
            egui::StrokeKind::Inside,
        );

        // Center tick
        let center_x = rect.center().x;
        painter.line_segment(
            [
                Pos2::new(center_x, rect.min.y + 1.0),
                Pos2::new(center_x, rect.max.y - 1.0),
            ],
            Stroke::new(1.0, theme.dimmed_text),
        );

        // Center-filled bar
        let frac = val.clamp(-1.0, 1.0);
        let fill_x = center_x + frac * (rect.width() * 0.5);
        let fill_rect = Rect::from_min_max(
            Pos2::new(center_x.min(fill_x), rect.min.y + 2.0),
            Pos2::new(center_x.max(fill_x), rect.max.y - 2.0),
        );
        if fill_rect.width() > 0.5 {
            painter.rect_filled(fill_rect, 2.0, theme.accent);
        }

        // Thumb indicator
        painter.line_segment(
            [
                Pos2::new(fill_x, rect.min.y + 1.0),
                Pos2::new(fill_x, rect.max.y - 1.0),
            ],
            Stroke::new(2.0, theme.bright_text),
        );

        if resp.double_clicked() {
            set_dual_slider_value(state, neg_control, pos_control, 0.0, true);
        } else if resp.dragged() || resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let signed =
                    (((pos.x - rect.min.x) / rect.width().max(1.0)) * 2.0 - 1.0).clamp(-1.0, 1.0);
                let add_wp = resp.drag_started() || resp.clicked();
                set_dual_slider_value(state, neg_control, pos_control, signed, add_wp);
            }
        }

        ui.label(
            egui::RichText::new(format!("{:+.2}", val))
                .monospace()
                .size(11.0)
                .color(theme.dimmed_text),
        );
    });
}

/// Individual `[0.0, 1.0]` FACS control slider row with keyframe diamond, symmetry badge,
/// and numeric readout.
fn show_single_facs_slider(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    theme: &AceTheme,
    facs_name: &str,
) {
    let mut val = get_facs_value(state, facs_name);
    let has_kf = has_facs_keyframe_at_playhead(state, facs_name);
    let has_sym = state.facs.symmetry_enabled && facs_symmetry_partner(facs_name).is_some();

    ui.horizontal(|ui| {
        paint_keyframe_diamond(ui, has_kf, theme);

        // Name + symmetry badge
        let name_width = 128.0;
        ui.allocate_ui_with_layout(
            Vec2::new(name_width, 18.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let label_color = if val > 0.0 {
                    theme.bright_text
                } else {
                    theme.text
                };
                ui.add(
                    egui::Label::new(egui::RichText::new(facs_name).size(11.0).color(label_color))
                        .truncate(),
                );
                if has_sym {
                    ui.label(egui::RichText::new("⇄").size(10.0).color(theme.accent));
                }
            },
        );

        let slider_w = (ui.available_width() - 40.0).max(52.0);
        let (rect, resp) =
            ui.allocate_exact_size(Vec2::new(slider_w, 15.0), Sense::click_and_drag());
        let painter = ui.painter();

        painter.rect_filled(rect, 3.0, theme.track_primary_bg);
        painter.rect_stroke(
            rect,
            3.0,
            Stroke::new(1.0, theme.border),
            egui::StrokeKind::Inside,
        );

        let fill_w = rect.width() * val.clamp(0.0, 1.0);
        if fill_w > 0.5 {
            let fill_rect = Rect::from_min_size(
                Pos2::new(rect.min.x + 1.0, rect.min.y + 1.0),
                Vec2::new((fill_w - 2.0).max(1.0), rect.height() - 2.0),
            );
            let fill_color = if has_kf {
                theme.accent
            } else {
                theme.accent.gamma_multiply(0.75)
            };
            painter.rect_filled(fill_rect, 2.0, fill_color);
        }

        if resp.double_clicked() {
            val = 0.0;
            set_facs_value(state, facs_name, 0.0, true);
        } else if resp.dragged() || resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let new_val = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0);
                let add_wp = resp.drag_started() || resp.clicked();
                val = new_val;
                set_facs_value(state, facs_name, new_val, add_wp);
            }
        }

        ui.label(
            egui::RichText::new(format!("{:.2}", val))
                .monospace()
                .size(11.0)
                .color(if val > 0.0 {
                    theme.bright_text
                } else {
                    theme.dimmed_text
                }),
        );
    });
}

/// Paints a small rotated keyframe diamond indicator (filled when keyed at current playhead).
fn paint_keyframe_diamond(ui: &mut egui::Ui, is_keyed: bool, theme: &AceTheme) {
    let (rect, _resp) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
    let c = rect.center();
    let r = 4.0;
    let pts = vec![
        Pos2::new(c.x, c.y - r),
        Pos2::new(c.x + r, c.y),
        Pos2::new(c.x, c.y + r),
        Pos2::new(c.x - r, c.y),
    ];
    let fill = if is_keyed {
        theme.accent
    } else {
        Color32::TRANSPARENT
    };
    let stroke_color = if is_keyed {
        theme.bright_text
    } else {
        theme.dimmed_text.gamma_multiply(0.6)
    };
    ui.painter().add(egui::Shape::convex_polygon(
        pts,
        fill,
        Stroke::new(1.0, stroke_color),
    ));
}

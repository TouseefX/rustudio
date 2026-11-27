//! Left-hand `TrackList` UI (`Header`, `AnimationEventsTrack`, `SummaryTrack`,
//! `AddTrackButton`, hierarchical `Track` / `ExpandableTrack`, inline
//! `NumberBox`, and `TrackActions` context menu) derived from
//! `Components/TrackList/*`.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Pos2, Rect, Sense, Stroke, Vec2,
};

use crate::anim_editor::constants::{
    ARROW_SIZE, EULER_ANGLES_ORDER, INDENT_PADDING, NUMBERBOX_DRAG_MULTIPLIER,
    NUMBERBOX_FACS_DRAG_MULTIPLIER, NUMBERBOX_WIDTH, NUMBER_FACS_PRECISION, NUMBER_PRECISION,
    PROP_POSITION, PROP_ROTATION, PROP_X, PROP_Y, PROP_Z, SUMMARY_TRACK_HEIGHT, TIMELINE_HEIGHT,
    TRACKLIST_BUTTON_SIZE, TRACKLIST_RIGHT_PADDING,
};
use crate::anim_editor::model::{KeyframeValue, RotationOrder, Track, TrackPath, TrackType};
use crate::anim_editor::state::{EditorMode, EditorState, TrackListEntry};
use crate::anim_editor::theme::AceTheme;

/// High-level action emitted by [`show_track_list`].
#[derive(Debug, Clone, PartialEq)]
pub enum TrackListAction {
    None,
    SelectionChanged,
    ValueEdited,
    TrackListChanged,
    OpenAddEventAtPlayhead,
    ToggleEditorMode(EditorMode),
    ToggleIk,
    ToggleFaceControls,
}

/// Flattened visible row shared between `ui_tracklist` and `ui_dopesheet` so
/// row indices stay 1:1 synchronized.
#[derive(Debug, Clone)]
pub struct VisibleTrackRow {
    pub instance: String,
    pub path: TrackPath,
    pub name: String,
    pub depth: usize,
    pub track_type: TrackType,
    pub euler_angles_order: Option<RotationOrder>,
    pub has_children: bool,
    pub expanded: bool,
    pub selected: bool,
    pub has_error: bool,
}

/// Collects the ordered visible track rows respecting each entry's `expanded`
/// state and `TrackType::component_order()`.
pub fn collect_visible_rows(state: &EditorState) -> Vec<VisibleTrackRow> {
    let mut rows = Vec::new();
    for entry in &state.tracks {
        let mut path = vec![entry.name.clone()];
        push_entry_rows(entry, &state.selected_tracks, &mut path, &mut rows);
    }
    rows
}

fn push_entry_rows(
    entry: &TrackListEntry,
    selected_tracks: &[TrackPath],
    path: &mut TrackPath,
    out: &mut Vec<VisibleTrackRow>,
) {
    let has_children = !entry.components.is_empty();
    let is_selected = entry.selected || selected_tracks.iter().any(|p| p == path);
    out.push(VisibleTrackRow {
        instance: entry.instance.clone(),
        path: path.clone(),
        name: entry.name.clone(),
        depth: entry.depth,
        track_type: entry.track_type,
        euler_angles_order: entry.euler_angles_order,
        has_children,
        expanded: entry.expanded,
        selected: is_selected,
        has_error: entry.has_error,
    });

    if entry.expanded && has_children {
        // Order standard components first, then any extras alphabetically.
        let std_order = entry.track_type.component_order();
        for &comp_key in std_order {
            if let Some(child) = entry.components.get(comp_key) {
                path.push(comp_key.to_string());
                push_entry_rows(child, selected_tracks, path, out);
                path.pop();
            }
        }
        for (comp_key, child) in &entry.components {
            if !std_order.contains(&comp_key.as_str()) {
                path.push(comp_key.clone());
                push_entry_rows(child, selected_tracks, path, out);
                path.pop();
            }
        }
    }
}

/// Renders the complete left-hand TrackList panel.
pub fn show_track_list(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    theme: &AceTheme,
    row_height: f32,
) -> TrackListAction {
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut action = TrackListAction::None;
    let width = ui.available_width().max(120.0);

    // 1. Header toolbar row (TIMELINE_HEIGHT)
    let hdr_action = draw_header_toolbar(ui, width, state, theme);
    if hdr_action != TrackListAction::None {
        action = hdr_action;
    }

    // 2. AnimationEventsTrack header row (when show_events && animation_data)
    if state.settings.show_events && state.animation_data.is_some() {
        if draw_events_header_row(ui, width, row_height, state, theme) {
            action = TrackListAction::OpenAddEventAtPlayhead;
        }
    }

    // 3. SummaryTrack header row + AddTrackButton (+) menu
    if draw_summary_header_row(ui, width, SUMMARY_TRACK_HEIGHT, state, theme) {
        action = TrackListAction::TrackListChanged;
    }

    // 4. Scrollable track rows synchronized with state.top_track_index
    let rows = collect_visible_rows(state);
    let total_rows = rows.len();
    let avail_h = ui.available_height().max(row_height);
    let (list_rect, list_resp) = ui.allocate_exact_size(Vec2::new(width, avail_h), Sense::hover());

    // Handle vertical mouse-wheel scrolling over TrackList
    let visible_rows = ((avail_h / row_height).floor() as usize).max(1);
    let max_top = total_rows.saturating_sub(visible_rows).saturating_add(1).max(1);
    state.top_track_index = state.top_track_index.clamp(1, max_top);
    if list_resp.hovered() {
        let scroll_y = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll_y.abs() > 0.5 && !ui.input(|i| i.modifiers.ctrl || i.modifiers.command) {
            let step = if scroll_y > 0.0 { -1isize } else { 1isize };
            let cur = state.top_track_index.max(1) as isize;
            state.top_track_index = (cur + step).clamp(1, max_top as isize) as usize;
        }
    }

    // Paint background stripes for full TrackList area
    let visible_capacity = (avail_h / row_height).ceil() as usize + 1;
    let start_idx = state.top_track_index.saturating_sub(1);
    let painter = ui.painter().with_clip_rect(list_rect);

    for slot in 0..visible_capacity {
        let y = list_rect.min.y + (slot as f32) * row_height;
        if y >= list_rect.max.y {
            break;
        }
        let row_rect =
            Rect::from_min_size(Pos2::new(list_rect.min.x, y), Vec2::new(width, row_height));
        let abs_row = start_idx + slot;
        let bg = if abs_row % 2 == 0 {
            theme.track_shaded_bg
        } else {
            theme.background
        };
        painter.rect_filled(row_rect, CornerRadius::ZERO, bg);
    }

    // Render each visible row
    let mut rendered_bottom = list_rect.min.y;
    for slot in 0..visible_capacity {
        let row_idx = start_idx + slot;
        let Some(row) = rows.get(row_idx) else {
            break;
        };
        let y = list_rect.min.y + (slot as f32) * row_height;
        if y >= list_rect.max.y {
            break;
        }
        let row_rect =
            Rect::from_min_size(Pos2::new(list_rect.min.x, y), Vec2::new(width, row_height));
        rendered_bottom = row_rect.max.y;
        let row_act = draw_track_row(ui, row_rect, row, state, theme);
        if row_act != TrackListAction::None {
            action = row_act;
        }
    }

    // Clicking empty background below the last track row clears selected tracks
    if rendered_bottom < list_rect.max.y {
        let empty_rect = Rect::from_min_max(
            Pos2::new(list_rect.min.x, rendered_bottom),
            list_rect.max,
        );
        let empty_resp = ui.interact(empty_rect, ui.id().with("tracklist_empty"), Sense::click());
        if empty_resp.clicked() {
            state.selected_tracks.clear();
            state.selected_track_instances.clear();
            clear_entry_selection(&mut state.tracks);
            action = TrackListAction::SelectionChanged;
        }
    }

    action
}

fn draw_header_toolbar(
    ui: &mut egui::Ui,
    width: f32,
    state: &mut EditorState,
    theme: &AceTheme,
) -> TrackListAction {
    let mut action = TrackListAction::None;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, TIMELINE_HEIGHT), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, theme.track_title_bg);
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, theme.border),
    );

    let mut x = rect.min.x + 6.0;
    let btn_h = rect.height() - 4.0;
    let y = rect.min.y + 2.0;

    // Mode toggles: DopeSheet / Curves
    let ds_active = state.editor_mode == EditorMode::DopeSheet;
    let ds_rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(66.0, btn_h));
    if draw_toolbar_pill(ui, ds_rect, "DopeSheet", ds_active, theme, "DopeSheet View") {
        action = TrackListAction::ToggleEditorMode(EditorMode::DopeSheet);
    }
    x = ds_rect.max.x + 4.0;

    let cv_active = state.editor_mode == EditorMode::Curves;
    let cv_rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(54.0, btn_h));
    if draw_toolbar_pill(ui, cv_rect, "Curves", cv_active, theme, "Curve Editor View") {
        action = TrackListAction::ToggleEditorMode(EditorMode::Curves);
    }
    x = cv_rect.max.x + 8.0;

    // IK toggle
    let ik_rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(28.0, btn_h));
    if draw_toolbar_pill(
        ui,
        ik_rect,
        "IK",
        state.ik.enabled || state.ik.show_tree,
        theme,
        "Toggle Inverse Kinematics Window",
    ) {
        action = TrackListAction::ToggleIk;
    }
    x = ik_rect.max.x + 4.0;

    // Face Controls toggle (shown if rig has FaceControls)
    if state.rig_cache.has_face_controls() {
        let fc_rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(40.0, btn_h));
        if draw_toolbar_pill(
            ui,
            fc_rect,
            "Face",
            state.facs.show_panel,
            theme,
            "Toggle Face Controls Editor",
        ) {
            action = TrackListAction::ToggleFaceControls;
        }
    }

    // Right-aligned visualization toggles: Bones + Grid
    let grid_rect = Rect::from_min_size(Pos2::new(rect.max.x - 42.0, y), Vec2::new(36.0, btn_h));
    if draw_toolbar_pill(
        ui,
        grid_rect,
        "Grid",
        state.settings.visualize_grid,
        theme,
        "Toggle Floor Grid (G)",
    ) {
        state.settings.visualize_grid = !state.settings.visualize_grid;
    }

    let bone_rect =
        Rect::from_min_size(Pos2::new(grid_rect.min.x - 48.0, y), Vec2::new(44.0, btn_h));
    if draw_toolbar_pill(
        ui,
        bone_rect,
        "Bones",
        state.settings.visualize_bones,
        theme,
        "Toggle Bone Visualization (V)",
    ) {
        state.settings.visualize_bones = !state.settings.visualize_bones;
    }

    action
}

fn draw_toolbar_pill(
    ui: &mut egui::Ui,
    rect: Rect,
    label: &str,
    active: bool,
    theme: &AceTheme,
    tooltip: &str,
) -> bool {
    let resp = ui
        .interact(rect, ui.id().with(label), Sense::click())
        .on_hover_text(tooltip);
    let painter = ui.painter();
    let bg = if active {
        theme.accent
    } else if resp.hovered() {
        theme.scrollbar_hover
    } else {
        theme.scrollbar_control
    };
    let fg = if active {
        theme.accent_text
    } else {
        theme.text
    };
    painter.rect_filled(rect, CornerRadius::same(3), bg);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(theme.font_size_track - 3.0),
        fg,
    );
    resp.clicked()
}

fn draw_events_header_row(
    ui: &mut egui::Ui,
    width: f32,
    row_height: f32,
    state: &mut EditorState,
    theme: &AceTheme,
) -> bool {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, row_height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, theme.track_title_bg);
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, theme.border),
    );
    painter.text(
        Pos2::new(rect.min.x + INDENT_PADDING + 4.0, rect.center().y),
        Align2::LEFT_CENTER,
        "Animation Events",
        FontId::proportional(theme.font_size_track - 1.0),
        theme.text,
    );

    let add_rect = Rect::from_center_size(
        Pos2::new(
            rect.max.x - TRACKLIST_RIGHT_PADDING - TRACKLIST_BUTTON_SIZE * 0.5,
            rect.center().y,
        ),
        Vec2::splat(TRACKLIST_BUTTON_SIZE),
    );
    let resp = ui
        .interact(
            add_rect,
            ui.id().with("add_event_at_playhead"),
            Sense::click(),
        )
        .on_hover_text("Add Animation Event at Playhead");
    if resp.hovered() {
        painter.rect_filled(add_rect, CornerRadius::same(2), theme.scrollbar_hover);
    }
    paint_plus_icon(painter, add_rect.center(), theme.text);

    if resp.clicked() {
        state.event_editing_tick = Some(state.playback.playhead);
        true
    } else {
        false
    }
}

fn draw_summary_header_row(
    ui: &mut egui::Ui,
    width: f32,
    row_height: f32,
    state: &mut EditorState,
    theme: &AceTheme,
) -> bool {
    let mut changed = false;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, row_height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, theme.track_primary_bg);
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, theme.border),
    );

    let title = if state.rig_cache.root_name.is_empty() {
        "Summary".to_string()
    } else {
        format!("Summary ({})", state.rig_cache.root_name)
    };
    painter.text(
        Pos2::new(rect.min.x + INDENT_PADDING, rect.center().y),
        Align2::LEFT_CENTER,
        &title,
        FontId::proportional(theme.font_size_track - 1.0),
        theme.bright_text,
    );

    let add_rect = Rect::from_center_size(
        Pos2::new(
            rect.max.x - TRACKLIST_RIGHT_PADDING - TRACKLIST_BUTTON_SIZE * 0.5,
            rect.center().y,
        ),
        Vec2::splat(TRACKLIST_BUTTON_SIZE),
    );
    let resp = ui
        .interact(add_rect, ui.id().with("add_track_btn"), Sense::click())
        .on_hover_text("Add Track");
    if resp.hovered() {
        painter.rect_filled(add_rect, CornerRadius::same(2), theme.scrollbar_hover);
    }
    paint_plus_icon(painter, add_rect.center(), theme.bright_text);

    let popup_id = ui.id().with("add_track_popup");
    if resp.clicked() {
        egui::Popup::toggle_id(ui.ctx(), popup_id);
    }

    egui::Popup::from_toggle_button_response(&resp)
        .id(popup_id)
        .show(|ui| {
            ui.set_min_width(180.0);
            let unused_body = state.unused_tracks.clone();
            let unused_facs = state.unused_facs.clone();

            if !unused_body.is_empty() {
                if ui.button("Add All Body").clicked() {
                    for name in &unused_body {
                        add_track_to_state(state, "Root", name, TrackType::CFrame);
                    }
                    changed = true;
                    ui.close();
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for name in &unused_body {
                            if ui.button(name).clicked() {
                                add_track_to_state(state, "Root", name, TrackType::CFrame);
                                changed = true;
                                ui.close();
                            }
                        }
                    });
            }

            if !unused_facs.is_empty() {
                if !unused_body.is_empty() {
                    ui.separator();
                }
                if ui.button("Add All Face").clicked() {
                    for name in &unused_facs {
                        add_track_to_state(state, "Root", name, TrackType::Facs);
                    }
                    changed = true;
                    ui.close();
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for name in &unused_facs {
                            if ui.button(name).clicked() {
                                add_track_to_state(state, "Root", name, TrackType::Facs);
                                changed = true;
                                ui.close();
                            }
                        }
                    });
            }

            if unused_body.is_empty() && unused_facs.is_empty() {
                ui.label("No unused tracks available");
            }
        });

    changed
}

fn draw_track_row(
    ui: &mut egui::Ui,
    rect: Rect,
    row: &VisibleTrackRow,
    state: &mut EditorState,
    theme: &AceTheme,
) -> TrackListAction {
    let mut action = TrackListAction::None;
    let row_id = ui.id().with(("track_row", &row.instance, &row.path));

    if row.selected {
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, theme.track_selected_bg);
    }

    let indent_x = rect.min.x + INDENT_PADDING * (row.depth as f32 + 0.5);

    // Expand/collapse chevron for composite tracks
    let arrow_rect = Rect::from_center_size(
        Pos2::new(indent_x + ARROW_SIZE * 0.5, rect.center().y),
        Vec2::splat(ARROW_SIZE + 4.0),
    );
    let mut clicked_arrow = false;
    if row.has_children {
        let arrow_resp = ui.interact(arrow_rect, row_id.with("arrow"), Sense::click());
        paint_chevron(
            ui.painter(),
            arrow_rect.center(),
            row.expanded,
            if row.selected {
                theme.track_selected_text
            } else {
                theme.text
            },
        );
        if arrow_resp.clicked() {
            set_entry_expanded(&mut state.tracks, &row.path, !row.expanded);
            clicked_arrow = true;
            action = TrackListAction::TrackListChanged;
        }
    }

    // Axis color bar for X / Y / Z / Quaternion / Facs
    let mut label_x = indent_x + ARROW_SIZE + 6.0;
    if matches!(
        row.track_type,
        TrackType::Number | TrackType::Angle | TrackType::Quaternion | TrackType::Facs
    ) {
        let axis_color = theme.curve_color_for_track(row.track_type, &row.name);
        let bar_rect = Rect::from_min_size(
            Pos2::new(label_x, rect.min.y + 3.0),
            Vec2::new(3.0, rect.height() - 6.0),
        );
        ui.painter()
            .rect_filled(bar_rect, CornerRadius::same(1), axis_color);
        label_x += 7.0;
    }

    // Right-aligned context button (`...`)
    let ctx_btn_rect = Rect::from_center_size(
        Pos2::new(
            rect.max.x - TRACKLIST_RIGHT_PADDING - TRACKLIST_BUTTON_SIZE * 0.5,
            rect.center().y,
        ),
        Vec2::splat(TRACKLIST_BUTTON_SIZE),
    );

    // Optional inline NumberBox for scalar tracks
    let is_scalar = matches!(
        row.track_type,
        TrackType::Number | TrackType::Angle | TrackType::Facs
    );
    let right_limit = if is_scalar {
        let nb_rect = Rect::from_min_size(
            Pos2::new(ctx_btn_rect.min.x - 4.0 - NUMBERBOX_WIDTH, rect.min.y + 2.0),
            Vec2::new(NUMBERBOX_WIDTH, rect.height() - 4.0),
        );
        if draw_inline_numberbox(ui, nb_rect, row_id.with("nb"), row, state, theme) {
            action = TrackListAction::ValueEdited;
        }
        nb_rect.min.x - 4.0
    } else {
        ctx_btn_rect.min.x - 4.0
    };

    // Track name label
    let label_rect = Rect::from_min_max(
        Pos2::new(label_x, rect.min.y),
        Pos2::new(right_limit.max(label_x), rect.max.y),
    );
    let text_color = if row.has_error {
        theme.dialog_error_text
    } else if row.selected {
        theme.track_selected_text
    } else {
        theme.track_text
    };
    ui.painter().with_clip_rect(label_rect).text(
        Pos2::new(label_rect.min.x, label_rect.center().y),
        Align2::LEFT_CENTER,
        &row.name,
        FontId::proportional(theme.font_size_track - 1.0),
        text_color,
    );

    // Row click selection + right-click context menu
    let row_hit_rect = Rect::from_min_max(
        Pos2::new(
            if row.has_children {
                arrow_rect.max.x
            } else {
                rect.min.x
            },
            rect.min.y,
        ),
        Pos2::new(right_limit.max(label_x), rect.max.y),
    );
    let row_resp = ui.interact(row_hit_rect, row_id.with("hit"), Sense::click());
    if row_resp.clicked() && !clicked_arrow {
        let multi = ui.input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.shift);
        select_track_path(state, row.path.clone(), multi);
        action = TrackListAction::SelectionChanged;
    }

    // Per-track `...` context button
    let ctx_resp = ui
        .interact(ctx_btn_rect, row_id.with("ctx_btn"), Sense::click())
        .on_hover_text("Track Actions");
    if ctx_resp.hovered() {
        ui.painter()
            .rect_filled(ctx_btn_rect, CornerRadius::same(2), theme.scrollbar_hover);
    }
    let dot_color = if row.selected {
        theme.track_selected_text
    } else {
        theme.text
    };
    for dx in [-3.5, 0.0, 3.5] {
        ui.painter().circle_filled(
            Pos2::new(ctx_btn_rect.center().x + dx, ctx_btn_rect.center().y),
            1.1,
            dot_color,
        );
    }

    let menu_popup_id = row_id.with("track_actions_menu");
    if ctx_resp.clicked() {
        egui::Popup::toggle_id(ui.ctx(), menu_popup_id);
    }

    // Both right-click on row and clicking `...` open the TrackActions menu
    let mut menu_changed = false;
    row_resp.context_menu(|ui| {
        if show_track_context_menu_items(ui, row, state) {
            menu_changed = true;
        }
    });
    egui::Popup::from_toggle_button_response(&ctx_resp)
        .id(menu_popup_id)
        .show(|ui| {
            if show_track_context_menu_items(ui, row, state) {
                menu_changed = true;
            }
        });

    if menu_changed {
        action = TrackListAction::TrackListChanged;
    }

    action
}

fn show_track_context_menu_items(
    ui: &mut egui::Ui,
    row: &VisibleTrackRow,
    state: &mut EditorState,
) -> bool {
    let mut changed = false;
    let tick = state.playback.playhead;

    if ui.button("Add Keyframe at Playhead").clicked() {
        let val = sample_track_value_at_playhead(state, &row.instance, &row.path, row.track_type);
        crate::anim_editor::ops::value_changed(
            state,
            &row.instance,
            &row.path,
            row.track_type,
            tick,
            val,
            true,
        );
        changed = true;
        ui.close();
    }

    // Rotation conversion & Euler order options for CFrame / Rotation tracks in channel animations
    let is_channel = state.is_channel_animation();
    let is_rot_or_cframe = matches!(
        row.track_type,
        TrackType::CFrame | TrackType::EulerAngles | TrackType::Quaternion
    );
    if is_channel && is_rot_or_cframe {
        let current_rot_type = state
            .animation_data
            .as_ref()
            .and_then(|d| d.get_track(&row.instance, &row.path))
            .and_then(Track::get_rotation_type);

        if current_rot_type == Some(TrackType::EulerAngles) {
            if ui.button("Convert to Quaternion").clicked() {
                crate::anim_editor::ops::convert_track_rotation(
                    state,
                    &row.instance,
                    &row.path,
                    false,
                );
                changed = true;
                ui.close();
            }
            ui.menu_button("Rotation Order", |ui| {
                let cur_order = row.euler_angles_order.unwrap_or(RotationOrder::XYZ);
                for &order in EULER_ANGLES_ORDER {
                    if ui
                        .selectable_label(cur_order == order, order.as_str())
                        .clicked()
                    {
                        crate::anim_editor::ops::set_track_euler_angles_order(
                            state,
                            &row.instance,
                            &row.path,
                            order,
                        );
                        changed = true;
                        ui.close();
                    }
                }
            });
        } else if current_rot_type == Some(TrackType::Quaternion) {
            if ui.button("Convert to Euler Angles").clicked() {
                crate::anim_editor::ops::convert_track_rotation(
                    state,
                    &row.instance,
                    &row.path,
                    true,
                );
                changed = true;
                ui.close();
            }
        }
    }

    ui.separator();

    if ui.button("Clear Track").clicked() {
        crate::anim_editor::ops::clear_track(state, &row.instance, &row.path);
        changed = true;
        ui.close();
    }

    if row.path.len() == 1 && ui.button("Delete Track").clicked() {
        let track_name = row.path[0].clone();
        crate::anim_editor::ops::delete_track(state, &row.instance, &track_name);
        changed = true;
        ui.close();
    }

    changed
}

/// Inline `NumberBox` with horizontal scrub-drag and click-to-type exact float.
/// Commits changes through `crate::anim_editor::ops::value_changed`.
fn draw_inline_numberbox(
    ui: &mut egui::Ui,
    rect: Rect,
    id: Id,
    row: &VisibleTrackRow,
    state: &mut EditorState,
    theme: &AceTheme,
) -> bool {
    let raw_val = sample_scalar_at_playhead(state, &row.instance, &row.path, row.track_type);
    let is_angle = row.track_type == TrackType::Angle;
    let is_facs = row.track_type == TrackType::Facs;

    let display_num = if is_angle {
        raw_val.to_degrees()
    } else {
        raw_val
    };
    let precision = if is_facs {
        NUMBER_FACS_PRECISION
    } else {
        NUMBER_PRECISION
    };
    let rounded = (display_num * precision).round() / precision;

    let label_w = 16.0;
    let label_rect = Rect::from_min_size(rect.min, Vec2::new(label_w, rect.height()));
    let edit_rect = Rect::from_min_max(Pos2::new(label_rect.max.x, rect.min.y), rect.max);

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(2), theme.background);
    painter.rect_filled(label_rect, CornerRadius::same(2), theme.track_shaded_bg);
    painter.rect_stroke(
        rect,
        CornerRadius::same(2),
        Stroke::new(1.0, theme.border),
        egui::StrokeKind::Middle,
    );

    let axis_label = match row.name.as_str() {
        PROP_X | PROP_Y | PROP_Z => row.name.as_str(),
        _ if is_facs => "V",
        _ => "#",
    };
    let axis_color = theme.curve_color_for_track(row.track_type, &row.name);
    painter.text(
        label_rect.center(),
        Align2::CENTER_CENTER,
        axis_label,
        FontId::proportional(theme.font_size_track - 3.0),
        axis_color,
    );

    // Drag-scrub on the label area
    let drag_resp = ui
        .interact(label_rect, id.with("drag"), Sense::drag())
        .on_hover_cursor(CursorIcon::ResizeHorizontal);
    let mut committed_display: Option<f32> = None;

    if drag_resp.drag_started() {
        state.add_waypoint();
    }
    if drag_resp.dragged() {
        let dx = drag_resp.drag_delta().x;
        if dx.abs() > 0.0 {
            let mult = if is_facs {
                NUMBERBOX_FACS_DRAG_MULTIPLIER
            } else {
                NUMBERBOX_DRAG_MULTIPLIER
            };
            let mut next = display_num + dx * mult;
            if is_facs {
                next = next.clamp(0.0, 1.0);
            }
            committed_display = Some(next);
        }
    }

    // Click-to-type numeric input
    let edit_id = id.with("edit");
    let buf_id = id.with("buf");
    let is_focused = ui.memory(|m| m.has_focus(edit_id));
    let formatted = format_number(rounded, is_facs);
    let mut text = if is_focused {
        ui.data_mut(|d| {
            d.get_temp::<String>(buf_id)
                .unwrap_or_else(|| formatted.clone())
        })
    } else {
        formatted.clone()
    };

    let edit_resp = ui.put(
        edit_rect,
        egui::TextEdit::singleline(&mut text)
            .id(edit_id)
            .font(FontId::proportional(theme.font_size_track - 2.0))
            .text_color(theme.text)
            .margin(egui::Margin::symmetric(3, 1)),
    );

    if edit_resp.gained_focus() || edit_resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(buf_id, text.clone()));
    }
    if edit_resp.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(buf_id));
        if text != formatted {
            if let Ok(mut parsed) = text.trim().parse::<f32>() {
                if parsed.is_finite() {
                    if is_facs {
                        parsed = parsed.clamp(0.0, 1.0);
                    }
                    state.add_waypoint();
                    committed_display = Some(parsed);
                }
            }
        }
    }

    if let Some(new_display) = committed_display {
        let stored_val = if is_angle {
            new_display.to_radians()
        } else {
            new_display
        };
        let kf_val = match row.track_type {
            TrackType::Angle => KeyframeValue::Angle(stored_val),
            TrackType::Facs => KeyframeValue::Facs(stored_val),
            _ => KeyframeValue::Number(stored_val),
        };
        let tick = state.playback.playhead;
        crate::anim_editor::ops::value_changed(
            state,
            &row.instance,
            &row.path,
            row.track_type,
            tick,
            kf_val,
            false,
        );
        return true;
    }

    false
}

fn format_number(v: f32, is_facs: bool) -> String {
    let clean = if v.abs() < 1e-5 { 0.0 } else { v };
    let s = if is_facs {
        format!("{clean:.2}")
    } else {
        format!("{clean:.3}")
    };
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-0" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn sample_scalar_at_playhead(
    state: &EditorState,
    instance: &str,
    path: &[String],
    track_type: TrackType,
) -> f32 {
    let Some(anim) = state.animation_data.as_ref() else {
        return 0.0;
    };
    let Some(track) = anim.get_track(instance, path) else {
        return 0.0;
    };
    let tick = state.playback.playhead;
    if let Some(kf) = track.data.get(&tick) {
        return kf.value.as_f32().unwrap_or(0.0);
    }
    if track.keyframes.is_empty() {
        return KeyframeValue::default_for(track_type)
            .as_f32()
            .unwrap_or(0.0);
    }
    // Linear fallback between bracketing keyframes
    match track.keyframes.binary_search(&tick) {
        Ok(i) => track
            .data
            .get(&track.keyframes[i])
            .and_then(|k| k.value.as_f32())
            .unwrap_or(0.0),
        Err(0) => track
            .data
            .get(&track.keyframes[0])
            .and_then(|k| k.value.as_f32())
            .unwrap_or(0.0),
        Err(i) if i >= track.keyframes.len() => track
            .data
            .get(&track.keyframes[track.keyframes.len() - 1])
            .and_then(|k| k.value.as_f32())
            .unwrap_or(0.0),
        Err(i) => {
            let t0 = track.keyframes[i - 1];
            let t1 = track.keyframes[i];
            let v0 = track
                .data
                .get(&t0)
                .and_then(|k| k.value.as_f32())
                .unwrap_or(0.0);
            let v1 = track
                .data
                .get(&t1)
                .and_then(|k| k.value.as_f32())
                .unwrap_or(0.0);
            let alpha = ((tick - t0) as f32) / ((t1 - t0).max(1) as f32);
            v0 + (v1 - v0) * alpha
        }
    }
}

fn sample_track_value_at_playhead(
    state: &EditorState,
    instance: &str,
    path: &[String],
    track_type: TrackType,
) -> KeyframeValue {
    if matches!(
        track_type,
        TrackType::Number | TrackType::Angle | TrackType::Facs
    ) {
        let v = sample_scalar_at_playhead(state, instance, path, track_type);
        return match track_type {
            TrackType::Angle => KeyframeValue::Angle(v),
            TrackType::Facs => KeyframeValue::Facs(v),
            _ => KeyframeValue::Number(v),
        };
    }
    state
        .animation_data
        .as_ref()
        .and_then(|d| d.get_track(instance, path))
        .and_then(|t| t.data.get(&state.playback.playhead))
        .map(|k| k.value)
        .unwrap_or_else(|| KeyframeValue::default_for(track_type))
}

fn add_track_to_state(state: &mut EditorState, instance: &str, name: &str, track_type: TrackType) {
    if state.tracks.iter().any(|t| t.name == name) {
        return;
    }
    state.add_waypoint();
    let rot_type = state.settings.default_rotation_type;
    let euler_order = state.settings.default_euler_angles_order;
    let is_channel = state.is_channel_animation();

    if let Some(anim) = state.animation_data.as_mut() {
        anim.add_track(instance, name, track_type, rot_type, euler_order);
    }

    let mut entry = TrackListEntry::new(name, instance, track_type, 0);
    if is_channel && track_type == TrackType::CFrame {
        entry.euler_angles_order = Some(euler_order);
        let mut pos = TrackListEntry::new(PROP_POSITION, instance, TrackType::Position, 1);
        for axis in [PROP_X, PROP_Y, PROP_Z] {
            pos.components.insert(
                axis.to_string(),
                TrackListEntry::new(axis, instance, TrackType::Number, 2),
            );
        }
        let mut rot = TrackListEntry::new(PROP_ROTATION, instance, rot_type, 1);
        if rot_type == TrackType::EulerAngles {
            rot.euler_angles_order = Some(euler_order);
            for axis in [PROP_X, PROP_Y, PROP_Z] {
                rot.components.insert(
                    axis.to_string(),
                    TrackListEntry::new(axis, instance, TrackType::Angle, 2),
                );
            }
        }
        entry.components.insert(PROP_POSITION.to_string(), pos);
        entry.components.insert(PROP_ROTATION.to_string(), rot);
    }
    state.tracks.push(entry);
    state.unused_tracks.retain(|t| t != name);
    state.unused_facs.retain(|t| t != name);
}

fn clear_entry_selection(entries: &mut [TrackListEntry]) {
    for entry in entries {
        entry.selected = false;
        for child in entry.components.values_mut() {
            clear_entry_selection(std::slice::from_mut(child));
        }
    }
}

fn select_track_path(state: &mut EditorState, path: TrackPath, multi: bool) {
    if !multi {
        state.selected_tracks.clear();
        clear_entry_selection(&mut state.tracks);
        state.selected_tracks.push(path.clone());
    } else if let Some(idx) = state.selected_tracks.iter().position(|p| p == &path) {
        state.selected_tracks.remove(idx);
    } else {
        state.selected_tracks.push(path.clone());
    }
    state.last_selected_path = Some(path);
    state.sync_selected_track_instances();
}

fn set_entry_expanded(entries: &mut [TrackListEntry], path: &[String], expanded: bool) {
    let Some((first, rest)) = path.split_first() else {
        return;
    };
    for entry in entries {
        if entry.name == *first {
            if rest.is_empty() {
                entry.expanded = expanded;
            } else {
                set_entry_expanded_map(&mut entry.components, rest, expanded);
            }
            return;
        }
    }
}

fn set_entry_expanded_map(
    map: &mut std::collections::BTreeMap<String, TrackListEntry>,
    path: &[String],
    expanded: bool,
) {
    let Some((first, rest)) = path.split_first() else {
        return;
    };
    if let Some(entry) = map.get_mut(first) {
        if rest.is_empty() {
            entry.expanded = expanded;
        } else {
            set_entry_expanded_map(&mut entry.components, rest, expanded);
        }
    }
}

fn paint_plus_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    painter.line_segment(
        [Pos2::new(c.x - 4.0, c.y), Pos2::new(c.x + 4.0, c.y)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(c.x, c.y - 4.0), Pos2::new(c.x, c.y + 4.0)],
        stroke,
    );
}

fn paint_chevron(painter: &egui::Painter, c: Pos2, expanded: bool, color: Color32) {
    let pts = if expanded {
        vec![
            Pos2::new(c.x - 3.5, c.y - 2.0),
            Pos2::new(c.x + 3.5, c.y - 2.0),
            Pos2::new(c.x, c.y + 3.0),
        ]
    } else {
        vec![
            Pos2::new(c.x - 2.0, c.y - 3.5),
            Pos2::new(c.x + 3.0, c.y),
            Pos2::new(c.x - 2.0, c.y + 3.5),
        ]
    };
    painter.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
}

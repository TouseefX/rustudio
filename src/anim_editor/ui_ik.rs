//! Floating Inverse Kinematics management window (`Components/IK/IKWindow`,
//! `IKModeButtons`, `IKTreeView`, `IKTreeRow`, `HierarchyLines`).

use std::collections::HashSet;

use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};

use crate::anim_editor::ik::{can_use_ik, get_active_chain_set, is_part_pinnable, toggle_pin_part};
use crate::anim_editor::state::{EditorState, IkMode, RigTypeKind};
use crate::anim_editor::theme::AceTheme;

const INDENT_WIDTH: f32 = 20.0;
const ROW_HEIGHT: f32 = 22.0;

/// Renders the floating `"Inverse Kinematics"` window when `state.ik.show_tree` is true.
pub fn show_ik_window(ctx: &egui::Context, state: &mut EditorState, theme: &AceTheme) {
    if !state.ik.show_tree {
        return;
    }

    let mut open = state.ik.show_tree;
    egui::Window::new("Inverse Kinematics")
        .id(egui::Id::new("ace_ik_window"))
        .open(&mut open)
        .default_size([260.0, 440.0])
        .min_size([220.0, 260.0])
        .max_height(ctx.content_rect().height().max(260.0) - 40.0)
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            show_ik_window_contents(ui, state, theme);
        });
    state.ik.show_tree = open;
}

fn show_ik_window_contents(ui: &mut egui::Ui, state: &mut EditorState, theme: &AceTheme) {
    let (ik_supported, is_r15) = can_use_ik(&state.rig_cache);

    // Reserve the bottom Enable/Disable IK bar first via `Panel::bottom`
    // so the window height never enters a feedback growth loop.
    egui::Panel::bottom(egui::Id::new("ace_ik_bottom_bar"))
        .frame(egui::Frame::NONE.inner_margin(egui::Margin {
            left: 0,
            right: 0,
            top: 4,
            bottom: 0,
        }))
        .show_inside(ui, |ui| {
            ui.separator();
            ui.horizontal(|ui| {
                let btn_label = if state.ik.enabled {
                    "Disable IK"
                } else {
                    "Enable IK"
                };
                let fill = if state.ik.enabled {
                    theme.track_shaded_bg
                } else {
                    theme.accent
                };
                let text_color = if state.ik.enabled {
                    theme.text
                } else {
                    theme.accent_text
                };
                let btn = ui.add_enabled(
                    ik_supported,
                    egui::Button::new(egui::RichText::new(btn_label).strong().color(text_color))
                        .fill(fill)
                        .min_size(Vec2::new(ui.available_width(), 26.0)),
                );
                if btn.clicked() {
                    state.ik.enabled = !state.ik.enabled;
                    if state.ik.enabled {
                        state.ik.opened_timestamp = Some(ui.ctx().input(|i| i.time));
                        if !is_r15 {
                            state.ik.mode = IkMode::FullBody;
                        }
                    } else {
                        state.ik.opened_timestamp = None;
                    }
                }
            });
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show_inside(ui, |ui| {
            // Header: rig type badge + quick Clear Pins button.
            ui.horizontal(|ui| {
                let rig_label = match state.rig_cache.rig_type {
                    RigTypeKind::R15 => "R15 Rig",
                    RigTypeKind::R6 => "R6 Rig",
                    RigTypeKind::Custom => "Custom Rig",
                };
                let badge_color = if is_r15 {
                    theme.accent
                } else {
                    theme.dimmed_text
                };
                ui.label(
                    egui::RichText::new(rig_label)
                        .small()
                        .strong()
                        .color(badge_color),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let has_pins = !state.ik.pinned_parts.is_empty();
                    let clear_btn = ui.add_enabled(
                        has_pins,
                        egui::Button::new(
                            egui::RichText::new(format!(
                                "Clear Pins ({})",
                                state.ik.pinned_parts.len()
                            ))
                            .small(),
                        ),
                    );
                    if clear_btn.clicked() {
                        state.ik.pinned_parts.clear();
                    }
                });
            });

            ui.separator();

            // Mode selector buttons (`IKModeButtons`): Body Part vs Full Body.
            if state.ik.enabled {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("IK Mode:")
                            .size(12.0)
                            .color(theme.dimmed_text),
                    );

                    let body_part_resp = ui.add_enabled(
                        is_r15,
                        egui::RadioButton::new(state.ik.mode == IkMode::BodyPart, "Body Part"),
                    );
                    if body_part_resp.clicked() {
                        state.ik.mode = IkMode::BodyPart;
                    }
                    body_part_resp.on_hover_text(
                        "Solves individual R15 limbs while automatically pinning the torso \
                         (or pinning both feet when dragging the torso).",
                    );

                    let full_body_resp = ui.add(egui::RadioButton::new(
                        state.ik.mode == IkMode::FullBody,
                        "Full Body",
                    ));
                    if full_body_resp.clicked() {
                        state.ik.mode = IkMode::FullBody;
                    }
                    full_body_resp.on_hover_text(
                        "Solves across the entire rig hierarchy up to the root or any manually pinned parts.",
                    );
                });
                ui.separator();
            }

            if !ik_supported {
                ui.add_space(16.0);
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new(
                            "Inverse Kinematics requires an active rig with Motor6D, \
                             AnimationConstraint, or Bone joints.",
                        )
                        .color(theme.dimmed_text),
                    );
                });
            } else if !state.ik.enabled {
                ui.add_space(16.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("IK is currently disabled.")
                            .strong()
                            .color(theme.text),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(
                            "Enable IK below to pose limbs naturally using Inverse Kinematics \
                             and pin parts in the rig hierarchy.",
                        )
                        .small()
                        .color(theme.dimmed_text),
                    );
                });
            } else {
                show_ik_tree_view(ui, state, theme);
            }
        });
}

#[derive(Clone)]
struct TreeRow {
    part_name: String,
    depth: usize,
    has_children: bool,
    is_expanded: bool,
    /// For each ancestor column `0..depth`, whether a vertical continuation line is needed.
    ancestor_has_more: Vec<bool>,
    is_last_sibling: bool,
}

fn show_ik_tree_view(ui: &mut egui::Ui, state: &mut EditorState, theme: &AceTheme) {
    let selected_part = state
        .selected_tracks
        .first()
        .and_then(|p| p.first())
        .map(String::as_str);

    let active_chain = get_active_chain_set(&state.rig_cache, &state.ik, selected_part);

    let root_name = state
        .rig_cache
        .root_part_name
        .clone()
        .or_else(|| state.rig_cache.hierarchy_order.first().cloned());

    let Some(root_name) = root_name else {
        ui.label(egui::RichText::new("No rig hierarchy detected.").color(theme.dimmed_text));
        return;
    };

    let collapse_id = egui::Id::new("ace_ik_collapsed_nodes");
    let mut collapsed: HashSet<String> = ui
        .ctx()
        .data(|d| d.get_temp::<HashSet<String>>(collapse_id))
        .unwrap_or_default();

    let mut rows = Vec::new();
    let mut visited = HashSet::new();
    collect_tree_rows(
        &state.rig_cache.children_map,
        &collapsed,
        &root_name,
        0,
        &[],
        true,
        &mut visited,
        &mut rows,
    );

    let mut toggle_collapse: Option<String> = None;
    let mut select_part: Option<String> = None;
    let mut pin_part: Option<String> = None;

    let selected_name = selected_part.map(str::to_string);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in &rows {
                let is_selected = selected_name.as_deref() == Some(row.part_name.as_str());
                let is_in_chain = active_chain.contains(&row.part_name);
                let is_pinned = state.ik.pinned_parts.contains(&row.part_name);
                let pinnable = is_part_pinnable(&state.rig_cache, &row.part_name);

                let (rect, resp) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), ROW_HEIGHT),
                    Sense::click(),
                );

                // Row background
                if is_selected {
                    ui.painter().rect_filled(rect, 2.0, theme.track_selected_bg);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 2.0, theme.track_shaded_bg);
                }

                // Paint HierarchyLines (vertical + horizontal connectors)
                paint_hierarchy_lines(ui.painter(), rect, row, is_in_chain, theme);

                let content_x = rect.min.x + (row.depth as f32) * INDENT_WIDTH + 6.0;
                let center_y = rect.center().y;

                // Expand/collapse toggle (12x12)
                let mut label_x = content_x;
                if row.has_children {
                    let toggle_rect = Rect::from_center_size(
                        Pos2::new(content_x + 6.0, center_y),
                        Vec2::splat(14.0),
                    );
                    let toggle_resp = ui.interact(
                        toggle_rect,
                        ui.id().with(("ik_expand", &row.part_name)),
                        Sense::click(),
                    );
                    let arrow = if row.is_expanded { "▾" } else { "▸" };
                    ui.painter().text(
                        toggle_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        arrow,
                        egui::FontId::proportional(11.0),
                        if is_selected {
                            theme.track_selected_text
                        } else {
                            theme.dimmed_text
                        },
                    );
                    if toggle_resp.clicked() {
                        toggle_collapse = Some(row.part_name.clone());
                    }
                    label_x += 16.0;
                } else {
                    label_x += 10.0;
                }

                // Part label
                let label_color = if is_selected {
                    theme.track_selected_text
                } else if is_in_chain {
                    theme.bright_text
                } else {
                    theme.track_text
                };
                ui.painter().text(
                    Pos2::new(label_x, center_y),
                    egui::Align2::LEFT_CENTER,
                    &row.part_name,
                    egui::FontId::proportional(12.5),
                    label_color,
                );

                // Right-side Pin / Unpin button in FullBody mode (or auto-pin badge in BodyPart).
                if pinnable {
                    let pin_rect = Rect::from_center_size(
                        Pos2::new(rect.max.x - 16.0, center_y),
                        Vec2::new(22.0, 16.0),
                    );
                    if state.ik.mode == IkMode::FullBody {
                        let pin_resp = ui.interact(
                            pin_rect,
                            ui.id().with(("ik_pin", &row.part_name)),
                            Sense::click(),
                        );
                        let pin_bg = if is_pinned {
                            theme.accent
                        } else if pin_resp.hovered() {
                            theme.scrollbar_hover
                        } else {
                            Color32::TRANSPARENT
                        };
                        if pin_bg != Color32::TRANSPARENT {
                            ui.painter().rect_filled(pin_rect, 3.0, pin_bg);
                        }
                        ui.painter().text(
                            pin_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            if is_pinned { "●" } else { "○" },
                            egui::FontId::proportional(11.0),
                            if is_pinned {
                                theme.accent_text
                            } else {
                                theme.dimmed_text
                            },
                        );
                        if pin_resp.clicked() {
                            pin_part = Some(row.part_name.clone());
                        }
                        pin_resp.on_hover_text(if is_pinned {
                            "Unpin part"
                        } else {
                            "Pin part in place during Full Body IK"
                        });
                    } else if is_pinned {
                        ui.painter().text(
                            pin_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "●",
                            egui::FontId::proportional(10.0),
                            theme.dimmed_text,
                        );
                    }
                }

                if resp.clicked() {
                    select_part = Some(row.part_name.clone());
                }
            }
        });

    if let Some(name) = toggle_collapse {
        if !collapsed.remove(&name) {
            collapsed.insert(name);
        }
        ui.ctx().data_mut(|d| d.insert_temp(collapse_id, collapsed));
    }
    if let Some(part) = select_part {
        state.selected_tracks = vec![vec![part]];
        state.sync_selected_track_instances();
    }
    if let Some(part) = pin_part {
        toggle_pin_part(state, &part);
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_tree_rows(
    children_map: &std::collections::HashMap<String, Vec<String>>,
    collapsed: &HashSet<String>,
    part_name: &str,
    depth: usize,
    ancestor_has_more: &[bool],
    is_last_sibling: bool,
    visited: &mut HashSet<String>,
    out: &mut Vec<TreeRow>,
) {
    if !visited.insert(part_name.to_string()) {
        return;
    }
    let children = children_map
        .get(part_name)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let has_children = !children.is_empty();
    let is_expanded = !collapsed.contains(part_name);

    out.push(TreeRow {
        part_name: part_name.to_string(),
        depth,
        has_children,
        is_expanded,
        ancestor_has_more: ancestor_has_more.to_vec(),
        is_last_sibling,
    });

    if has_children && is_expanded {
        let mut next_ancestors = ancestor_has_more.to_vec();
        if depth > 0 {
            next_ancestors.push(!is_last_sibling);
        }
        for (idx, child) in children.iter().enumerate() {
            let child_is_last = idx + 1 == children.len();
            collect_tree_rows(
                children_map,
                collapsed,
                child,
                depth + 1,
                &next_ancestors,
                child_is_last,
                visited,
                out,
            );
        }
    }
}

/// Paints crisp vertical and horizontal connector lines (`Components/IK/HierarchyLines`).
fn paint_hierarchy_lines(
    painter: &egui::Painter,
    rect: Rect,
    row: &TreeRow,
    is_in_chain: bool,
    theme: &AceTheme,
) {
    if row.depth == 0 {
        return;
    }

    let inactive_stroke = Stroke::new(1.0, theme.dimmed_text.gamma_multiply(0.55));
    let active_stroke = Stroke::new(2.0, theme.accent);

    // Ancestor continuation vertical lines.
    for (col, &has_more) in row.ancestor_has_more.iter().enumerate() {
        if has_more {
            let x = rect.min.x + (col as f32) * INDENT_WIDTH + 12.0;
            painter.line_segment(
                [Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)],
                inactive_stroke,
            );
        }
    }

    // Current depth L/T connector.
    let col_x = rect.min.x + ((row.depth - 1) as f32) * INDENT_WIDTH + 12.0;
    let mid_y = rect.center().y;
    let stroke = if is_in_chain {
        active_stroke
    } else {
        inactive_stroke
    };

    // Vertical segment from top to center (and to bottom if not last sibling).
    let bottom_y = if row.is_last_sibling {
        mid_y
    } else {
        rect.max.y
    };
    painter.line_segment(
        [Pos2::new(col_x, rect.min.y), Pos2::new(col_x, bottom_y)],
        stroke,
    );

    // Horizontal segment from vertical stem to the node label/toggle.
    painter.line_segment(
        [
            Pos2::new(col_x, mid_y),
            Pos2::new(col_x + INDENT_WIDTH * 0.55, mid_y),
        ],
        stroke,
    );
}

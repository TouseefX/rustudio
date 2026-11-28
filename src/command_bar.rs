//! Modular [`NativePlugin`] implementing Roblox Studio 0.748's **Command Bar**.
//!
//! Reverse-engineered directly from Studio 0.748 (`checks/ui/studio/window-output.png`
//! and `STUDIO-PANELS.md`):
//! - Full-width slim bottom strip (36 px default, vertically resizable for multi-line
//!   snippets) docked directly above the status bar and below all dock panes.
//! - Left 3-dot grip (`rgb(100, 102, 110)`), dark input well (`rgb(32, 34, 39)`)
//!   with monospace Luau syntax highlighting (`crate::script_editor::highlight`),
//!   and right pill cluster (`rgb(19, 19, 22)`) holding:
//!   1. Saved commands (bookmark) popup
//!   2. Command history (clock-arrow) popup
//!   3. Run button (play triangle + "Run", disabled muted state when blank)
//! - Shortcuts: `Ctrl+9` opens & focuses; `Enter` (single-line) or `Ctrl+Enter`
//!   (always) runs at `security::COMMAND` identity; `Shift+Enter` inserts newline;
//!   `Ctrl+Up/Down` (or `Up/Down` on single-line) cycles command history.

use std::any::Any;
use std::path::{Path, PathBuf};

use egui::{Align, Color32, CornerRadius, FontId, Key, Layout, Margin, Rect, RichText, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};

use crate::plugin_host::native::{
    NativeHostCtx, NativePlugin, NativePluginEntry, NativePluginMeta, NativePluginRequest,
};
use rustudio_engine::theme;

/// Stable plugin id under [`crate::plugin_host::PluginHost`].
pub const PLUGIN_ID: &str = "rustudio_CommandBar";
/// Shell capability / ribbon action id owned by this plugin.
pub const CAPABILITY: &str = "command_bar";

const MAX_HISTORY: usize = 100;
const DEFAULT_HEIGHT: f32 = 36.0;
const MIN_HEIGHT: f32 = 36.0;
const MAX_HEIGHT: f32 = 220.0;

// Exact dark-theme colors measured from checks/ui/studio/window-output.png (y=822..858).
const BAR_BG: Color32 = Color32::from_rgb(25, 26, 31);
const BAR_TOP_BORDER: Color32 = Color32::from_rgb(53, 55, 65);
const GRIP_DOT: Color32 = Color32::from_rgb(100, 102, 110);
const INPUT_BG: Color32 = Color32::from_rgb(32, 34, 39);
const PLACEHOLDER_FG: Color32 = Color32::from_rgb(112, 115, 124);
const CLUSTER_BG: Color32 = Color32::from_rgb(19, 19, 22);
const ICON_BTN_BG: Color32 = Color32::from_rgb(41, 43, 49);
const ICON_BTN_HOVER: Color32 = Color32::from_rgb(54, 57, 66);
const ICON_DIM: Color32 = Color32::from_rgb(104, 107, 116);
const ICON_BRIGHT: Color32 = Color32::from_rgb(232, 234, 238);
const RUN_DISABLED_BG: Color32 = Color32::from_rgb(30, 31, 36);
const RUN_DISABLED_FG: Color32 = Color32::from_rgb(132, 136, 145);
const RUN_ENABLED_BG: Color32 = Color32::from_rgb(48, 52, 62);
const RUN_ENABLED_HOVER: Color32 = Color32::from_rgb(60, 66, 78);
const POPUP_BG: Color32 = Color32::from_rgb(30, 32, 38);
const POPUP_BORDER: Color32 = Color32::from_rgb(58, 61, 72);

/// Static manifest for RusTudio's modular Command Bar plugin.
pub const META: NativePluginMeta = NativePluginMeta {
    id: PLUGIN_ID,
    display_name: "Command Bar",
    description: "Run Luau commands directly in Edit or Play mode with history and saved snippets (Ctrl+9)",
    builtin_tool: None,
    capability: Some(CAPABILITY),
    widget: None,
    has_bottom_bar: true,
};

/// Factory entry registering [`CommandBarPlugin`] in the Built-in Suite.
pub fn plugin_entry() -> NativePluginEntry {
    NativePluginEntry {
        meta: META,
        factory: Box::new(|| Box::new(CommandBarPlugin::default())),
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct PersistedCommandBar {
    #[serde(default = "default_open_true")]
    open: bool,
    #[serde(default)]
    history: Vec<String>,
    #[serde(default)]
    bookmarks: Vec<String>,
}

fn default_open_true() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActivePopup {
    None,
    History,
    Bookmarks,
}

/// Modular Roblox Studio 0.748 Command Bar plugin.
pub struct CommandBarPlugin {
    /// Whether the bottom Command Bar strip is currently visible.
    pub open: bool,
    /// Current Luau command text in the editor field.
    pub input: String,
    /// Recent executed commands (oldest first, newest at the end).
    pub history: Vec<String>,
    /// Bookmarked reusable commands.
    pub bookmarks: Vec<String>,
    /// Current cursor inside `history` when browsing with Up/Down (`None` = editing live draft).
    history_idx: Option<usize>,
    /// Saved unsent draft while stepping through history.
    draft: String,
    /// User-dragged bar height (clamped to `MIN_HEIGHT..=MAX_HEIGHT`).
    bar_height: f32,
    /// Request keyboard focus on the next frame (e.g. after Ctrl+9 or opening).
    request_focus: bool,
    /// Which right-cluster popup is open, if any.
    popup: ActivePopup,
    /// Search filter inside the History popup.
    history_filter: String,
    /// Whether persisted state has been loaded from disk for this session.
    initialized: bool,
    /// Path to `command_bar.json` when persistence is active.
    state_path: Option<PathBuf>,
}

impl Default for CommandBarPlugin {
    fn default() -> Self {
        Self {
            open: false,
            input: String::new(),
            history: Vec::new(),
            bookmarks: Vec::new(),
            history_idx: None,
            draft: String::new(),
            bar_height: DEFAULT_HEIGHT,
            request_focus: false,
            popup: ActivePopup::None,
            history_filter: String::new(),
            initialized: false,
            state_path: None,
        }
    }
}

impl CommandBarPlugin {
    fn default_state_path() -> PathBuf {
        rustudio_core::paths::config()
            .join("rustudio")
            .join("command_bar.json")
    }

    fn ensure_initialized(&mut self, interactive: bool) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        if interactive && std::env::var_os("RUSTUDIO_DETERMINISTIC").is_none() {
            let path = Self::default_state_path();
            let loaded = load_state(&path);
            self.open = loaded.open;
            self.history = loaded.history;
            self.bookmarks = loaded.bookmarks;
            self.state_path = Some(path);
        }
    }

    fn save(&self) {
        let Some(path) = &self.state_path else {
            return;
        };
        let state = PersistedCommandBar {
            open: self.open,
            history: self.history.clone(),
            bookmarks: self.bookmarks.clone(),
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(&state) {
            let _ = std::fs::write(path, json);
        }
    }

    /// Pushes `code` into history (deduplicating the most recent matching entry),
    /// resets the browsing cursor, and returns a `RunLuau` request.
    pub fn submit(&mut self, code: String) -> Option<NativePluginRequest> {
        let trimmed = code.trim();
        if trimmed.is_empty() {
            return None;
        }
        let cmd = code.trim_end_matches(['\r', '\n']).to_string();
        if let Some(pos) = self.history.iter().position(|h| h == &cmd) {
            self.history.remove(pos);
        }
        self.history.push(cmd.clone());
        if self.history.len() > MAX_HISTORY {
            let drop_n = self.history.len() - MAX_HISTORY;
            self.history.drain(0..drop_n);
        }
        self.history_idx = None;
        self.draft.clear();
        self.popup = ActivePopup::None;
        self.save();
        Some(NativePluginRequest::RunLuau { code: cmd })
    }

    /// Steps backward (older) in command history.
    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next = match self.history_idx {
            None => {
                self.draft = self.input.clone();
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.history_idx = Some(next);
        self.input = self.history[next].clone();
    }

    /// Steps forward (newer) in command history, restoring the draft past the end.
    pub fn history_next(&mut self) {
        let Some(i) = self.history_idx else {
            return;
        };
        if i + 1 < self.history.len() {
            let next = i + 1;
            self.history_idx = Some(next);
            self.input = self.history[next].clone();
        } else {
            self.history_idx = None;
            self.input = std::mem::take(&mut self.draft);
        }
    }

    /// Toggles bookmark state for `cmd`.
    pub fn toggle_bookmark(&mut self, cmd: &str) {
        let trimmed = cmd.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Some(pos) = self.bookmarks.iter().position(|b| b == trimmed) {
            self.bookmarks.remove(pos);
        } else {
            self.bookmarks.insert(0, trimmed.to_string());
        }
        self.save();
    }

    pub fn is_bookmarked(&self, cmd: &str) -> bool {
        let trimmed = cmd.trim();
        !trimmed.is_empty() && self.bookmarks.iter().any(|b| b == trimmed)
    }

    fn desired_bar_height(&self) -> f32 {
        let lines = self.input.lines().count().max(1);
        let auto_h = if lines <= 1 {
            DEFAULT_HEIGHT
        } else {
            (18.0 * lines as f32 + 16.0).clamp(DEFAULT_HEIGHT, MAX_HEIGHT)
        };
        self.bar_height.max(auto_h).clamp(MIN_HEIGHT, MAX_HEIGHT)
    }

    fn paint_grip(painter: &egui::Painter, rect: Rect) {
        let cx = rect.left() + 5.0;
        let cy = rect.center().y;
        for dy in [-4.0, 0.0, 4.0] {
            let dot = Rect::from_center_size(egui::pos2(cx, cy + dy), Vec2::splat(2.0));
            painter.rect_filled(dot, 0.0, GRIP_DOT);
        }
    }

    fn show_history_popup(
        &mut self,
        ctx: &egui::Context,
        anchor: Rect,
        requests: &mut Vec<NativePluginRequest>,
    ) {
        let width = 460.0_f32.min(ctx.content_rect().width() - 24.0);
        let max_h = 300.0_f32;
        let x = (anchor.right() - width).max(8.0);
        let y = (anchor.top() - 6.0).max(40.0);
        let mut close = false;
        let mut pick: Option<(String, bool)> = None;

        let area = egui::Area::new(egui::Id::new("command_bar_history_popup"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(egui::pos2(x, y))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(POPUP_BG)
                    .stroke(Stroke::new(1.0, POPUP_BORDER))
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Command History").strong().size(12.5).color(theme::TEXT));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if !self.history.is_empty() && ui.small_button("Clear").clicked() {
                                    self.history.clear();
                                    self.history_idx = None;
                                    self.save();
                                }
                            });
                        });
                        if self.history.len() > 5 {
                            ui.add_space(4.0);
                            ui.add(
                                egui::TextEdit::singleline(&mut self.history_filter)
                                    .hint_text("Filter history...")
                                    .desired_width(width - 4.0),
                            );
                        }
                        ui.add_space(4.0);
                        ui.separator();
                        if self.history.is_empty() {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("No commands run yet.")
                                    .small()
                                    .color(theme::TEXT_DIM),
                            );
                            ui.add_space(4.0);
                            return;
                        }
                        let q = self.history_filter.to_ascii_lowercase();
                        egui::ScrollArea::vertical()
                            .max_height(max_h)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                for cmd in self.history.iter().rev() {
                                    if !q.is_empty() && !cmd.to_ascii_lowercase().contains(&q) {
                                        continue;
                                    }
                                    let preview = one_line_preview(cmd, 80);
                                    ui.horizontal(|ui| {
                                        let row_w = (ui.available_width() - 28.0).max(120.0);
                                        let (r, resp) = ui.allocate_exact_size(Vec2::new(row_w, 22.0), Sense::click());
                                        if resp.hovered() {
                                            ui.painter().rect_filled(r, 3.0, ICON_BTN_HOVER);
                                        }
                                        ui.painter().text(
                                            egui::pos2(r.left() + 6.0, r.center().y),
                                            egui::Align2::LEFT_CENTER,
                                            preview,
                                            FontId::monospace(12.0),
                                            theme::TEXT,
                                        );
                                        if resp.clicked() {
                                            pick = Some((cmd.clone(), false));
                                        }
                                        if resp.double_clicked() {
                                            pick = Some((cmd.clone(), true));
                                        }
                                        let (pr, presp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                                        if presp.hovered() {
                                            ui.painter().rect_filled(pr, 3.0, ICON_BTN_HOVER);
                                        }
                                        rustudio_engine::ti::paint(
                                            ui.painter(),
                                            pr.shrink(5.0),
                                            "player-play-filled",
                                            if presp.hovered() { ICON_BRIGHT } else { ICON_DIM },
                                        );
                                        if presp.on_hover_text("Run command").clicked() {
                                            pick = Some((cmd.clone(), true));
                                        }
                                    });
                                }
                            });
                    });
            });

        if let Some((cmd, run_now)) = pick {
            self.input = cmd.clone();
            self.history_idx = None;
            self.request_focus = true;
            close = true;
            if run_now {
                if let Some(req) = self.submit(cmd) {
                    requests.push(req);
                }
            }
        }
        let esc = ctx.input(|i| i.key_pressed(Key::Escape));
        let clicked_out = ctx.input(|i| i.pointer.any_pressed())
            && ctx.pointer_interact_pos().is_some_and(|p| !area.response.rect.contains(p) && !anchor.contains(p));
        if close || esc || clicked_out {
            self.popup = ActivePopup::None;
        }
    }

    fn show_bookmarks_popup(
        &mut self,
        ctx: &egui::Context,
        anchor: Rect,
        requests: &mut Vec<NativePluginRequest>,
    ) {
        let width = 460.0_f32.min(ctx.content_rect().width() - 24.0);
        let max_h = 280.0_f32;
        let x = (anchor.right() - width).max(8.0);
        let y = (anchor.top() - 6.0).max(40.0);
        let mut close = false;
        let mut pick: Option<(String, bool)> = None;
        let mut delete_idx: Option<usize> = None;
        let current_non_empty = !self.input.trim().is_empty();
        let current_saved = self.is_bookmarked(&self.input);

        let area = egui::Area::new(egui::Id::new("command_bar_bookmarks_popup"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(egui::pos2(x, y))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(POPUP_BG)
                    .stroke(Stroke::new(1.0, POPUP_BORDER))
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Saved Commands").strong().size(12.5).color(theme::TEXT));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let label = if current_saved { "Remove Current" } else { "+ Save Current" };
                                if ui.add_enabled(current_non_empty, egui::Button::new(label).small()).clicked() {
                                    let cur = self.input.clone();
                                    self.toggle_bookmark(&cur);
                                }
                            });
                        });
                        ui.add_space(4.0);
                        ui.separator();
                        if self.bookmarks.is_empty() {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("No saved commands yet. Type a command and click + Save Current.")
                                    .small()
                                    .color(theme::TEXT_DIM),
                            );
                            ui.add_space(4.0);
                            return;
                        }
                        egui::ScrollArea::vertical()
                            .max_height(max_h)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                for (idx, cmd) in self.bookmarks.iter().enumerate() {
                                    let preview = one_line_preview(cmd, 74);
                                    ui.horizontal(|ui| {
                                        let row_w = (ui.available_width() - 54.0).max(120.0);
                                        let (r, resp) = ui.allocate_exact_size(Vec2::new(row_w, 22.0), Sense::click());
                                        if resp.hovered() {
                                            ui.painter().rect_filled(r, 3.0, ICON_BTN_HOVER);
                                        }
                                        ui.painter().text(
                                            egui::pos2(r.left() + 6.0, r.center().y),
                                            egui::Align2::LEFT_CENTER,
                                            preview,
                                            FontId::monospace(12.0),
                                            theme::TEXT,
                                        );
                                        if resp.clicked() {
                                            pick = Some((cmd.clone(), false));
                                        }
                                        if resp.double_clicked() {
                                            pick = Some((cmd.clone(), true));
                                        }
                                        let (pr, presp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                                        if presp.hovered() {
                                            ui.painter().rect_filled(pr, 3.0, ICON_BTN_HOVER);
                                        }
                                        rustudio_engine::ti::paint(
                                            ui.painter(),
                                            pr.shrink(5.0),
                                            "player-play-filled",
                                            if presp.hovered() { ICON_BRIGHT } else { ICON_DIM },
                                        );
                                        if presp.on_hover_text("Run command").clicked() {
                                            pick = Some((cmd.clone(), true));
                                        }
                                        let (dr, dresp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                                        if dresp.hovered() {
                                            ui.painter().rect_filled(dr, 3.0, ICON_BTN_HOVER);
                                        }
                                        rustudio_engine::ti::paint(
                                            ui.painter(),
                                            dr.shrink(5.0),
                                            "trash",
                                            if dresp.hovered() { theme::ERROR } else { ICON_DIM },
                                        );
                                        if dresp.on_hover_text("Delete saved command").clicked() {
                                            delete_idx = Some(idx);
                                        }
                                    });
                                }
                            });
                    });
            });

        if let Some(i) = delete_idx {
            if i < self.bookmarks.len() {
                self.bookmarks.remove(i);
                self.save();
            }
        }
        if let Some((cmd, run_now)) = pick {
            self.input = cmd.clone();
            self.history_idx = None;
            self.request_focus = true;
            close = true;
            if run_now {
                if let Some(req) = self.submit(cmd) {
                    requests.push(req);
                }
            }
        }
        let esc = ctx.input(|i| i.key_pressed(Key::Escape));
        let clicked_out = ctx.input(|i| i.pointer.any_pressed())
            && ctx.pointer_interact_pos().is_some_and(|p| !area.response.rect.contains(p) && !anchor.contains(p));
        if close || esc || clicked_out {
            self.popup = ActivePopup::None;
        }
    }
}

fn load_state(path: &Path) -> PersistedCommandBar {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn one_line_preview(cmd: &str, max_chars: usize) -> String {
    let flat: String = cmd
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ; ");
    let mut out = String::new();
    for (i, ch) in flat.chars().enumerate() {
        if i >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

impl NativePlugin for CommandBarPlugin {
    fn meta(&self) -> NativePluginMeta {
        META
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn tick(&mut self, ctx: &mut NativeHostCtx<'_>, _widget_open: bool) {
        self.ensure_initialized(ctx.interactive);
    }

    fn is_capability_active(&self, _widget_open: bool) -> bool {
        self.open
    }

    fn on_capability_trigger(&mut self, ctx: &mut NativeHostCtx<'_>) {
        self.ensure_initialized(ctx.interactive);
        self.open = !self.open;
        if self.open {
            self.request_focus = true;
        } else {
            self.popup = ActivePopup::None;
        }
        self.save();
    }

    fn render_bottom_bar(&mut self, ui: &mut egui::Ui, host_ctx: &mut NativeHostCtx<'_>) {
        self.ensure_initialized(host_ctx.interactive);
        let ctx = ui.ctx().clone();

        // Global Ctrl+9 shortcut (Studio 0.748: opens if hidden and focuses Command Bar).
        let focus_shortcut = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::Num9));
        if focus_shortcut {
            if !self.open {
                self.open = true;
                self.save();
            }
            self.request_focus = true;
        }

        if !self.open {
            return;
        }

        let height = self.desired_bar_height();
        let mut run_clicked = false;
        let mut history_anchor: Option<Rect> = None;
        let mut bookmark_anchor: Option<Rect> = None;
        let input_id = egui::Id::new("rustudio_command_bar_input");

        let panel_resp = egui::Panel::bottom("command_bar_strip")
            .exact_size(height)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(BAR_BG)
                    .inner_margin(Margin {
                        left: 0,
                        right: 4,
                        top: 1,
                        bottom: 0,
                    }),
            )
            .show(ui, |ui| {
                let full = ui.max_rect();
                // Exact 1 px top border matching Studio 0.748 (y=822 rgb(53,55,65)).
                ui.painter().line_segment(
                    [egui::pos2(full.left(), full.top()), egui::pos2(full.right(), full.top())],
                    Stroke::new(1.0, BAR_TOP_BORDER),
                );
                // Top-edge drag handle for expanding into multi-line mode (double-click resets).
                let resize_rect = Rect::from_min_size(full.min, Vec2::new(full.width(), 4.0));
                let resize_resp = ui
                    .interact(resize_rect, egui::Id::new("command_bar_resize"), Sense::click_and_drag())
                    .on_hover_cursor(egui::CursorIcon::ResizeVertical);
                if resize_resp.dragged() {
                    let dy = ui.input(|i| i.pointer.delta().y);
                    self.bar_height = (self.bar_height - dy).clamp(MIN_HEIGHT, MAX_HEIGHT);
                }
                if resize_resp.double_clicked() {
                    self.bar_height = DEFAULT_HEIGHT;
                }

                Self::paint_grip(ui.painter(), full);

                let play_pill_w = if host_ctx.play_side.is_some() { 54.0 } else { 0.0 };
                let cluster_w = 132.0 + play_pill_w;
                let left_pad = 11.0;
                let gap = 4.0;
                let input_w = (full.width() - left_pad - cluster_w - gap - 4.0).max(120.0);
                let input_h = (full.height() - 9.0).max(24.0);
                let input_rect = Rect::from_min_size(
                    egui::pos2(full.left() + left_pad, full.top() + 5.0),
                    Vec2::new(input_w, input_h),
                );

                // Inspect keys BEFORE TextEdit consumes Enter / Up / Down when focused.
                let is_focused = ctx.memory(|m| m.has_focus(input_id));
                let is_multiline = self.input.contains('\n');
                let mut history_step: i8 = 0;
                if is_focused {
                    let ctrl_enter = ctx.input_mut(|i| {
                        i.consume_key(egui::Modifiers::COMMAND, Key::Enter)
                            || i.consume_key(egui::Modifiers::CTRL, Key::Enter)
                    });
                    let plain_enter = !is_multiline
                        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
                    if ctrl_enter || plain_enter {
                        run_clicked = true;
                    }
                    let up = ctx.input_mut(|i| {
                        i.consume_key(egui::Modifiers::CTRL, Key::ArrowUp)
                            || i.consume_key(egui::Modifiers::COMMAND, Key::ArrowUp)
                            || (!is_multiline && i.consume_key(egui::Modifiers::NONE, Key::ArrowUp))
                    });
                    let down = ctx.input_mut(|i| {
                        i.consume_key(egui::Modifiers::CTRL, Key::ArrowDown)
                            || i.consume_key(egui::Modifiers::COMMAND, Key::ArrowDown)
                            || (!is_multiline && i.consume_key(egui::Modifiers::NONE, Key::ArrowDown))
                    });
                    if up {
                        history_step = -1;
                    } else if down {
                        history_step = 1;
                    }
                }
                if history_step < 0 {
                    self.history_prev();
                } else if history_step > 0 {
                    self.history_next();
                }

                // Draw Studio's input well (`rgb(32, 34, 39)`).
                let stroke = if is_focused {
                    Stroke::new(1.0, theme::ACCENT)
                } else {
                    Stroke::NONE
                };
                ui.painter().rect(
                    input_rect,
                    CornerRadius::same(3),
                    INPUT_BG,
                    stroke,
                    egui::StrokeKind::Inside,
                );

                let inner_input = input_rect.shrink2(Vec2::new(6.0, 4.0));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner_input));
                child.set_clip_rect(inner_input);
                let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                    let font = FontId::monospace(13.0);
                    let mut job = crate::script_editor::highlight(text.as_str(), None, font);
                    job.wrap.max_width = wrap_width;
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                let edit_resp = egui::ScrollArea::vertical()
                    .id_salt("command_bar_scroll")
                    .max_height(inner_input.height())
                    .show(&mut child, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.input)
                                .id(input_id)
                                .frame(egui::Frame::NONE)
                                .desired_width(inner_input.width())
                                .desired_rows(1)
                                .font(FontId::monospace(13.0))
                                .hint_text(
                                    RichText::new(
                                        "Execute a command with Ctrl+Enter or use Ctrl+Up/Down for history",
                                    )
                                    .font(FontId::monospace(12.5))
                                    .color(PLACEHOLDER_FG),
                                )
                                .layouter(&mut layouter),
                        )
                    })
                    .inner;
                if edit_resp.changed() {
                    self.history_idx = None;
                }
                if self.request_focus {
                    edit_resp.request_focus();
                    self.request_focus = false;
                }
                if is_focused && ctx.input(|i| i.key_pressed(Key::Escape)) && self.popup == ActivePopup::None {
                    edit_resp.surrender_focus();
                }

                // Right action pill cluster (`rgb(19, 19, 22)`), vertically centered in the 36px strip.
                let cluster_h = 30.0;
                let cluster_rect = Rect::from_min_size(
                    egui::pos2(input_rect.right() + gap, full.top() + (full.height() - cluster_h) * 0.5 + 0.5),
                    Vec2::new(cluster_w, cluster_h),
                );
                ui.painter().rect_filled(cluster_rect, CornerRadius::same(4), CLUSTER_BG);

                let mut x = cluster_rect.left() + 4.0;
                let btn_y = cluster_rect.top() + 2.0;
                let btn_h = 26.0;

                // Optional Play-mode DataModel badge (`Server` / `Client`).
                if let Some(side) = host_ctx.play_side {
                    let badge = Rect::from_min_size(egui::pos2(x, btn_y + 2.0), Vec2::new(48.0, btn_h - 4.0));
                    ui.painter().rect_filled(badge, CornerRadius::same(3), theme::ACCENT.gamma_multiply(0.25));
                    ui.painter().text(
                        badge.center(),
                        egui::Align2::CENTER_CENTER,
                        side,
                        FontId::proportional(11.0),
                        theme::ACCENT,
                    );
                    x += 52.0;
                }

                // 1. Saved commands (bookmark) icon button.
                let bm_rect = Rect::from_min_size(egui::pos2(x, btn_y), Vec2::splat(btn_h));
                let bm_resp = ui
                    .interact(bm_rect, egui::Id::new("command_bar_bookmarks"), Sense::click())
                    .on_hover_text("Saved Commands");
                let bm_active = self.popup == ActivePopup::Bookmarks || self.is_bookmarked(&self.input);
                if bm_active {
                    ui.painter().rect_filled(bm_rect, CornerRadius::same(4), ICON_BTN_BG);
                } else if bm_resp.hovered() {
                    ui.painter().rect_filled(bm_rect, CornerRadius::same(4), ICON_BTN_HOVER);
                }
                let bm_icon = if self.is_bookmarked(&self.input) {
                    "bookmark-filled"
                } else {
                    "bookmark"
                };
                let bm_color = if bm_active || bm_resp.hovered() || !self.bookmarks.is_empty() {
                    ICON_BRIGHT
                } else {
                    ICON_DIM
                };
                rustudio_engine::ti::paint(ui.painter(), bm_rect.shrink(5.0), bm_icon, bm_color);
                if bm_resp.clicked() {
                    self.popup = if self.popup == ActivePopup::Bookmarks {
                        ActivePopup::None
                    } else {
                        ActivePopup::Bookmarks
                    };
                }
                bookmark_anchor = Some(bm_rect);
                x += btn_h + 6.0;

                // 2. Command History button (Studio shows filled `rgb(41, 43, 49)` square).
                let hist_rect = Rect::from_min_size(egui::pos2(x, btn_y), Vec2::splat(btn_h));
                let hist_resp = ui
                    .interact(hist_rect, egui::Id::new("command_bar_history"), Sense::click())
                    .on_hover_text("Command History (Ctrl+Up/Down)");
                let hist_bg = if hist_resp.hovered() || self.popup == ActivePopup::History {
                    ICON_BTN_HOVER
                } else {
                    ICON_BTN_BG
                };
                ui.painter().rect_filled(hist_rect, CornerRadius::same(4), hist_bg);
                rustudio_engine::ti::paint(ui.painter(), hist_rect.shrink(5.0), "history", ICON_BRIGHT);
                if hist_resp.clicked() {
                    self.popup = if self.popup == ActivePopup::History {
                        ActivePopup::None
                    } else {
                        ActivePopup::History
                    };
                }
                history_anchor = Some(hist_rect);
                x += btn_h + 6.0;

                // 3. Run button (play triangle + "Run").
                let can_run = !self.input.trim().is_empty();
                let run_w = (cluster_rect.right() - x - 4.0).max(54.0);
                let run_rect = Rect::from_min_size(egui::pos2(x, btn_y), Vec2::new(run_w, btn_h));
                let run_sense = if can_run { Sense::click() } else { Sense::hover() };
                let run_resp = ui
                    .interact(run_rect, egui::Id::new("command_bar_run"), run_sense)
                    .on_hover_text("Run command (Ctrl+Enter)");
                let (run_bg, run_fg) = if !can_run {
                    (RUN_DISABLED_BG, RUN_DISABLED_FG)
                } else if run_resp.hovered() {
                    (RUN_ENABLED_HOVER, ICON_BRIGHT)
                } else {
                    (RUN_ENABLED_BG, ICON_BRIGHT)
                };
                ui.painter().rect_filled(run_rect, CornerRadius::same(4), run_bg);
                let tri_rect = Rect::from_center_size(
                    egui::pos2(run_rect.left() + 14.0, run_rect.center().y),
                    Vec2::splat(13.0),
                );
                rustudio_engine::ti::paint(ui.painter(), tri_rect, "player-play-filled", run_fg);
                ui.painter().text(
                    egui::pos2(run_rect.left() + 26.0, run_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    "Run",
                    FontId::proportional(13.0),
                    run_fg,
                );
                if can_run && run_resp.clicked() {
                    run_clicked = true;
                }
            });
        let _ = panel_resp;

        if run_clicked {
            let code = self.input.clone();
            if let Some(req) = self.submit(code) {
                host_ctx.requests.push(req);
            }
        }

        match self.popup {
            ActivePopup::History => {
                if let Some(anchor) = history_anchor {
                    self.show_history_popup(&ctx, anchor, host_ctx.requests);
                }
            }
            ActivePopup::Bookmarks => {
                if let Some(anchor) = bookmark_anchor {
                    self.show_bookmarks_popup(&ctx, anchor, host_ctx.requests);
                }
            }
            ActivePopup::None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_cycling_preserves_unsent_draft_and_deduplicates() {
        let mut cb = CommandBarPlugin::default();
        assert!(cb.submit("print(1)".into()).is_some());
        assert!(cb.submit("print(2)".into()).is_some());
        // Re-running print(1) moves it to newest without duplicating.
        assert!(cb.submit("print(1)".into()).is_some());
        assert_eq!(cb.history, vec!["print(2)", "print(1)"]);

        cb.input = "local x = ".into();
        cb.history_prev();
        assert_eq!(cb.input, "print(1)");
        cb.history_prev();
        assert_eq!(cb.input, "print(2)");
        // Clamped at oldest.
        cb.history_prev();
        assert_eq!(cb.input, "print(2)");
        cb.history_next();
        assert_eq!(cb.input, "print(1)");
        // Stepping past newest restores the unsent draft.
        cb.history_next();
        assert_eq!(cb.input, "local x = ");
        assert_eq!(cb.history_idx, None);
    }

    #[test]
    fn bookmarks_toggle_and_whitespace_ignored() {
        let mut cb = CommandBarPlugin::default();
        assert!(cb.submit("   \n\t ".into()).is_none());
        cb.toggle_bookmark("print('hi')");
        assert!(cb.is_bookmarked("print('hi')"));
        cb.toggle_bookmark("print('hi')");
        assert!(!cb.is_bookmarked("print('hi')"));
    }

    #[test]
    fn command_bar_executes_luau_in_edit_vm_logs_output_and_supports_undo() {
        let ctx = egui::Context::default();
        let mut app = crate::app::RustudioApp::new_in(
            &ctx,
            None,
            None,
            None,
            crate::app::Harness::test_empty(),
            false,
        );
        let ed = &mut app.ed;
        assert!(ed.plugins.native().is_loaded(PLUGIN_ID));
        assert_eq!(ed.capability_state(CAPABILITY), (true, Some(false)));

        // Toggle open via capability.
        assert!(ed.trigger_capability(CAPABILITY));
        assert_eq!(ed.capability_state(CAPABILITY), (true, Some(true)));

        // Run a Luau command that creates a Part and prints.
        ed.run_command_bar_luau(
            "local p = Instance.new('Part'); p.Name = 'CmdBarPart'; p.Parent = workspace; print('CMD_BAR_RAN')",
        );
        let root = ed.game.doc.dom.root_ref();
        let ws = ed.game.doc.find_child(root, "Workspace").unwrap();
        assert!(
            ed.game.doc.find_named(ws, "CmdBarPart").is_some(),
            "Command Bar should have inserted CmdBarPart into Workspace; log={:?}",
            ed.game.log.iter().map(|l| &l.text).collect::<Vec<_>>()
        );
        assert!(
            ed.game.log.iter().any(|l| l.text.contains("CMD_BAR_RAN")),
            "Output should contain CMD_BAR_RAN: {:?}",
            ed.game.log.iter().map(|l| &l.text).collect::<Vec<_>>()
        );

        // Undo reverts the Command Bar edit.
        ed.undo();
        assert!(
            ed.game.doc.find_named(ws, "CmdBarPart").is_none(),
            "Undo should revert Command Bar edits"
        );
    }
}

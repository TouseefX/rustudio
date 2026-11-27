//! The Output panel, as Studio's (checks/ui/studio/output*.png, the Studio
//! an offline Studio runs): a toolbar with "All Messages", "All Contexts", a Filter
//! field, the clear button and a "⋯" menu (Show Timestamp off, Show Context
//! off, Show Source on, Studio's defaults), then monospace lines:
//!
//!   [bar] [HH:MM:SS.mmm  ]message - Script:line
//!
//! Colours as Studio picks them (, see
//! docs/MECHANICS.md): a table of theme colours indexed by message type — Log
//! MainText (188, 190, 200), Information InfoText (143, 180, 255), Warning
//! WarningText (245, 118, 48), Error ErrorText (231, 87, 80), System
//! SensitiveText (223, 106, 247) — with types above Information DemiBold
//! (Builder Mono has no semi-bold face: Bold). The context bar
//! is one &nbsp; on #00CC67 (server) or #349AD5 (client),
//! then one more &nbsp;; lines 15 px apart. Log lines keep rustudio's
//! stored form ("[Server] msg  —  Path.To.Script:12", which the harnesses
//! read); this only draws them.

use egui::{Color32, FontId, RichText, Sense, Vec2};

use crate::app::{Editor, Level, LogLine};
use rustudio_engine::runtime::{error_origin, STACK_MARK};
use rustudio_engine::theme;

pub const PRINT: Color32 = Color32::from_rgb(188, 190, 200);
pub const WARN: Color32 = Color32::from_rgb(245, 118, 48);
pub const ERROR: Color32 = Color32::from_rgb(231, 87, 80);
pub const INFO: Color32 = Color32::from_rgb(143, 180, 255);
pub const SYSTEM: Color32 = Color32::from_rgb(223, 106, 247);
pub const SERVER_BAR: Color32 = Color32::from_rgb(0, 204, 103);
pub const CLIENT_BAR: Color32 = Color32::from_rgb(52, 154, 213);
const LINE_H: f32 = 15.0;

/// Studio Settings > Output > Output Font: its size (10 pt = 13 px, with
/// LINE_H rows), and the row height that goes with it.
fn mono_px() -> f32 {
    crate::studio_settings::font_px("Studio", "Output Font")
}

pub(crate) fn line_h() -> f32 {
    (LINE_H * mono_px() / 13.0).round()
}

fn mono_font() -> FontId {
    FontId::monospace(mono_px())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Print,
    Info,
    Warn,
    Error,
    System,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Context {
    None,
    Server,
    Client,
}

/// One stored line taken apart the way Studio shows it.
pub struct Shown<'a> {
    pub kind: Kind,
    pub context: Context,
    pub message: &'a str,
    /// "Script:line" (the last part of the source path), when there is one.
    pub source: Option<String>,
}

fn short_loc(loc: &str) -> String {
    // "Players.Player.PlayerScripts.Client.MapPreload:30" → "MapPreload:30"
    let (path, line) = match loc.rsplit_once(':') {
        Some((p, n)) if n.chars().all(|c| c.is_ascii_digit()) => (p, Some(n)),
        _ => (loc, None),
    };
    let short = path.rsplit('.').next().unwrap_or(path);
    match line {
        Some(n) => format!("{short}:{n}"),
        None => short.to_string(),
    }
}

pub fn parse(l: &LogLine) -> Shown<'_> {
    let mut t = l.text.as_str();
    let context = if let Some(rest) = t.strip_prefix("[Server] ") {
        t = rest;
        Context::Server
    } else if let Some(rest) = t.strip_prefix("[Client] ") {
        t = rest;
        Context::Client
    } else {
        Context::None
    };
    // Stack lines: blue, without the context bar (as Studio draws them).
    let (t, stack) = match t.strip_prefix(STACK_MARK) {
        Some(rest) => (rest, true),
        None => (t, false),
    };
    let (message, source) = match t.rsplit_once("  —  ") {
        Some((m, loc)) => (m, Some(short_loc(loc))),
        // An error names its script in front: "Full.Name:3: boom - Name:3".
        None if l.level == Level::Error => (t, error_origin(t).map(|(p, n)| short_loc(&format!("{p}:{n}")))),
        None => (t, None),
    };
    if stack {
        return Shown { kind: Kind::Info, context: Context::None, message, source };
    }
    // Script output (from a side, or with a source) prints in the normal
    // colour; rustudio's own lines are Studio's blue info lines; publish /
    // save / system lines are Studio's purple System lines (MESSAGE_SYSTEM).
    let kind = match l.level {
        Level::Warn => Kind::Warn,
        Level::Error => Kind::Error,
        Level::System => Kind::System,
        Level::Info if context != Context::None || source.is_some() => Kind::Print,
        Level::Info => Kind::Info,
    };
    Shown { kind, context, message, source }
}

/// The run of same-kind characters (word chars, spaces, or one other mark)
/// around `c` on line `c.0`.
fn word_at(text: &str, c: TextPos) -> (TextPos, TextPos) {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return (c, c);
    }
    let class = |ch: char| {
        if ch.is_alphanumeric() || ch == '_' {
            0
        } else if ch == '\n' {
            3
        } else if ch.is_whitespace() {
            1
        } else {
            2
        }
    };
    let mut at = c.1.min(chars.len() - 1);
    // Past the end of a line: the word just before it.
    if class(chars[at]) == 3 && at > 0 && class(chars[at - 1]) != 3 {
        at -= 1;
    }
    let k = class(chars[at]);
    if k >= 2 {
        return ((c.0, at), (c.0, at + 1));
    }
    let mut a = at;
    while a > 0 && class(chars[a - 1]) == k {
        a -= 1;
    }
    let mut b = at + 1;
    while b < chars.len() && class(chars[b]) == k {
        b += 1;
    }
    ((c.0, a), (c.0, b))
}

/// The line of a (maybe multi-line) message around `c`, its break included
/// when one follows.
fn line_at(text: &str, c: TextPos) -> (TextPos, TextPos) {
    let mut start = 0;
    for seg in text.split('\n') {
        let end = start + seg.chars().count();
        if c.1 <= end {
            let n = text.chars().count();
            return ((c.0, start), (c.0, (end + 1).min(n)));
        }
        start = end + 1;
    }
    ((c.0, 0), (c.0, text.chars().count()))
}

pub fn kind_color(k: Kind) -> Color32 {
    match k {
        Kind::Print => PRINT,
        Kind::Info => INFO,
        Kind::Warn => WARN,
        Kind::Error => ERROR,
        Kind::System => SYSTEM,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MessageFilter {
    All,
    Output,
    Info,
    Warnings,
    Errors,
    System,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ContextFilter {
    All,
    Server,
    Client,
}

/// A place in the output: (log line index, char in the line as shown).
pub type TextPos = (usize, usize);

/// Text selected across Output lines, as a text editor's: the whole log is
/// one selectable text (click / double / triple + drag, Ctrl+A, Ctrl+C).
#[derive(Default)]
pub struct OutputSelection {
    /// Where the selection started and where it ends now (either order).
    pub anchor: TextPos,
    pub head: TextPos,
    /// Mouse held after a press in the panel: 1 chars, 2 words, 3 lines,
    /// with the word / line first picked.
    drag: Option<(u8, TextPos, TextPos)>,
    /// Quick presses near one spot: time, place, count (1..=3).
    press: Option<(f64, egui::Pos2, u8)>,
}

pub struct OutputState {
    pub messages: MessageFilter,
    pub contexts: ContextFilter,
    pub filter: String,
    pub show_timestamp: bool,
    pub show_context: bool,
    pub show_source: bool,
    pub wrap_text: bool,
    pub sel: OutputSelection,
    rows: OutputRows,
    pub(crate) log_generation: u64,
}

#[derive(Default)]
struct OutputRows {
    filter: Option<(MessageFilter, ContextFilter, String)>,
    indexed: usize,
    shown: Vec<usize>,
    tops: Vec<f32>,
    height: f32,
    row_height: f32,
    /// Active wrap width when `wrap_text` is enabled, else `None`.
    wrap_width: Option<f32>,
    /// Line-format flags that affect rendered width when wrapping.
    line_opts: (bool, bool, bool),
    pixels_per_point: f32,
    /// The widest line drawn so far (the content's width when not wrapping:
    /// long lines scroll sideways, as Studio's Output does).
    width: f32,
}

impl OutputRows {
    fn clear(&mut self) {
        self.indexed = 0;
        self.shown.clear();
        self.tops.clear();
        self.height = 0.0;
        self.width = 0.0;
    }
}

impl OutputState {
    /// The log dropped its first `n` lines: keep the selection on its text.
    pub fn log_drained(&mut self, n: usize) {
        self.rows.clear();
        self.log_generation = self.log_generation.wrapping_add(1);
        let s = &mut self.sel;
        let shift = |p: &mut TextPos| *p = if p.0 >= n { (p.0 - n, p.1) } else { (0, 0) };
        shift(&mut s.anchor);
        shift(&mut s.head);
        s.drag = None;
    }
    pub fn log_cleared(&mut self) {
        self.rows.clear();
        self.log_generation = self.log_generation.wrapping_add(1);
        let s = &mut self.sel;
        s.anchor = (0, 0);
        s.head = (0, 0);
        s.drag = None;
    }
}

impl Default for OutputState {
    fn default() -> Self {
        OutputState {
            messages: MessageFilter::All,
            contexts: ContextFilter::All,
            filter: String::new(),
            show_timestamp: false,
            show_context: false,
            show_source: true,
            wrap_text: true,
            sel: OutputSelection::default(),
            rows: OutputRows::default(),
            log_generation: 0,
        }
    }
}

fn wanted(st: &OutputState, filter: &str, s: &Shown, l: &LogLine) -> bool {
    let kind_ok = match st.messages {
        MessageFilter::All => true,
        MessageFilter::Output => s.kind == Kind::Print,
        MessageFilter::Info => s.kind == Kind::Info,
        MessageFilter::Warnings => s.kind == Kind::Warn,
        MessageFilter::Errors => s.kind == Kind::Error,
        MessageFilter::System => s.kind == Kind::System,
    };
    let ctx_ok = match st.contexts {
        ContextFilter::All => true,
        ContextFilter::Server => s.context == Context::Server,
        ContextFilter::Client => s.context == Context::Client,
    };
    let text_ok = filter.is_empty() || l.text.to_lowercase().contains(filter);
    kind_ok && ctx_ok && text_ok
}

/// The line as one LayoutJob in Studio's colours (bold for warnings / errors / system).
pub fn line_job(st: &OutputState, l: &LogLine, s: &Shown) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let mono = mono_font();
    let plain = |c: Color32| egui::TextFormat { font_id: mono.clone(), color: c, line_height: Some(line_h()), ..Default::default() };
    if st.show_timestamp {
        job.append(&format!("{}  ", l.at), 0.0, plain(PRINT));
    }
    if st.show_context && s.context != Context::None {
        let c = if s.context == Context::Server { "Server" } else { "Client" };
        job.append(&format!("{c} - "), 0.0, plain(PRINT));
    }
    let mut fmt = plain(kind_color(s.kind));
    if matches!(s.kind, Kind::Warn | Kind::Error | Kind::System) {
        // (Studio draws types above Information DemiBold; egui's
        // monospace has no semi-bold face, so a heavier look comes from the
        // "mono-bold" family when it is installed)
        fmt.font_id = FontId::new(mono_px(), egui::FontFamily::Name("mono-bold".into()));
    }
    job.append(s.message, 0.0, fmt);
    if st.show_source {
        if let Some(src) = &s.source {
            job.append(&format!(" - {src}"), 0.0, plain(PRINT));
        }
    }
    job
}

impl Editor {
    pub fn output_ui(&mut self, ui: &mut egui::Ui) {
        let _p = rustudio_core::bench::scope("output_build");
        self.ui_rects.insert("output.panel", ui.max_rect());
        self.flush_output_changes();
        let st = &mut self.output_view;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("out_messages")
                .width(96.0)
                .selected_text(match st.messages {
                    MessageFilter::All => "All Messages",
                    MessageFilter::Output => "Output",
                    MessageFilter::Info => "Info",
                    MessageFilter::Warnings => "Warnings",
                    MessageFilter::Errors => "Errors",
                    MessageFilter::System => "System",
                })
                .show_ui(ui, |ui| {
                    for (v, n) in [
                        (MessageFilter::All, "All Messages"),
                        (MessageFilter::Output, "Output"),
                        (MessageFilter::Info, "Info"),
                        (MessageFilter::Warnings, "Warnings"),
                        (MessageFilter::Errors, "Errors"),
                        (MessageFilter::System, "System"),
                    ] {
                        ui.selectable_value(&mut st.messages, v, n);
                    }
                });
            egui::ComboBox::from_id_salt("out_contexts")
                .width(96.0)
                .selected_text(match st.contexts {
                    ContextFilter::All => "All Contexts",
                    ContextFilter::Server => "Server",
                    ContextFilter::Client => "Client",
                })
                .show_ui(ui, |ui| {
                    for (v, n) in [(ContextFilter::All, "All Contexts"), (ContextFilter::Server, "Server"), (ContextFilter::Client, "Client")] {
                        ui.selectable_value(&mut st.contexts, v, n);
                    }
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let more = ui.add(egui::Button::new(String::default()).min_size(Vec2::new(24.0, 20.0))).on_hover_text("Output Settings");
                rustudio_engine::ti::paint(ui.painter(), more.rect.shrink(2.0), "settings", theme::TEXT);
                egui::Popup::menu(&more).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                    ui.checkbox(&mut st.show_timestamp, "Show Timestamp");
                    ui.checkbox(&mut st.show_context, "Show Context");
                    ui.checkbox(&mut st.show_source, "Show Source");
                    ui.checkbox(&mut st.wrap_text, "Wrap Text");
                    // Studio Settings > Output > Clear Output On Start (the same setting).
                    let mut clear = crate::studio_settings::flag("Studio", "Clear Output On Start", true);
                    if ui.checkbox(&mut clear, "Clear On Play").changed() {
                        crate::studio_settings::set("Studio", "Clear Output On Start", crate::studio_settings::Val::Bool(clear));
                    }
                    if let Some(path) = rustudio_core::diagnostics::path() {
                        if ui.button("Copy Diagnostic Log Path").clicked() {
                            ui.ctx().copy_text(path.display().to_string());
                        }
                    }
                });
                let clear = ui.add(egui::Button::new(String::default()).min_size(Vec2::new(24.0, 20.0))).on_hover_text("Clear Output");
                rustudio_engine::ti::paint(ui.painter(), clear.rect.shrink(2.0), "trash", theme::TEXT);
                if clear.clicked() {
                    self.game.log.clear();
                    st.log_cleared();
                }
                ui.add(egui::TextEdit::singleline(&mut st.filter).hint_text("Filter...").desired_width(240.0));
            });
        });
        self.hud.output_lines = log_view(ui, &mut self.output_view, &self.game.log);
        let _ = RichText::new("");
    }
}

/// The lines, drawn and selectable as one text; returns how many were drawn.
pub fn log_view(ui: &mut egui::Ui, st: &mut OutputState, log: &[LogLine]) -> usize {
    let row_height = line_h();
    let mut index = std::mem::take(&mut st.rows);
    let text_of = |st: &OutputState, i: usize| line_job(st, &log[i], &parse(&log[i])).text;
    let id = egui::Id::new("output_text");
    let mut drawn = 0;
    let mut scroll = egui::ScrollArea::new([!st.wrap_text, true])
        .auto_shrink([false, false])
        .stick_to_bottom(true);
    if st.wrap_text {
        scroll = scroll.horizontal_scroll_offset(0.0);
    }
    scroll.show_viewport(ui, |ui, viewport| {
        let space = ui.fonts_mut(|f| f.glyph_width(&mono_font(), ' '));
        let text_indent = 2.0 * space + 2.0 * ui.spacing().item_spacing.x;
        let avail_w = ui.available_width();
        let mut wrap_width = st.wrap_text.then(|| (avail_w - text_indent - space).max(space * 2.0).floor());
        // While the panel is being resized (a button held), keep the last
        // wrap width: re-wrapping a long log at every intermediate width is a
        // full layout pass per frame. It re-wraps once on release.
        if wrap_width.is_some() && index.wrap_width.is_some() && ui.ctx().input(|i| i.pointer.any_down()) {
            wrap_width = index.wrap_width;
        }
        let line_opts = (st.show_timestamp, st.show_context, st.show_source);
        let pixels_per_point = ui.ctx().pixels_per_point();

        let same_filter = index.filter.as_ref().is_some_and(|(messages, contexts, text)| {
            *messages == st.messages && *contexts == st.contexts && *text == st.filter
        });
        let same_wrap = index.wrap_width == wrap_width
            && (wrap_width.is_none() || (index.line_opts == line_opts && index.pixels_per_point == pixels_per_point));
        if !same_filter || !same_wrap || index.indexed > log.len() || index.row_height != row_height {
            index.clear();
            index.filter = Some((st.messages, st.contexts, st.filter.clone()));
        }
        index.row_height = row_height;
        index.wrap_width = wrap_width;
        index.line_opts = line_opts;
        index.pixels_per_point = pixels_per_point;
        index.tops.pop();
        if index.indexed < log.len() {
            // Appends only inspect new messages. Clearing or draining the log
            // invalidates the index, as do the filters, Output Font, wrap width,
            // and (when wrapping) the line prefix/suffix options.
            let filter = st.filter.to_lowercase();
            let unfiltered = st.messages == MessageFilter::All && st.contexts == ContextFilter::All && filter.is_empty();
            let max_ascii_w = if wrap_width.is_some() {
                let mono_w = ui.fonts_mut(|f| f.glyph_width(&mono_font(), 'W'));
                let bold_w = ui.fonts_mut(|f| {
                    f.glyph_width(&FontId::new(mono_px(), egui::FontFamily::Name("mono-bold".into())), 'W')
                });
                mono_w.max(bold_w).max(space)
            } else {
                0.0
            };
            for (i, line) in log.iter().enumerate().skip(index.indexed) {
                let parsed = parse(line);
                if unfiltered || wanted(st, &filter, &parsed, line) {
                    index.shown.push(i);
                    index.tops.push(index.height);
                    let h = match wrap_width {
                        None => row_height * (line.text.matches('\n').count() + 1) as f32,
                        Some(w) => {
                            let mut job = line_job(st, line, &parsed);
                            let single = !job.text.bytes().any(|b| b < 0x20 || b >= 0x80)
                                && (job.text.len() as f32) * max_ascii_w + 1.0 <= w;
                            if single {
                                row_height
                            } else {
                                job.wrap.max_width = w;
                                ui.fonts_mut(|f| f.layout_job(job)).size().y.max(row_height)
                            }
                        }
                    };
                    index.height += h;
                }
            }
            index.indexed = log.len();
        }
        index.tops.push(index.height);

        let (shown, tops) = (&index.shown, &index.tops);
        let mut widest = index.width;
        let total = *tops.last().unwrap_or(&0.0);
        let content_w = if st.wrap_text { avail_w } else { avail_w.max(widest) };
        let (content, _) = ui.allocate_exact_size(Vec2::new(content_w, total), Sense::hover());
        let view = ui.clip_rect();
        // Under the lines (they only sense hover), above nothing: the
        // scroll bar is added after and stays on top.
        let resp = ui.interact(view, id, Sense::click_and_drag());
        let first = tops.partition_point(|&t| t <= viewport.min.y).saturating_sub(1);
        let last = tops.partition_point(|&t| t < viewport.max.y).min(shown.len());
        // Each drawn message: log index, its height, its top, where its text starts,
        // its layout, and a slot for the selection painted under it.
        let mut rows = Vec::new();
        for k in first..last {
            let i = shown[k];
            let l = &log[i];
            let s = parse(l);
            let mut job = line_job(st, l, &s);
            if let Some(w) = wrap_width {
                job.wrap.max_width = w;
            }
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            let top = content.min.y + tops[k];
            // One monospace space wide, then one more space.
            let bar = egui::Rect::from_min_size(egui::pos2(content.min.x, top), Vec2::new(space, row_height));
            let bar_color = match s.context {
                Context::Server => Some(SERVER_BAR),
                Context::Client => Some(CLIENT_BAR),
                Context::None => None,
            };
            if let Some(c) = bar_color {
                ui.painter().rect_filled(bar, 0.0, c);
            }
            let slot = ui.painter().add(egui::Shape::Noop);
            // (as the earlier row layout spaced them: bar, gap, space, gap)
            let pos = egui::pos2(content.min.x + text_indent, top);
            ui.painter().galley(pos, galley.clone(), PRINT);
            if !st.wrap_text {
                widest = widest.max(pos.x - content.min.x + galley.size().x + space);
            }
            rows.push((i, tops[k + 1] - tops[k], top, pos, galley, slot, space));
            drawn += 1;
        }
        index.width = widest;
        if resp.hovered() {
            ui.set_cursor_icon(egui::CursorIcon::Text);
        }

        // Pointer → (line, char) on the lines drawn this frame;
        // above / below them clamps to the first / last.
        let at = |p: egui::Pos2| -> Option<TextPos> {
            let (first, last) = (rows.first()?, rows.last()?);
            if p.y < first.2 {
                return Some((first.0, 0));
            }
            let row = rows.iter().rev().find(|r| p.y >= r.2).unwrap_or(last);
            if p.y >= row.2 + row.1 && std::ptr::eq(row, last) {
                return Some((row.0, row.4.text().chars().count()));
            }
            let c = row.4.cursor_from_pos(p - row.3).index.0;
            Some((row.0, c))
        };
        let (pressed, down, ptr, now, shift) = ui.input(|i| {
            (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.interact_pos(), i.time, i.modifiers.shift)
        });
        let text_on = |line: usize| rows.iter().find(|r| r.0 == line).map(|r| r.4.text().to_string());
        let sel = &mut st.sel;
        let hit = ptr.and_then(|p| at(p).map(|c| (p, c)));
        if let (true, Some((p, c))) = (pressed && resp.hovered(), hit) {
            ui.memory_mut(|m| m.request_focus(id));
            let count = match sel.press {
                Some((t, q, n)) if now - t < 0.4 && q.distance(p) < 6.0 => n % 3 + 1,
                _ => 1,
            };
            sel.press = Some((now, p, count));
            let text = text_on(c.0).unwrap_or_default();
            let (a, b) = match count {
                1 if shift => (sel.anchor, c),
                1 => (c, c),
                2 => word_at(&text, c),
                _ => line_at(&text, c),
            };
            sel.anchor = a;
            sel.head = b;
            sel.drag = Some((count, a, b));
            if count == 1 && shift {
                sel.drag = Some((1, a, a));
            }
        } else if let (true, Some((mode, a, b)), Some((p, c))) = (down, sel.drag, hit) {
            // Dragging: grow by chars, words or lines from the press.
            let (h0, h1) = match mode {
                1 => (c, c),
                2 => word_at(&text_on(c.0).unwrap_or_default(), c),
                _ => line_at(&text_on(c.0).unwrap_or_default(), c),
            };
            (sel.anchor, sel.head) = if mode == 1 {
                (a, c)
            } else if h0 < a {
                (b, h0)
            } else {
                (a, h1.max(b))
            };
            // Past the top / bottom edge: scroll along.
            let dy = if p.y < view.top() {
                view.top() - p.y
            } else if p.y > view.bottom() {
                view.bottom() - p.y
            } else {
                0.0
            };
            if dy != 0.0 {
                ui.scroll_with_delta(Vec2::new(0.0, dy.clamp(-row_height * 2.0, row_height * 2.0)));
                ui.ctx().request_repaint();
            }
        }
        if !down {
            sel.drag = None;
        }
        // Clicking anywhere else gives the keyboard back.
        if ui.input(|i| i.pointer.any_pressed()) && !resp.hovered() && ui.memory(|m| m.has_focus(id)) {
            ui.memory_mut(|m| m.surrender_focus(id));
        }

        // Paint the selection under each drawn line.
        let (lo, hi) = if sel.anchor <= sel.head { (sel.anchor, sel.head) } else { (sel.head, sel.anchor) };
        if lo != hi {
            let fill = ui.visuals().selection.bg_fill;
            for (i, _, _top, pos, galley, slot, space) in &rows {
                if *i < lo.0 || *i > hi.0 {
                    continue;
                }
                let n = galley.text().chars().count();
                let a = if *i == lo.0 { lo.1.min(n) } else { 0 };
                let b = if *i == hi.0 { hi.1.min(n) } else { n };
                // One rect per visual row of the message; a selection running
                // past an explicit line break covers its break too.
                let mut shapes = Vec::new();
                let mut start = 0;
                let last_row = galley.rows.len().saturating_sub(1);
                for (k, row) in galley.rows.iter().enumerate() {
                    let len = row.char_count_excluding_newline().0;
                    let (sa, sb) = (a.max(start), b.min(start + len));
                    let runs_on = if row.ends_with_newline {
                        b > start + len
                    } else {
                        k == last_row && *i < hi.0
                    };
                    if sa <= sb && (sa < sb || runs_on) {
                        let x0 = pos.x + row.pos.x + row.x_offset(egui::text::CharIndex(sa - start));
                        let x1 = pos.x + row.pos.x + row.x_offset(egui::text::CharIndex(sb - start))
                            + if runs_on { *space } else { 0.0 };
                        let r = egui::Rect::from_min_max(
                            egui::pos2(x0, pos.y + row.min_y()),
                            egui::pos2(x1, pos.y + row.max_y()),
                        );
                        shapes.push(egui::Shape::rect_filled(r, 0.0, fill));
                    }
                    start += row.char_count_including_newline().0;
                }
                ui.painter().set(*slot, egui::Shape::Vec(shapes));
            }
        }
    });

    let shown = &index.shown;
    // Keys while the Output has the keyboard: Ctrl+A, Ctrl+C, Esc.
    if ui.memory(|m| m.has_focus(id)) {
        let (all, copy, esc) = ui.input_mut(|i| {
            (
                i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::A)),
                i.events.iter().any(|e| matches!(e, egui::Event::Copy | egui::Event::Cut)),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if let (true, Some(&f), Some(&l)) = (all, shown.first(), shown.last()) {
            st.sel.anchor = (f, 0);
            st.sel.head = (l, text_of(st, l).chars().count());
        }
        if esc {
            st.sel.head = st.sel.anchor;
        }
        if copy {
            let (lo, hi) = if st.sel.anchor <= st.sel.head { (st.sel.anchor, st.sel.head) } else { (st.sel.head, st.sel.anchor) };
            if lo != hi {
                let mut out = String::new();
                for &i in shown.iter().filter(|&&i| i >= lo.0 && i <= hi.0) {
                    let t = text_of(st, i);
                    let n = t.chars().count();
                    let a = if i == lo.0 { lo.1.min(n) } else { 0 };
                    let b = if i == hi.0 { hi.1.min(n) } else { n };
                    if !out.is_empty() || i > lo.0 {
                        out.push('\n');
                    }
                    out.extend(t.chars().skip(a).take(b.saturating_sub(a)));
                }
                ui.ctx().copy_text(out.trim_start_matches('\n').to_string());
            }
        }
    }
    st.rows = index;
    drawn
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(level: Level, text: &str) -> LogLine {
        LogLine { at: "20:50:23.591".into(), level, text: text.into() }
    }

    /// Lines take Studio's kinds, contexts and "Script:line" sources.
    #[test]
    fn studio_line_format() {
        let p = line(Level::Info, "[Server] hello from a script  —  ServerScriptService.RefScript:1");
        let s = parse(&p);
        assert!(s.kind == Kind::Print && s.context == Context::Server);
        assert_eq!(s.message, "hello from a script");
        assert_eq!(s.source.as_deref(), Some("RefScript:1"));
        let wl = line(Level::Warn, "[Client] careful  —  Players.Player.PlayerScripts.RefLocal:2");
        let w = parse(&wl);
        assert!(w.kind == Kind::Warn && w.context == Context::Client);
        let il = line(Level::Info, "Opened place.rbxl (39 instances)");
        let i = parse(&il);
        assert!(i.kind == Kind::Info && i.source.is_none());
        let el = line(Level::Error, "[Server] ServerScriptService.RefScript:3: boom");
        let e = parse(&el);
        assert!(e.kind == Kind::Error);
        assert_eq!(e.source.as_deref(), Some("RefScript:3"));
        let sl = line(Level::Info, &format!("[Server] {STACK_MARK}Script 'ServerScriptService.RefScript', Line 3  —  ServerScriptService.RefScript:3"));
        let k = parse(&sl);
        assert!(k.kind == Kind::Info && k.context == Context::None);
        assert_eq!(k.message, "Script 'ServerScriptService.RefScript', Line 3");
        let mut st = OutputState::default();
        let text: String = line_job(&st, &p, &s).text;
        assert_eq!(text, "hello from a script - RefScript:1");
        st.show_timestamp = true;
        assert_eq!(line_job(&st, &p, &s).text, "20:50:23.591  hello from a script - RefScript:1");

        // Studio System messages (save / publish): SensitiveText purple, bold, filtered under All / System.
        let sysl = line(Level::System, "Published new changes in \"Baseplate\" to Roblox.");
        let sys = parse(&sysl);
        assert!(sys.kind == Kind::System && sys.context == Context::None && sys.source.is_none());
        assert_eq!(kind_color(sys.kind), SYSTEM);
        st.show_timestamp = false;
        let sys_job = line_job(&st, &sysl, &sys);
        assert_eq!(sys_job.text, "Published new changes in \"Baseplate\" to Roblox.");
        assert_eq!(sys_job.sections.len(), 1);
        assert_eq!(sys_job.sections[0].format.color, SYSTEM);
        assert_eq!(sys_job.sections[0].format.font_id.family, egui::FontFamily::Name("mono-bold".into()));
        assert!(wanted(&st, "", &sys, &sysl));
        st.messages = MessageFilter::System;
        assert!(wanted(&st, "", &sys, &sysl));
        assert!(!wanted(&st, "", &i, &il));
        st.messages = MessageFilter::Info;
        assert!(!wanted(&st, "", &sys, &sysl));
    }

    /// Drives `log_view` headless with mouse / key events.
    struct View {
        ctx: egui::Context,
        t: f64,
        width: f32,
        st: OutputState,
        log: Vec<LogLine>,
        copied: Option<String>,
    }

    impl View {
        fn new() -> Self {
            let log = vec![
                line(Level::Info, "[Server] hello world  —  ServerScriptService.A:1"),
                line(Level::Info, "[Client] second line here  —  Players.B:2"),
                line(Level::Warn, "[Server] third warning  —  ServerScriptService.C:3"),
            ];
            let ctx = egui::Context::default();
            rustudio_engine::ti::install(&ctx);
            let mut v = View { ctx, t: 0.0, width: 800.0, st: OutputState::default(), log, copied: None };
            v.frame(vec![]);
            v.frame(vec![]);
            v
        }
        fn frame(&mut self, events: Vec<egui::Event>) {
            let input = egui::RawInput {
                time: Some(self.t),
                events,
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(self.width, 300.0))),
                ..Default::default()
            };
            let (st, log) = (&mut self.st, &self.log);
            let mut fo = self.ctx.run_ui(input, |ui| {
                log_view(ui, st, log);
            });
            fo.textures_delta.clear();
            for c in fo.platform_output.commands {
                if let egui::OutputCommand::CopyText(s) = c {
                    self.copied = Some(s);
                }
            }
            self.t += 1.0 / 60.0;
        }
        /// Screen point of char `c` on shown line `row` (monospace).
        fn at(&self, row: usize, c: usize) -> egui::Pos2 {
            let w = self.ctx.fonts_mut(|f| f.glyph_width(&mono_font(), 'x'));
            // Bar, gap, space, gap, then the text.
            let gap = self.ctx.global_style().spacing.item_spacing.x;
            egui::pos2(2.0 * w + 2.0 * gap + w * (c as f32 + 0.3), line_h() * (row as f32 + 0.5))
        }
        fn btn(&mut self, p: egui::Pos2, down: bool) {
            self.frame(vec![egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: down,
                modifiers: egui::Modifiers::NONE,
            }]);
        }
        fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
            for i in 1..=8 {
                self.frame(vec![egui::Event::PointerMoved(from + (to - from) * (i as f32 / 8.0))]);
            }
        }
        fn copy(&mut self) -> String {
            self.copied = None;
            self.frame(vec![egui::Event::Copy]);
            self.copied.clone().unwrap_or_default()
        }
    }

    #[test]
    fn select_across_lines_and_copy() {
        let mut v = View::new();
        let (p, q) = (v.at(0, 6), v.at(1, 6));
        v.frame(vec![egui::Event::PointerMoved(p)]);
        v.btn(p, true);
        v.drag(p, q);
        v.btn(q, false);
        assert_eq!(v.copy(), "world - A:1\nsecond");
    }

    #[test]
    fn double_click_word_and_drag_words() {
        let mut v = View::new();
        let p = v.at(0, 7);
        v.frame(vec![egui::Event::PointerMoved(p)]);
        v.btn(p, true);
        v.btn(p, false);
        v.btn(p, true);
        v.frame(vec![]);
        v.frame(vec![]);
        assert_eq!(v.copy(), "world");
        let q = v.at(1, 2);
        v.drag(p, q);
        v.btn(q, false);
        assert_eq!(v.copy(), "world - A:1\nsecond");
    }

    #[test]
    fn triple_click_line_and_drag_lines() {
        let mut v = View::new();
        let p = v.at(1, 3);
        v.frame(vec![egui::Event::PointerMoved(p)]);
        for _ in 0..2 {
            v.btn(p, true);
            v.btn(p, false);
        }
        v.btn(p, true);
        assert_eq!(v.copy(), "second line here - B:2");
        let q = v.at(2, 1);
        v.drag(p, q);
        v.btn(q, false);
        assert_eq!(v.copy(), "second line here - B:2\nthird warning - C:3");
    }

    #[test]
    fn ctrl_a_selects_everything() {
        let mut v = View::new();
        let p = v.at(0, 1);
        v.frame(vec![egui::Event::PointerMoved(p)]);
        v.btn(p, true);
        v.btn(p, false);
        v.frame(vec![egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }]);
        assert_eq!(v.copy(), "hello world - A:1\nsecond line here - B:2\nthird warning - C:3");
    }

    #[test]
    fn filtered_output_indexes_appends_and_rebuilds_after_drain() {
        let mut v = View::new();
        v.st.contexts = ContextFilter::Server;
        v.frame(vec![]);
        assert_eq!(v.st.rows.shown, [0, 2]);

        v.log.push(line(Level::Warn, "[Client] hidden warning"));
        v.log.push(line(Level::Info, "[Server] extra line\ncontinued"));
        v.frame(vec![]);
        assert_eq!(v.st.rows.shown, [0, 2, 4]);
        assert_eq!(v.st.rows.height, 4.0 * v.st.rows.row_height);

        v.log.drain(..2);
        v.st.log_drained(2);
        v.frame(vec![]);
        assert_eq!(v.st.rows.shown, [0, 2]);
        assert_eq!(v.st.rows.height, 3.0 * v.st.rows.row_height);
    }

    #[test]
    fn multi_line_messages_take_their_height() {
        let mut v = View::new();
        v.log.insert(1, line(Level::Info, "[Server] banner top\nbanner mid\nbanner end  —  ServerScriptService.D:4"));
        v.st.log_cleared();
        v.frame(vec![]);
        // Visual rows: 0 hello, 1-3 banner, 4 second line.
        let p = v.at(4, 1);
        v.frame(vec![egui::Event::PointerMoved(p)]);
        for _ in 0..3 {
            v.btn(p, true);
            v.btn(p, false);
        }
        assert_eq!(v.copy(), "second line here - B:2");
        // Triple-click the banner's middle line, drag to its last.
        let (p, q) = (v.at(2, 2), v.at(3, 2));
        v.frame(vec![egui::Event::PointerMoved(p)]);
        v.frame(vec![]);
        v.t += 1.0;
        for _ in 0..2 {
            v.btn(p, true);
            v.btn(p, false);
        }
        v.btn(p, true);
        assert_eq!(v.copy(), "banner mid\n");
        v.drag(p, q);
        v.btn(q, false);
        assert_eq!(v.copy(), "banner mid\nbanner end - D:4");
        // Char drag from the banner's top into the next message.
        let (p, q) = (v.at(1, 7), v.at(4, 6));
        v.t += 1.0;
        v.frame(vec![egui::Event::PointerMoved(p)]);
        v.btn(p, true);
        v.drag(p, q);
        v.btn(q, false);
        assert_eq!(v.copy(), "top\nbanner mid\nbanner end - D:4\nsecond");
    }

    #[test]
    fn wrapping_defaults_on_and_updates_layout_and_selection() {
        let mut v = View::new();
        assert!(v.st.wrap_text);
        v.log = vec![
            line(Level::Info, "alpha beta gamma delta epsilon zeta eta theta iota kappa"),
            line(Level::Info, "after wrap"),
        ];
        v.st.log_cleared();
        v.width = 220.0;
        v.frame(vec![]);
        v.frame(vec![]);

        let row_h = v.st.rows.row_height;
        let first_h = v.st.rows.tops[1] - v.st.rows.tops[0];
        let wrapped_rows = (first_h / row_h).round() as usize;
        assert!(wrapped_rows >= 2, "expected long line to wrap at 220px, got height {first_h} (row_h {row_h})");
        assert_eq!(v.st.rows.width, 0.0, "wrapping should not grow horizontal content width");

        // Drag from row 0 across the wrapped visual rows into the second log line.
        let p = v.at(0, 6);
        let q = v.at(wrapped_rows, 5);
        v.frame(vec![egui::Event::PointerMoved(p)]);
        v.btn(p, true);
        v.drag(p, q);
        v.btn(q, false);
        assert_eq!(
            v.copy(),
            "beta gamma delta epsilon zeta eta theta iota kappa\nafter",
        );

        // Enabling timestamps while wrapped rebuilds heights to fit the longer prefix.
        v.st.show_timestamp = true;
        v.frame(vec![]);
        assert!(v.st.rows.tops[1] >= first_h);

        // Widening the panel reduces the wrapped height back to a single row per line.
        v.width = 900.0;
        v.frame(vec![]);
        assert_eq!(v.st.rows.tops[1] - v.st.rows.tops[0], row_h);

        // Turning wrapping off keeps long lines on a single row and tracks horizontal width.
        v.width = 220.0;
        v.st.wrap_text = false;
        v.frame(vec![]);
        assert_eq!(v.st.rows.tops[1] - v.st.rows.tops[0], row_h);
        assert!(v.st.rows.width > 220.0);
    }
}

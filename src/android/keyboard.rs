//! Room for the soft keyboard. winit never resizes an Android window for
//! the keyboard (the manifest's `adjustResize` moves NativeActivity's
//! content rect; the surface egui draws on stays the whole screen, and on
//! Android 11+ a window's frame is never resized for the keyboard at all:
//! the keyboard is an inset, `WindowInsets.Type.ime()`, that the app lays
//! itself out around), so the field being typed into sat behind it. While
//! a text field has the focus the keyboard's height is read from the window
//! insets ([`jni::keyboard_height_later`](super::jni::keyboard_height_later))
//! and
//!
//! * the editor's panels are laid out above it and the text bar over it
//!   (text_bar.rs: Cut / Copy / Paste / Select all; [`Keyboard::shrink`]
//!   takes that much — [`Keyboard::room`] — off the root `Ui` in `App::ui`,
//!   so the dock, the 3D view and the status bar move up as `adjustResize`
//!   would move an Android view)
//!   — unless a game's TextBox is typed into: the keyboard then lies over
//!   the window as it does over a phone's games, and the 3D view slides up
//!   by what shows the box above it (`Editor::keyboard_pan` in
//!   src/viewport.rs, from `Editor::keyboard_cover`; the panels above the
//!   keyboard left a landscape phone a view a few lines tall, the box
//!   squeezed or off it under the status bar). The game's scripts read
//!   the keyboard's place as `UserInputService.OnScreenKeyboardVisible /
//!   Position / Size`;
//! * it is egui's bottom safe-area inset, so windows, menus and popups keep
//!   to the area above it too ([`Keyboard::before_frame`]) — again not
//!   while a game's TextBox is typed into: egui lays its bottom panels out
//!   above the inset, and the status bar rose with the keyboard over the
//!   box the view had just slid up to show; the inset stays 0 then
//!   ([`Keyboard::cover`] tells the view how much the keyboard covers);
//! * the frame the room changes, [`reveal`] is true: the field being typed
//!   into scrolls itself into view (properties.rs `text_cell`, the script
//!   editor: [`reveal_cursor`]), as Android scrolls a focused EditText.
//!
//! The height is asked of the Java main thread every [`POLL`] while a text
//! field has the focus or the keyboard was up at the last look, without
//! waiting for the answer (it comes a frame or two later); at other times
//! nothing runs. egui keeps the last safe-area inset it was given, so the
//! inset is set every frame, zero included.
//!
//! The keyboard is shown and hidden from here ([`Keyboard::end_frame`]),
//! not by egui-winit. egui-winit shows it while egui reports an IME field
//! (`PlatformOutput::ime`: a focused `TextEdit`) and hides and shows it
//! again whenever egui asks for an IME interruption, which every
//! `request_focus` is — egui's own `TextEdit` makes one for the tap that
//! focuses it and for every tap that moves its caret, so the keyboard
//! opened, closed and opened again on each. Here the IME output is taken
//! out of the frame's output at its end, so egui-winit never asks winit
//! for the keyboard, and the activity is asked directly
//! (`AndroidApp::show_soft_input` / `hide_soft_input`, what winit's
//! `set_ime_allowed` does) when what wants it changes: a focused egui text
//! field ([`focused_text`]) or a Roblox TextBox with the focus (the game's
//! in Play, a plugin widget's: `gui::share().focus`), which egui-winit
//! never showed it for. It is asked up at once and down once nothing has
//! wanted it for [`HIDE_AFTER`] (a field losing the focus for a frame, or
//! the focus moving from one field to the next, keeps it up, as Android
//! keeps it between EditTexts).
//!
//! The readings are not followed blindly, or the layout flaps. The
//! keyboard's own slide in and out reads as 0 for a few polls, as does the
//! hide and show of a focus move. So while a text field keeps the focus:
//!
//! * the room is made as soon as a height is read (the keyboard slides in
//!   after the focus; its final height is reported from the start);
//! * a 0 reading takes nothing away for [`HIDDEN_FOR`]; past that, with the
//!   field the keyboard was up for still focused, the user hid the keyboard
//!   (Back, its own hide key) — which is invisible to the app otherwise,
//!   and nothing would bring the keyboard back but another tap — so the
//!   field's focus is released here (a TextBox's: FocusLost): the edit
//!   ends (its text kept) as the user meant, the room goes with it, and a
//!   tap starts it again;
//! * another height (the keyboard's suggestions strip or symbols page)
//!   is taken once it has been read [`STEADY_READS`] times running.
//!
//! With no text field focused the first 0 reading takes the room away:
//! the keyboard has been hidden. Each change is logged (`adb logcat -s
//! rustudio`: "soft keyboard ...").

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use egui::{SafeAreaInsets, epaint::MarginF32};

use super::jni::KeyboardAnswer;

/// How often the height is read while it matters.
const POLL: Duration = Duration::from_millis(100);
/// The keyboard read as down this long while the field it was up for
/// keeps the focus: the user hid it. Longer than its slide out and back
/// in for an IME interruption (two animations of about 200 ms) and than
/// the hide and show of a focus move between fields.
const HIDDEN_FOR: Duration = Duration::from_millis(600);
/// A new height while the keyboard is up is taken after this many
/// readings running (one reading can be mid-change).
const STEADY_READS: u32 = 2;
/// A question the main thread hasn't answered in this long is given up
/// on (it is busy with a pause or a configuration change) and asked again.
const GIVE_UP: Duration = Duration::from_secs(1);
/// Points the panels keep however tall the keyboard is.
const MIN_HEIGHT: f32 = 120.0;
/// Nothing has wanted the keyboard this long: it is asked down. Longer
/// than a field's focus is away between two frames or two fields.
const HIDE_AFTER: Duration = Duration::from_millis(250);

/// Set for the frame after the room for the keyboard changed (`reveal`).
static CHANGED: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
pub struct Keyboard {
    polled: Option<Instant>,
    /// The question out to the main thread, and when it was asked.
    asked: Option<(Instant, KeyboardAnswer)>,
    /// The height the panels make room for, in pixels (0: none).
    height_px: i32,
    /// The last height read, and how many times running it was read.
    last_read: Option<(i32, u32)>,
    /// Since when the readings have been 0 while room is made.
    zero_since: Option<Instant>,
    /// What the keyboard is up for.
    up_for: Option<Want>,
    /// Since when something has wanted the keyboard (for the log's timings).
    focus_since: Option<Instant>,
    /// The activity was last asked to show (true) or hide the keyboard.
    shown: bool,
    /// Since when nothing has wanted the keyboard, while `shown`.
    unwanted_since: Option<Instant>,
    /// The keyboard lies over the window (up for a game's TextBox): no
    /// room is made for it — no inset, no `shrink` — and the view slides
    /// instead (`cover`). Decided while something wants the keyboard and
    /// kept until it is down, so the panels don't jump for the frames
    /// between a TextBox's focus going and the keyboard's slide out.
    over: bool,
    /// A press began inside one of last frame's [`keep_focus_in`] rects
    /// and hasn't been released.
    hold_pressed: bool,
    /// egui's focus rule from before that press, put back after it.
    hold_surrender: Option<egui::SurrenderFocusOn>,
}

/// Rects (this frame's) a tap inside which must not take the focus from
/// the text field: the script editor's completion strip and its soft Tab
/// keys act on the focused field — egui's rule is that a click outside
/// the focused widget surrenders its focus, so the field lost the focus
/// for the frame of the tap, and with it the keyboard's mirror (ime.rs
/// ends the session on "no field") and, when the tap took longer than a
/// frame or two, the keyboard ([`HIDE_AFTER`]). Registered every frame
/// they show; read in [`Keyboard::before_frame`], which holds egui's
/// `surrender_focus_on` at `Never` through a press that began inside one
/// (as text_bar.rs does for its bar).
static KEEP_FOCUS: std::sync::Mutex<Vec<egui::Rect>> = std::sync::Mutex::new(Vec::new());

/// A tap inside `rect` (this frame's) leaves the focused text field its
/// focus (the module comment on [`KEEP_FOCUS`]).
pub fn keep_focus_in(rect: egui::Rect) {
    KEEP_FOCUS.lock().unwrap_or_else(|e| e.into_inner()).push(rect);
}

/// What wants the soft keyboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Want {
    /// A focused egui text field.
    Field(egui::Id),
    /// A Roblox TextBox with the focus: the game's in Play, a plugin
    /// widget's (src/gui_input.rs `keyboard` types into it).
    TextBox,
}

/// What wants the keyboard this frame, if anything. `textbox`: a Roblox
/// TextBox has the focus (the caller knows which GUIs are live: the
/// game's in Play, `Editor::gui.focus`; a plugin's,
/// `PluginUi::has_text_focus`).
pub fn wanted(ctx: &egui::Context, textbox: bool) -> Option<Want> {
    if let Some(id) = focused_text(ctx) {
        return Some(Want::Field(id));
    }
    textbox.then_some(Want::TextBox)
}

/// Releases a focused TextBox as a click elsewhere would: the scripts fire
/// FocusLost (`focus_events`), the GUI stops typing into it (`focus_req`).
fn release_textbox() {
    for ch in 0..2u8 {
        let _c = rustudio_engine::gui::share_channel(ch);
        let mut sh = rustudio_engine::gui::share();
        if let Some(f) = sh.focus.take() {
            sh.focus_events.push((f, false, false, None));
            sh.focus_req.push(None);
        }
    }
}

/// Asks for the keyboard: through the input-connection bridge (ime.rs),
/// which shows it for the view that mirrors the focused field; without
/// the bridge, through the activity (what winit's `set_ime_allowed` does),
/// in the framework's fallback mode.
fn soft_input(show: bool) {
    let bridged = if show { super::ime::show() } else { super::ime::hide() };
    if !bridged {
        match super::app() {
            Some(app) if show => app.show_soft_input(true),
            Some(app) => app.hide_soft_input(true),
            None => log::warn!("soft keyboard: no activity to ask"),
        }
    }
    if !show {
        // (the navigation bar came up with the keyboard; API 26–29's
        // sticky flags don't always take it down again)
        super::system_bars::keyboard_closed();
    }
}

/// The focused widget when it is a text field (egui keeps a
/// `TextEditState` for every text field it has drawn).
pub fn focused_text(ctx: &egui::Context) -> Option<egui::Id> {
    ctx.memory(|m| m.focused()).filter(|id| egui::TextEdit::load_state(ctx, *id).is_some())
}

/// True during the frame after the room for the keyboard changed: the
/// focused text field scrolls itself into view then.
pub fn reveal() -> bool {
    CHANGED.load(Ordering::Relaxed)
}

/// Scrolls a text field's caret into view when [`reveal`] says to (the
/// field's `ScrollArea` takes the request; egui's `TextEdit` does the
/// same of its own accord at a keystroke).
pub fn reveal_cursor(ui: &egui::Ui, out: &egui::text_edit::TextEditOutput) {
    if !reveal() {
        return;
    }
    let field = out.response.response.rect;
    let rect = match out.cursor_range {
        Some(range) => {
            let mut r = out.galley.pos_from_cursor(range.primary);
            r.max.y = r.max.y.max(r.min.y + out.galley.rows.first().map_or(16.0, |row| row.rect().height()));
            r.translate(out.galley_pos.to_vec2() - egui::vec2(out.galley.rect.left(), 0.0)).expand2(egui::vec2(0.0, 8.0))
        }
        None => field,
    };
    ui.scroll_to_rect(rect, Some(egui::Align::Center));
}

impl Keyboard {
    /// In `App::raw_input_hook`: reads the keyboard's height when it may
    /// have changed and hands it to egui as the bottom safe-area inset —
    /// unless the keyboard lies over the window (`game_box`: a game's
    /// TextBox has the focus; `textbox` covers it and a plugin GUI's).
    /// With the inset, egui's bottom panels end above the keyboard: the
    /// status bar rose with it and sat over the very box being typed
    /// into, the view slid up by the keyboard's height behind it.
    pub fn before_frame(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput, textbox: bool, game_box: bool) {
        CHANGED.store(false, Ordering::Relaxed);
        self.hold_focus(ctx, raw);
        let want = wanted(ctx, textbox);
        if let Some(w) = want {
            // (a focused egui field comes first in `wanted`: the game's
            // box is typed into only while none is)
            self.over = w == Want::TextBox && game_box;
        }
        let watching = want.is_some() || self.height_px > 0 || self.last_read.is_some_and(|(h, _)| h > 0);
        if watching {
            let now = Instant::now();
            match want {
                Some(_) if self.focus_since.is_none() => self.focus_since = Some(now),
                None => self.focus_since = None,
                _ => {}
            }
            self.take_answer(now);
            if self.asked.is_none() && self.polled.is_none_or(|t| now.duration_since(t) >= POLL) {
                self.polled = Some(now);
                self.asked = Some((now, super::jni::keyboard_height_later()));
            }
            // The keyboard slides in after the focus (and out after it):
            // keep looking while it matters, input or none.
            ctx.request_repaint_after(POLL);
            self.follow(ctx, want, now);
        } else {
            self.polled = None;
            self.asked = None;
            self.last_read = None;
            self.zero_since = None;
            self.up_for = None;
            self.focus_since = None;
        }
        // Every frame, down (zero) or up: egui keeps the last inset given.
        let bottom = if self.over { 0.0 } else { self.room(ctx) };
        raw.safe_area_insets = Some(SafeAreaInsets(MarginF32 { left: 0.0, right: 0.0, top: 0.0, bottom }));
    }

    /// The focus held for the field through a press that began inside one
    /// of last frame's [`keep_focus_in`] rects ([`KEEP_FOCUS`]): egui's
    /// `surrender_focus_on` is `Never` from the press to the frame of its
    /// release (the release's frame is the click's: the rule holds through
    /// it), then what it was.
    fn hold_focus(&mut self, ctx: &egui::Context, raw: &egui::RawInput) {
        let rects = std::mem::take(&mut *KEEP_FOCUS.lock().unwrap_or_else(|e| e.into_inner()));
        let mut released = false;
        for e in &raw.events {
            if let egui::Event::PointerButton { pos, pressed, .. } = e {
                if *pressed {
                    self.hold_pressed = rects.iter().any(|r| r.contains(*pos));
                } else {
                    released = true;
                }
            }
        }
        let hold = self.hold_pressed;
        if released {
            self.hold_pressed = false;
        }
        if hold {
            ctx.options_mut(|o| {
                let was = std::mem::replace(&mut o.input_options.surrender_focus_on, egui::SurrenderFocusOn::Never);
                if self.hold_surrender.is_none() && was != egui::SurrenderFocusOn::Never {
                    self.hold_surrender = Some(was);
                }
            });
        } else if let Some(rule) = self.hold_surrender.take() {
            ctx.options_mut(|o| {
                if o.input_options.surrender_focus_on == egui::SurrenderFocusOn::Never {
                    o.input_options.surrender_focus_on = rule;
                }
            });
        }
    }

    /// The last question's answer, once it has come.
    fn take_answer(&mut self, now: Instant) {
        let Some((when, slot)) = &self.asked else { return };
        let when = *when;
        let answer = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
        match answer {
            Some(answer) => {
                self.asked = None;
                let height = match answer {
                    Ok(h) => h.max(0),
                    Err(e) => {
                        log::warn!("keyboard height: {e}");
                        return;
                    }
                };
                self.last_read = Some(match self.last_read {
                    Some((h, n)) if h == height => (h, n.saturating_add(1)),
                    _ => (height, 1),
                });
            }
            None if now.duration_since(when) >= GIVE_UP => self.asked = None,
            None => {}
        }
    }

    /// Turns the readings into the room made (the module comment).
    fn follow(&mut self, ctx: &egui::Context, want: Option<Want>, now: Instant) {
        let Some((read, runs)) = self.last_read else { return };
        let since_focus = || self.focus_since.map_or(0, |t| now.duration_since(t).as_millis());
        if read > 0 {
            self.zero_since = None;
            if want.is_some() {
                self.up_for = want;
            }
            if self.height_px == 0 {
                log::info!("soft keyboard up: {read} px, {} ms after the focus", since_focus());
                self.set_height(read);
            } else if read != self.height_px && runs >= STEADY_READS {
                log::info!("soft keyboard now {read} px (was {})", self.height_px);
                self.set_height(read);
            }
            return;
        }
        // Read as down.
        if self.height_px == 0 {
            return;
        }
        let Some(want) = want else {
            log::info!("soft keyboard down: nothing to type into");
            self.up_for = None;
            self.zero_since = None;
            self.set_height(0);
            return;
        };
        let since = *self.zero_since.get_or_insert(now);
        if now.duration_since(since) < HIDDEN_FOR {
            // The hide and show of a focus move, or the slide out of a
            // keyboard the user is hiding: the room stays meanwhile.
            return;
        }
        log::info!(
            "soft keyboard hidden by the user ({} ms down{}): {}",
            now.duration_since(since).as_millis(),
            if self.up_for == Some(want) { "" } else { ", after the focus moved to another field" },
            match want {
                Want::Field(_) => "the text field's focus released",
                Want::TextBox => "the TextBox's focus released",
            }
        );
        match want {
            Want::Field(id) => ctx.memory_mut(|m| m.surrender_focus(id)),
            Want::TextBox => release_textbox(),
        }
        self.up_for = None;
        self.zero_since = None;
        self.set_height(0);
    }

    /// At the end of the frame: keeps egui's IME output from egui-winit
    /// and asks the activity for the keyboard when what wants it changes
    /// (the module comment).
    pub fn end_frame(&mut self, ctx: &egui::Context, textbox: bool) {
        ctx.output_mut(|o| o.ime = None);
        let want = wanted(ctx, textbox);
        let now = Instant::now();
        match (want, self.shown) {
            (Some(w), false) => {
                self.shown = true;
                self.unwanted_since = None;
                log::info!("soft keyboard asked up for {}", match w {
                    Want::Field(_) => "a text field",
                    Want::TextBox => "a TextBox",
                });
                soft_input(true);
            }
            (Some(_), true) => self.unwanted_since = None,
            (None, true) => {
                // (not while a finger is down: a tap's release may put the
                // focus back — a completion taken, a soft key — and the
                // keyboard's slide out and in for it is what's unwanted)
                if ctx.input(|i| i.pointer.any_down()) {
                    self.unwanted_since = None;
                    ctx.request_repaint_after(HIDE_AFTER);
                    return;
                }
                let since = *self.unwanted_since.get_or_insert(now);
                let for_ms = now.duration_since(since);
                if for_ms >= HIDE_AFTER {
                    self.shown = false;
                    self.unwanted_since = None;
                    log::info!(
                        "soft keyboard asked down: nothing to type into for {} ms (focus now {})",
                        for_ms.as_millis(),
                        match ctx.memory(|m| m.focused()) {
                            Some(id) => format!("{id:?}, not a text field"),
                            None => "nothing".to_string(),
                        }
                    );
                    soft_input(false);
                } else {
                    ctx.request_repaint_after(HIDE_AFTER - for_ms);
                }
            }
            (None, false) => {}
        }
    }

    fn set_height(&mut self, px: i32) {
        if px != self.height_px {
            // Down by the keyboard's own Back as well as by `soft_input`:
            // the navigation bar it brought up goes with it.
            if px <= 0 && self.height_px > 0 {
                super::system_bars::keyboard_closed();
                self.over = false;
            }
            self.height_px = px;
            CHANGED.store(true, Ordering::Relaxed);
        }
    }

    /// The keyboard's height in points (0 while no room is made).
    pub fn height(&self, ctx: &egui::Context) -> f32 {
        if self.height_px <= 0 {
            return 0.0;
        }
        self.height_px as f32 / ctx.pixels_per_point().max(0.1)
    }

    /// The keyboard's height plus the band of the text bar above it
    /// (text_bar.rs: Cut / Copy / Paste / Select all), in points: what the
    /// window's bottom gives up while the keyboard is up (0 while it is
    /// down). The bar is always up, including with the bridge: a phone's
    /// keyboard need not offer its own paste actions.
    pub fn room(&self, ctx: &egui::Context) -> f32 {
        let keyboard = self.height(ctx);
        if keyboard <= 0.0 {
            0.0
        } else if super::text_bar::in_use() {
            keyboard + super::text_bar::HEIGHT
        } else {
            keyboard
        }
    }

    /// What the keyboard and the bar cover of the window while the
    /// keyboard lies over it (a game's TextBox: `Editor::keyboard_cover`,
    /// the view slides by it); 0 while room is made for them instead.
    pub fn cover(&self, ctx: &egui::Context) -> f32 {
        if self.over { self.room(ctx) } else { 0.0 }
    }

    /// An editor text field is being typed into with room made for the
    /// keyboard (not a game's TextBox, which the keyboard lies over): the
    /// typing layout, `RustudioApp::ui` leaves the menu bar, the test bar
    /// and the ribbon out while it lasts. A landscape phone's window is
    /// some 410 points tall; the keyboard and the text bar take 200 of
    /// them and the chrome 140 of the rest, which left the script editor
    /// two or three lines, the one typed into as often as not off them
    /// (the 2026-11-25 report, again: "still can't see what I'm typing").
    pub fn typing_layout(&self, ctx: &egui::Context) -> bool {
        !self.over && self.room(ctx) > 0.0
    }

    /// At the start of `App::ui`: the root `Ui` ends above the keyboard
    /// and the bar, so every panel laid out in it does too (not while the
    /// keyboard lies over the window: `cover`).
    pub fn shrink(&self, ui: &mut egui::Ui) {
        let room = self.room(ui.ctx());
        if room <= 0.0 || self.over {
            return;
        }
        let full = ui.max_rect().height();
        ui.set_max_height((full - room).max(MIN_HEIGHT.min(full)));
        // The room given up is painted the window's colour: nothing else
        // draws there (the panels end above it), and the keyboard doesn't
        // always cover all of it — the moment between its going and the
        // next height reading, the band of the text bar while the bar is
        // not up — which left a black strip over the panels' bottom.
        let max = ui.max_rect();
        let band = egui::Rect::from_min_max(egui::pos2(max.min.x, max.max.y), egui::pos2(max.max.x, max.min.y + full));
        if band.is_positive() {
            ui.ctx().layer_painter(egui::LayerId::background()).rect_filled(band, 0.0, rustudio_engine::theme::BG);
        }
    }

}

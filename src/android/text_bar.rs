//! Cut / Copy / Paste / Select all on a bar above the soft keyboard, while
//! it is up for something: an egui text field (a Properties value, the
//! script editor, the Output filter) or a Roblox TextBox (a game's in
//! Play, a plugin widget's). The soft keyboard has no Ctrl and its own
//! paste key never reaches a NativeActivity (clipboard.rs), the text
//! fields' long-press menu (text_menu.rs) has to be known of, and a
//! game's TextBox has no menu at all — the bar is always there to see.
//!
//! Its entries are the events the keyboard shortcuts would be, fed to
//! egui through `App::raw_input_hook` on the next frame: `Event::Cut` /
//! `Copy` / `Paste(the clipboard's text)` / Ctrl+A for a text field (egui's
//! `TextEdit` answers them), `Copy` / `Paste` for a TextBox (gui_input.rs
//! `Gui::keyboard`: Paste types the text in, Copy copies the box's text).
//! The copies reach the system clipboard through clipboard.rs, and its
//! text is asked for when the bar comes up.
//!
//! The bar stands in a band of [`HEIGHT`] points right above the
//! keyboard: part of the room keyboard.rs makes (`Keyboard::room`), so the
//! panels end above the bar, or — for a game's TextBox, where the keyboard
//! lies over the view — part of what the view slides up for (`cover`). The
//! band is outside egui's safe area, so the bar's `Area` is left
//! unconstrained. A tap on it is egui's, not the game's (the touch layer
//! leaves fingers on a foreground layer to egui), and the field it is for
//! keeps the focus: egui's rule gives the focus up at a click outside the
//! focused widget — judged as the field draws, before the bar does, so
//! the bar (drawn for a focused field) would be gone in the very frame
//! the tap completes in, its button never clicked — hence, from a press
//! that begins on the bar to its release, egui is told to surrender the
//! focus never ([`TextBar::before_frame`]); and should the field lack the
//! focus all the same, it is asked back for the frame the event is handled
//! in (as the long-press menu does).
//!
//! The bar is always up while the keyboard is up, also with the
//! input-connection bridge (ime.rs). A 2026-11 Android 13 report had a
//! keyboard whose clipboard panel and Select all never reached the field
//! through its `InputConnection`, and a game TextBox has no long-press menu
//! at all: a band of the screen with the four entries is the reliable way
//! to paste. The bridge's own actions still work when a keyboard provides
//! them ([`in_use`]); the bar is not a fallback for a failed bridge only.

use egui::{Align, Event, Id, Key, Layout, Modifiers, Order, Pos2, Rect, SurrenderFocusOn};

use super::keyboard::{Keyboard, Want};

/// The bar's band, in points.
pub const HEIGHT: f32 = 34.0;

/// Whether the bar (and its band in `Keyboard::room`) is in use. Always:
/// the keyboard's own context-menu actions are not available on every
/// phone (a 2026-11 Android 13 report), and a Roblox TextBox has no other
/// menu ([`super::text_menu`]).
pub fn in_use() -> bool {
    true
}

const ID: &str = "rustudio text bar";

#[derive(Default)]
pub struct TextBar {
    /// Events for the next frame.
    queued: Vec<Event>,
    /// The bar showed last frame.
    shown: bool,
    /// Where it stood last frame.
    rect: Option<Rect>,
    /// A press began on the bar and hasn't been released.
    pressed: bool,
    /// egui's focus rule from before the press, put back after it.
    surrender: Option<SurrenderFocusOn>,
}

impl TextBar {
    /// In `App::raw_input_hook`: the entry chosen last frame; and the
    /// focus held for the field through a press on the bar (the module
    /// comment).
    pub fn before_frame(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        raw.events.append(&mut self.queued);
        let mut released = false;
        for e in &raw.events {
            if let Event::PointerButton { pos, pressed, .. } = e {
                if *pressed {
                    self.pressed = self.shown && self.rect.is_some_and(|r| r.contains(*pos));
                } else {
                    released = true;
                }
            }
        }
        // (the release's frame is the click's: the rule holds through it)
        let hold = self.pressed;
        if released {
            self.pressed = false;
        }
        if hold {
            ctx.options_mut(|o| {
                let was = std::mem::replace(&mut o.input_options.surrender_focus_on, SurrenderFocusOn::Never);
                if self.surrender.is_none() && was != SurrenderFocusOn::Never {
                    self.surrender = Some(was);
                }
            });
        } else if let Some(rule) = self.surrender.take() {
            ctx.options_mut(|o| {
                if o.input_options.surrender_focus_on == SurrenderFocusOn::Never {
                    o.input_options.surrender_focus_on = rule;
                }
            });
        }
    }

    /// At the end of `App::ui`, after every panel: the bar while the
    /// keyboard is up (`keyboard`) for `want`.
    pub fn end_frame(&mut self, ctx: &egui::Context, keyboard: &Keyboard, want: Option<Want>) {
        let keyboard_h = keyboard.height(ctx);
        if !in_use() {
            self.shown = false;
            self.rect = None;
            return;
        }
        let Some(want) = want.filter(|_| keyboard_h > 0.0) else {
            self.shown = false;
            self.rect = None;
            return;
        };
        if !self.shown {
            self.shown = true;
            // (the clipboard's text, for Paste: read now, so the button is
            // enabled at once)
            super::clipboard::prime();
            log::info!(
                "text bar: up for {}",
                match want {
                    Want::Field(_) => "a text field",
                    Want::TextBox => "a TextBox",
                }
            );
        }
        let screen = ctx.viewport_rect();
        let rect = Rect::from_min_max(
            Pos2::new(screen.min.x, screen.max.y - keyboard_h - HEIGHT),
            Pos2::new(screen.max.x, screen.max.y - keyboard_h),
        );
        self.rect = Some(rect);
        let clipboard = super::clipboard::text();
        let (field, selection) = match want {
            Want::Field(id) => (
                Some(id),
                egui::TextEdit::load_state(ctx, id).and_then(|s| s.cursor.char_range()).is_some_and(|r| !r.is_empty()),
            ),
            Want::TextBox => (None, false),
        };
        let mut chosen: Option<Event> = None;
        egui::Area::new(Id::new(ID))
            .order(Order::Foreground)
            .fixed_pos(rect.min)
            .constrain(false)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_min_size(rect.size());
                ui.set_max_size(rect.size());
                egui::Frame::new()
                    .fill(rustudio_engine::theme::MENU_BAR)
                    .stroke(egui::Stroke::new(1.0, rustudio_engine::theme::DIVIDER))
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.set_min_width(rect.width() - 16.0);
                        ui.set_max_height(HEIGHT - 8.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            ui.spacing_mut().button_padding = egui::vec2(10.0, 3.0);
                            let item = |ui: &mut egui::Ui, label: &str, enabled: bool| {
                                ui.add_enabled(enabled, egui::Button::new(label)).clicked()
                            };
                            // (right to left: Select all stands at the right edge)
                            if field.is_some() && item(ui, "Select all", true) {
                                chosen = Some(Event::Key {
                                    key: Key::A,
                                    physical_key: None,
                                    pressed: true,
                                    repeat: false,
                                    modifiers: Modifiers::COMMAND,
                                });
                            }
                            if item(ui, "Paste", clipboard.is_some()) {
                                // (what is on the clipboard at this moment)
                                chosen = Some(Event::Paste(super::clipboard::text_now().or_else(|| clipboard.clone()).unwrap_or_default()));
                            }
                            if item(ui, "Copy", field.is_none() || selection) {
                                chosen = Some(Event::Copy);
                            }
                            if field.is_some() && item(ui, "Cut", selection) {
                                chosen = Some(Event::Cut);
                            }
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                ui.add_space(2.0);
                                ui.weak(match want {
                                    Want::Field(_) if selection => "Text — selection",
                                    Want::Field(_) => "Text",
                                    Want::TextBox => "TextBox",
                                });
                            });
                        });
                    });
            });
        let Some(event) = chosen else { return };
        log::info!(
            "text bar: {}",
            match &event {
                Event::Cut => "cut".to_string(),
                Event::Copy => "copy".to_string(),
                Event::Paste(t) => format!("paste {} chars", t.chars().count()),
                _ => "select all".to_string(),
            }
        );
        // A key press comes with its release, or egui holds the key.
        if let Event::Key { key, modifiers, .. } = &event {
            let up = Event::Key { key: *key, physical_key: None, pressed: false, repeat: false, modifiers: *modifiers };
            self.queued.push(event.clone());
            self.queued.push(up);
        } else {
            self.queued.push(event);
        }
        // The field takes the event next frame: it keeps the focus whatever
        // this frame's tap did. (Asked only when it lacks it: egui counts a
        // request as an IME interruption, which hides and shows the soft
        // keyboard again.)
        if let Some(id) = field {
            if !ctx.memory(|m| m.has_focus(id)) {
                ctx.memory_mut(|m| m.request_focus(id));
            }
        }
        ctx.request_repaint();
    }
}

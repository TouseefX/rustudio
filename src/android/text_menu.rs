//! Cut / Copy / Paste / Select all for text fields on a touch screen. The
//! soft keyboard has no Ctrl, egui's text fields have no menu of their own
//! and egui-winit has no clipboard on Android, so a long press on a field
//! (src/touch_input.rs: a right click) opens this menu, and its entries
//! are the events the keyboard shortcuts would be: `Event::Cut`, `Copy`,
//! `Paste(the system clipboard's text)` and Ctrl+A, fed to egui through
//! `App::raw_input_hook` on the next frame. The copies reach the system
//! clipboard through src/android/clipboard.rs, which also keeps the text
//! Paste pastes. The same four stand on the bar above the keyboard
//! (text_bar.rs), which needs no long press to be found.
//!
//! Two things egui does on its own are undone here: a right click moves
//! the caret to the finger (the selection from before the press comes back
//! for the menu), and a click outside a focused field takes its focus (the
//! menu's tap would, so focus is kept while the menu is up).

use egui::text::CCursorRange;
use egui::{Event, Id, Key, Modifiers, Pos2};

use super::keyboard::focused_text;

#[derive(Default)]
pub struct TextMenu {
    open: Option<Open>,
    /// The focused field and its selection at the end of the last frame.
    prev: Option<(Id, Option<CCursorRange>)>,
    /// Events for the next frame.
    queued: Vec<Event>,
}

struct Open {
    field: Id,
    at: Pos2,
    /// The system clipboard's text when the menu opened.
    clipboard: Option<String>,
    /// egui's focus rule from before the menu, put back when it closes.
    surrender: egui::SurrenderFocusOn,
}

const ID: &str = "rustudio text menu";

impl TextMenu {
    /// In `App::raw_input_hook`: the entry chosen last frame.
    pub fn before_frame(&mut self, raw: &mut egui::RawInput) {
        raw.events.append(&mut self.queued);
    }

    /// At the end of `App::ui`, after every text field was drawn.
    pub fn end_frame(&mut self, ctx: &egui::Context) {
        let focused = focused_text(ctx);
        // A long press goes out as a secondary click alone (touch_input.rs),
        // so an unfocused field never got the primary click that would have
        // focused it: the field from the frame before is the one under the
        // finger. Its rect is checked below, so a press elsewhere opens no
        // menu on it.
        let candidate = focused.or_else(|| self.prev.map(|(id, _)| id));
        let response = candidate.and_then(|id| ctx.read_response(id));
        // A right click (a long press) on a text field: by place, not
        // by the field's response, since the Properties panel lays its
        // own click catcher over its edit box (properties.rs `text_cell`).
        let (secondary, at) = ctx.input(|i| (i.pointer.button_clicked(egui::PointerButton::Secondary), i.pointer.interact_pos()));
        let clicked = match (candidate, response.as_ref(), at) {
            (Some(id), Some(r), Some(p)) if secondary && self.open.is_none() && r.rect.expand(4.0).contains(p) => Some((id, r)),
            _ => None,
        };
        if let Some((id, r)) = clicked {
            log::info!("text menu: open for the focused text field");
            // The press moved the caret: the selection from before it is
            // what the menu is for.
            match (self.prev, egui::TextEdit::load_state(ctx, id)) {
                (Some((prev_id, range)), Some(mut state)) if prev_id == id => {
                    state.cursor.set_char_range(range);
                    state.store(ctx, id);
                }
                _ => {}
            }
            let at = r.interact_pointer_pos().or_else(|| ctx.pointer_latest_pos()).unwrap_or(r.rect.left_bottom());
            // (the clipboard's text, read now; should that fail, the main
            // thread is asked and Paste comes alive when its answer lands)
            super::clipboard::prime();
            let clipboard = super::clipboard::text();
            let surrender = ctx.options_mut(|o| {
                std::mem::replace(&mut o.input_options.surrender_focus_on, egui::SurrenderFocusOn::Never)
            });
            // The long press alone never focused the field; the menu's
            // Cut / Copy / Paste events are for it, so give it the focus
            // now (and hold it through the menu below).
            if focused != Some(id) && !ctx.memory(|m| m.has_focus(id)) {
                ctx.memory_mut(|m| m.request_focus(id));
            }
            // Open in egui's memory, as the context menus are: the touch
            // layer asks `Popup::is_any_open` whether a long press came to
            // something before it lets the finger drag.
            egui::Popup::open_id(ctx, Id::new(ID));
            self.open = Some(Open { field: id, at, clipboard, surrender });
        }

        if let Some(open) = self.open.as_mut() {
            if open.clipboard.is_none() {
                open.clipboard = super::clipboard::text();
            }
        }
        if let Some(open) = self.open.as_ref() {
            let id = Id::new(ID);
            let mut keep = ctx.memory(|m| m.has_focus(open.field)) && egui::Popup::is_id_open(ctx, id);
            let selection = egui::TextEdit::load_state(ctx, open.field)
                .and_then(|s| s.cursor.char_range())
                .is_some_and(|r| !r.is_empty());
            let mut chosen: Option<Event> = None;
            if keep {
                let layer = ctx.layer_id_at(open.at).unwrap_or_else(egui::LayerId::background);
                egui::Popup::new(id, ctx.clone(), egui::PopupAnchor::Position(open.at), layer)
                    .kind(egui::PopupKind::Menu)
                    .layout(egui::Layout::top_down_justified(egui::Align::Min))
                    .style(egui::containers::menu::menu_style)
                    .gap(0.0)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
                    .open_memory(None)
                    .show(|ui| {
                        let item = |ui: &mut egui::Ui, label: &str, enabled: bool| {
                            ui.add_enabled(enabled, egui::Button::new(label).frame(false)).clicked()
                        };
                        if item(ui, "Cut", selection) {
                            chosen = Some(Event::Cut);
                        }
                        if item(ui, "Copy", selection) {
                            chosen = Some(Event::Copy);
                        }
                        if item(ui, "Paste", open.clipboard.is_some()) {
                            // (what is on the clipboard at this moment)
                            chosen = Some(Event::Paste(super::clipboard::text_now().or_else(|| open.clipboard.clone()).unwrap_or_default()));
                        }
                        if item(ui, "Select all", true) {
                            chosen = Some(Event::Key {
                                key: Key::A,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers: Modifiers::COMMAND,
                            });
                        }
                    });
                // A tap (on an entry or outside) or Escape closed it.
                keep = egui::Popup::is_id_open(ctx, id);
            }
            if let Some(event) = chosen {
                keep = false;
                log::info!(
                    "text menu: {}",
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
                // The field takes the event next frame: it keeps the focus
                // whatever this frame's tap did. (Asked only when it lacks
                // it: egui counts a request as an IME interruption, which
                // hides and shows the soft keyboard again.)
                if focused != Some(open.field) {
                    ctx.memory_mut(|m| m.request_focus(open.field));
                }
            }
            if !keep {
                let surrender = open.surrender;
                ctx.options_mut(|o| o.input_options.surrender_focus_on = surrender);
                egui::Popup::close_id(ctx, id);
                self.open = None;
            }
            ctx.request_repaint();
        }

        self.prev = focused.map(|id| (id, egui::TextEdit::load_state(ctx, id).and_then(|s| s.cursor.char_range())));
    }
}

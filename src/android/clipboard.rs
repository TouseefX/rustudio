//! The system clipboard. egui-winit has no clipboard backend on Android
//! (its copies stay in a string inside the process), so copies are
//! mirrored to `ClipboardManager` after each frame and a paste reads it
//! back first ([`jni::clipboard_set`](super::jni::clipboard_set) /
//! [`jni::clipboard_get`](super::jni::clipboard_get)).
//!
//! egui-winit swallows the paste shortcut while its own string is empty
//! (no `Event::Paste` at all), so the first frame gives it a marker text:
//! every paste then arrives as `Paste(marker)` and is swapped for the
//! system clipboard's text here. The soft keyboard's own paste key commits
//! text through the IME and needs none of this.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// egui-winit's stand-in clipboard text (never shown, never mirrored).
const SEED: &str = "\u{200b}rustudio-clipboard-seed\u{200b}";

/// The last text this app put on the clipboard (not copied back out again).
static LAST_SET: Mutex<Option<String>> = Mutex::new(None);
static SEEDED: AtomicBool = AtomicBool::new(false);

/// In `App::raw_input_hook`: a paste takes the system clipboard's text when
/// it has one; the marker alone (nothing ever copied) pastes nothing.
pub fn before_frame(raw: &mut egui::RawInput) {
    if !raw.events.iter().any(|e| matches!(e, egui::Event::Paste(_))) {
        return;
    }
    let system = match super::jni::clipboard_get() {
        Ok(Some(s)) if !s.is_empty() => Some(s),
        Ok(_) => None,
        Err(e) => {
            log::warn!("clipboard: {e}");
            None
        }
    };
    raw.events.retain_mut(|e| {
        let egui::Event::Paste(text) = e else { return true };
        match &system {
            Some(s) => {
                *text = s.clone();
                true
            }
            None => text.as_str() != SEED,
        }
    });
}

/// At the end of `App::update`: text egui was asked to copy this frame
/// (`Context::copy_text`, Ctrl+C in a text field, Explorer copies) goes to
/// the system clipboard as well.
pub fn after_frame(ctx: &egui::Context) {
    if !SEEDED.swap(true, Ordering::SeqCst) {
        ctx.copy_text(SEED.to_string());
        return;
    }
    let copied: Option<String> = ctx.output(|o| {
        o.commands.iter().rev().find_map(|c| match c {
            egui::OutputCommand::CopyText(s) if !s.is_empty() && s != SEED => Some(s.clone()),
            _ => None,
        })
    });
    let Some(text) = copied else { return };
    if LAST_SET.lock().unwrap().as_deref() == Some(text.as_str()) {
        return;
    }
    match super::jni::clipboard_set(&text) {
        Ok(()) => *LAST_SET.lock().unwrap() = Some(text),
        Err(e) => log::warn!("clipboard: {e}"),
    }
}

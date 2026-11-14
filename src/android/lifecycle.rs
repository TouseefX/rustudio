//! The activity's end, which winit 0.30 never passes on (`MainEvent::Destroy`
//! is logged and dropped), and the Back button.
//!
//! `NativeActivity.onDestroy` runs on the Java main thread and, in
//! android-activity's glue, waits there until `android_main` has returned.
//! With the destroy notice dropped, the editor's loop never returns, the
//! main thread stays blocked (an ANR after a while) and a launch in the
//! meantime, which lands in this same process, shows a blank window with
//! no frames until the system has killed the process: the "black screen
//! until I leave and come back" of the first device runs. So [`hook`]
//! wraps the glue's `onDestroy`: it asks the loop to close (a save and a
//! clean return, when the loop still runs frames), waits a moment, and
//! leaves the process otherwise. [`run`](super::run) leaves the process
//! after the loop too, so `android_main` never runs twice in one process
//! (the statics here and in [`super`] are per activity).
//!
//! The hook needs the `ANativeActivity` itself, which android-activity's
//! `AndroidApp` doesn't give out (`activity_as_ptr` is the Java `Activity`
//! reference, a JNI handle: reading it as the struct was the first APK's
//! crash at start). The library's entry point has it: manifest.yaml names
//! `rustudio_on_create` (crates/rustudio-android) as the activity's
//! `android.app.func_name`, and that calls the glue's
//! `ANativeActivity_onCreate` and then [`hook`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The editor's egui context, for the hook's close request.
static CTX: Mutex<Option<egui::Context>> = Mutex::new(None);
/// The glue's own `onDestroy`.
static ORIGINAL: Mutex<Option<unsafe extern "C" fn(*mut ndk_sys::ANativeActivity)>> = Mutex::new(None);
/// `run_native` has returned.
pub static LOOP_DONE: AtomicBool = AtomicBool::new(false);
/// How long the hook gives the loop to close on its own.
const GRACE: Duration = Duration::from_millis(1500);

/// Keeps the context the hook will close.
pub fn note_context(ctx: &egui::Context) {
    *CTX.lock().unwrap() = Some(ctx.clone());
}

/// Puts [`on_destroy`] in front of the glue's `onDestroy` callback. Called
/// on the Java main thread by the library's entry point, right after the
/// glue's `ANativeActivity_onCreate` returned.
///
/// # Safety
/// `activity` must be the `ANativeActivity` the framework passed to the
/// entry point (it stays valid until `onDestroy` returns, and the hook is
/// the one that returns from it), with the glue's callbacks installed.
pub unsafe fn hook(activity: *mut std::ffi::c_void) {
    let activity = activity.cast::<ndk_sys::ANativeActivity>();
    if activity.is_null() {
        log::error!("destroy hook: no ANativeActivity");
        return;
    }
    // SAFETY: the live ANativeActivity; `callbacks` points at the
    // framework's own struct (NativeCode::callbacks) for its lifetime, and
    // nothing else writes it after onCreate.
    unsafe {
        let callbacks = (*activity).callbacks;
        if callbacks.is_null() {
            log::error!("destroy hook: no callbacks");
            return;
        }
        let original = (*callbacks).onDestroy;
        if original.is_none() {
            log::warn!("destroy hook: the glue installed no onDestroy");
        }
        *ORIGINAL.lock().unwrap() = original;
        (*callbacks).onDestroy = Some(on_destroy);
    }
    log::info!("destroy hook installed");
}

unsafe extern "C" fn on_destroy(activity: *mut ndk_sys::ANativeActivity) {
    log::info!("activity destroyed: closing the editor");
    if let Some(ctx) = CTX.lock().unwrap().clone() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        ctx.request_repaint();
    }
    let since = Instant::now();
    while !LOOP_DONE.load(Ordering::SeqCst) && since.elapsed() < GRACE {
        std::thread::sleep(Duration::from_millis(25));
    }
    if LOOP_DONE.load(Ordering::SeqCst) {
        // The loop returned: the glue's onDestroy no longer has to wait.
        let original = *ORIGINAL.lock().unwrap();
        if let Some(f) = original {
            // SAFETY: the glue's own callback, with the activity it was
            // registered for.
            unsafe { f(activity) };
        }
        return;
    }
    // Paused activities run no frames (winit), so the close request can't
    // be seen: the window's state went to disk when the surface was lost
    // (App::save) and a fresh process serves the next launch.
    log::warn!("the editor's loop didn't end in {GRACE:?}: leaving the process");
    std::process::exit(0);
}

/// The Back button (`Key::BrowserBack` through winit): closes what is open
/// on top (as Escape does) and otherwise sends the app to the background,
/// where it stays ready, instead of tearing the activity down.
pub fn back_pressed(ctx: &egui::Context, raw: &mut egui::RawInput) {
    let mut back = false;
    raw.events.retain(|e| match e {
        egui::Event::Key { key: egui::Key::BrowserBack, pressed, .. } => {
            back |= *pressed;
            false
        }
        _ => true,
    });
    if !back {
        return;
    }
    let something_open = egui::Popup::is_any_open(ctx) || ctx.memory(|m| m.focused().is_some());
    if something_open {
        for pressed in [true, false] {
            raw.events.push(egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        return;
    }
    if let Err(e) = super::jni::move_task_to_back() {
        log::warn!("back: {e}");
    }
}

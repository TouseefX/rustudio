//! The panel's refresh rate asked for the frame limit (Performance >
//! Editor frame limit, 60 on a phone by default: app.rs `pace_frame`).
//! A phone whose panel switches between 60 and 120 Hz runs the editor at
//! 120 unless the app says otherwise; `ANativeWindow_setFrameRate` (API
//! 30) is the app's vote, and SurfaceFlinger moves the panel to 60 when
//! the user's display setting lets it ("Adaptive" / "Auto" motion
//! smoothness) — each frame then presents on time with nothing slept, at
//! half the panel's power. Looked up at run time: the symbol is in
//! libnativewindow.so from Android 11; an older phone has none, and the
//! limit is slept for instead. A fixed 120 Hz setting ignores the vote,
//! and the sleep covers that too.
use std::sync::OnceLock;

type SetFrameRate = unsafe extern "C" fn(*mut std::ffi::c_void, f32, i8) -> i32;
/// ANATIVEWINDOW_FRAME_RATE_COMPATIBILITY_DEFAULT: no particular
/// relation between the content and the rate (FIXED_SOURCE is video).
const COMPATIBILITY_DEFAULT: i8 = 0;

fn symbol() -> Option<SetFrameRate> {
    static SYM: OnceLock<Option<usize>> = OnceLock::new();
    let addr = *SYM.get_or_init(|| unsafe {
        let name = c"libnativewindow.so";
        let mut lib = libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_NOLOAD);
        if lib.is_null() {
            lib = libc::dlopen(name.as_ptr(), libc::RTLD_NOW);
        }
        if lib.is_null() {
            return None;
        }
        let f = libc::dlsym(lib, c"ANativeWindow_setFrameRate".as_ptr());
        (!f.is_null()).then_some(f as usize)
    });
    // SAFETY: the address came from dlsym for a function of this signature
    // (the NDK's ANativeWindow_setFrameRate).
    addr.map(|p| unsafe { std::mem::transmute::<usize, SetFrameRate>(p) })
}

/// Votes `hz` for the activity's window (0: the system's choice, the
/// default). None when the API (Android 10 and before) or the window
/// (between surfaces) is missing; else whether the call was accepted.
pub fn vote(hz: f32) -> Option<bool> {
    let f = symbol()?;
    let window = super::app()?.native_window()?;
    // SAFETY: a live window from the activity; the function is the NDK's.
    let status = unsafe { f(window.ptr().as_ptr().cast(), hz.max(0.0), COMPATIBILITY_DEFAULT) };
    Some(status == 0)
}

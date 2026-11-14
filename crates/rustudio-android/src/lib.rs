//! The Android app around the rustudio library.
//!
//! `NativeActivity` (declared in manifest.yaml, `android.app.lib_name` =
//! `rustudio_android`) dlopens this library and calls `ANativeActivity_onCreate`,
//! which android-activity's glue defines; the glue runs [`android_main`] on
//! its own thread with the `AndroidApp` handle. Everything after that is
//! `rustudio::android::run`.
//!
//! On every other target this crate is empty (see Cargo.toml).

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: rustudio::android::AndroidApp) {
    rustudio::android::run(app);
}

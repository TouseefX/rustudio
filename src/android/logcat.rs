//! Logging on Android: `log` records go to logcat under the tag `rustudio`
//! (eframe's, wgpu's and winit's lines included), and a panic's message is
//! logged before the crash report is written. stdout and stderr are already
//! forwarded to logcat by android-activity (tag `RustStdoutStderr`), so
//! `println!` / `eprintln!` in the desktop code show up too.
//!
//! `adb logcat -s rustudio RustStdoutStderr` follows the app.

use std::sync::Once;

static INSTALLED: Once = Once::new();

pub fn install() {
    INSTALLED.call_once(|| {
        // Debug builds log at Debug (wgpu's adapter / surface choices among
        // it); release builds at Info.
        let level = if cfg!(debug_assertions) { log::LevelFilter::Debug } else { log::LevelFilter::Info };
        android_logger::init_once(android_logger::Config::default().with_max_level(level).with_tag("rustudio"));
        // crash::install() chains its own hook after this one (it takes the
        // hook that is current when it runs), so the message is in logcat
        // before the report is written, even if writing it fails.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log::error!("panic: {info}");
            previous(info);
        }));
    });
}

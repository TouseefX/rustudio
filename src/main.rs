//! rustudio's entry point. The program itself is the library (`src/lib.rs`),
//! so `tests/` and `examples/` can use `rustudio::…` instead of including
//! source files by path, and the Android `cdylib` (crates/rustudio-android)
//! shares it.
//!
//! This file stays a binary on purpose: the attributes below are the ones that
//! describe how the *executable* is presented to the OS.

// Launch as a GUI application on Windows without opening a console window.
// (Here, not in the library: it is the binary the OS starts.)
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

// Counts bytes allocated per profiler scope while the profiler asks for it
// (one atomic load per allocation otherwise).
#[global_allocator]
static ALLOC: rustudio_core::profiler::alloc::CountingAlloc = rustudio_core::profiler::alloc::CountingAlloc;

fn should_show_startup_dialog() -> bool {
    if std::env::var_os("RUSTUDIO_NO_DIALOG").is_some() {
        return false;
    }
    let automated = std::env::args().any(|a| {
        matches!(
            a.as_str(),
            "--headless"
                | "--no-render"
                | "--selftest"
                | "--bench"
                | "--play-only"
                | "--ui-shots"
                | "--fidelity"
                | "--help"
                | "-h"
                | "--version"
                | "-V"
        )
    });
    if automated {
        return false;
    }
    // Windows GUI builds detach stderr (windows_subsystem = "windows"), so
    // always surface startup failures in a dialog; on Unix only when not
    // launched from an interactive terminal.
    cfg!(windows) || !std::io::IsTerminal::is_terminal(&std::io::stderr())
}

fn show_startup_error(error: &eframe::Error) {
    let mut body = format!("RusTudio could not start:\n\n{error}");
    if let Some(log_path) = rustudio_core::diagnostics::path() {
        body.push_str(&format!("\n\nDiagnostic log:\n{}", log_path.display()));
    }
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("RusTudio failed to start")
        .set_description(&body)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

fn main() -> eframe::Result<()> {
    rustudio_core::profiler::alloc::CountingAlloc::mark_installed();
    rustudio_core::profiler::set_build(option_env!("RUSTUDIO_GIT_SHA").unwrap_or(""));
    let result = rustudio::run();
    match &result {
        Ok(()) => log::info!("RusTudio exited"),
        Err(error) => {
            log::error!("RusTudio startup or event loop failed: {error}");
            if should_show_startup_dialog() {
                show_startup_error(error);
            }
        }
    }
    result
}

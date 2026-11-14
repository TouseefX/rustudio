//! The desktop binary (Linux, macOS, Windows): everything is in the library
//! (src/lib.rs), shared with the Android `cdylib` in crates/rustudio-android.

fn main() -> eframe::Result<()> {
    rustudio::desktop_main()
}

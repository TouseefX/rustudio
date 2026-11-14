//! The Android app. `NativeActivity` loads `librustudio_android.so`
//! (crates/rustudio-android), whose `android_main` calls [`run`] with the
//! `AndroidApp` handle android-activity made for it; everything after that
//! is the same `RustudioApp` as on a desktop, in eframe's Android backend.
//!
//! What differs from a desktop and lives here:
//!
//! * [`paths`](self): the app's own folders stand in for `~/.local/share`
//!   and friends (`Context.getFilesDir()`, `getExternalFilesDir()`,
//!   `getCacheDir()`), and `api-dump.json` is unpacked from the APK's assets
//!   ([`assets`]).
//! * Logging: `log` lines, and anything printed to stdout / stderr, go to
//!   logcat under the tag `rustudio` ([`logcat`]).
//! * The lifecycle: eframe's wgpu backend drops the window and its surface
//!   in `suspended()` and makes them again in `resumed()`, so the GPU
//!   objects survive the activity going to the background; a rotation
//!   arrives as a resize and the swapchain is reconfigured.
//! * The Roblox services with a device behind them ([`jni`]: Vibrator,
//!   orientation, window insets, connectivity; [`sensors`]: accelerometer,
//!   gyroscope, rotation; [`storage`]: files opened through the system
//!   picker / share sheet).

mod assets;
pub mod clipboard;
pub mod jni;
pub mod lifecycle;
mod logcat;
pub mod sensors;
pub mod storage;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// android-activity's handle on the activity (the same type eframe and
/// winit use, through winit's re-export so the versions can't drift).
pub use winit::platform::android::activity::AndroidApp;

static APP: OnceLock<AndroidApp> = OnceLock::new();

/// The running activity's handle, once [`run`] has it. None in the desktop
/// test binaries that link this crate (never on a device).
pub fn app() -> Option<&'static AndroidApp> {
    APP.get()
}

/// The app's folders, decided once at start.
struct Dirs {
    /// `Context.getFilesDir()`: `/data/user/0/<package>/files`. Private,
    /// backed up, counts against internal storage.
    files: PathBuf,
    /// `Context.getExternalFilesDir(null)`: `/storage/emulated/0/Android/
    /// data/<package>/files`. Still the app's own (no permission needed) but
    /// on the shared volume, visible to file managers, and the right place
    /// for the Studio content download and the person's places.
    external: Option<PathBuf>,
    /// `Context.getCacheDir()`: `/data/user/0/<package>/cache` (the system
    /// may clear it).
    cache: PathBuf,
}

static DIRS: OnceLock<Dirs> = OnceLock::new();

fn dirs() -> &'static Dirs {
    DIRS.get_or_init(|| {
        let files = app()
            .and_then(|a| a.internal_data_path())
            .unwrap_or_else(|| PathBuf::from("/data/local/tmp/rustudio"));
        let external = app().and_then(|a| a.external_data_path()).filter(|p| {
            std::fs::create_dir_all(p).is_ok() && is_writable(p)
        });
        // getCacheDir() is files/../cache for every app; android-activity
        // doesn't hand it over, so it's derived rather than asked for.
        let cache = files.parent().map(|p| p.join("cache")).unwrap_or_else(|| files.join("cache"));
        let _ = std::fs::create_dir_all(&files);
        let _ = std::fs::create_dir_all(&cache);
        Dirs { files, external, cache }
    })
}

fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".rustudio-write-test");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// The folder standing in for the home folder (`getFilesDir()`).
pub fn files_dir() -> PathBuf {
    dirs().files.clone()
}

/// Where big, user-visible data goes (`paths::data()`): the external files
/// folder when there is one, else the internal one.
pub fn data_dir() -> PathBuf {
    let d = dirs();
    d.external.clone().unwrap_or_else(|| d.files.clone())
}

/// Caches (`paths::cache()`): `getCacheDir()`.
pub fn cache_dir() -> PathBuf {
    dirs().cache.clone()
}

/// Settings and state (`paths::config()` / `paths::state()`): under the
/// internal files folder, so they are backed up with the app.
pub fn config_dir() -> PathBuf {
    dirs().files.join("config")
}

pub fn state_dir() -> PathBuf {
    dirs().files.join("state")
}

/// Screenshots (`paths::pictures()`): the app's external Pictures folder
/// (writing to the shared Pictures collection needs MediaStore).
pub fn pictures_dir() -> PathBuf {
    data_dir().join("Pictures")
}

/// Files unpacked from the APK's `assets/` (api-dump.json), where
/// `paths::resource` looks first.
pub fn resource_dir() -> PathBuf {
    dirs().files.join("resources")
}

/// Sets the environment the desktop code and the `dirs` crate read
/// (rustudio-account's config / cache folders, `TMPDIR` for
/// `std::env::temp_dir`), so nothing outside this module needs to know it
/// runs on Android. HOME isn't set in an Android process.
fn export_env() {
    let d = dirs();
    let set = |k: &str, v: &Path| {
        // SAFETY: called once, at the start of android_main, before this
        // process has any Rust thread that reads the environment. (The
        // block is what edition 2024 asks for; 2021 doesn't mind it.)
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(k, v)
        };
    };
    set("HOME", &d.files);
    set("XDG_CONFIG_HOME", &config_dir());
    set("XDG_STATE_HOME", &state_dir());
    set("XDG_CACHE_HOME", &d.cache);
    set("XDG_DATA_HOME", &data_dir());
    set("TMPDIR", &d.cache.join("tmp"));
    let _ = std::fs::create_dir_all(d.cache.join("tmp"));
    // The editor's RUSTUDIO_* switches, for a device that has no shell
    // environment to launch from:
    //   adb shell setprop debug.rustudio.env "RUSTUDIO_SSAO=1 RUSTUDIO_VIEW_REUSE=0"
    // (space-separated KEY=VALUE pairs, 91 characters at most; read at
    // start, so relaunch after; `setprop debug.rustudio.env ""` clears).
    for pair in system_property("debug.rustudio.env").split_whitespace() {
        if let Some((k, v)) = pair.split_once('=') {
            if k.starts_with("RUSTUDIO_") {
                log::info!("env from debug.rustudio.env: {k}={v}");
                // SAFETY: as above, before any other thread.
                #[allow(unused_unsafe)]
                unsafe {
                    std::env::set_var(k, v)
                };
            }
        }
    }
}

/// An Android system property (`adb shell getprop`), or "".
fn system_property(name: &str) -> String {
    extern "C" {
        fn __system_property_get(name: *const std::os::raw::c_char, value: *mut std::os::raw::c_char) -> std::os::raw::c_int;
    }
    let Ok(name) = std::ffi::CString::new(name) else {
        return String::new();
    };
    // PROP_VALUE_MAX, the NUL included.
    let mut buf = [0u8; 92];
    // SAFETY: bionic's getter writes at most PROP_VALUE_MAX bytes into
    // `buf` and returns the value's length.
    let n = unsafe { __system_property_get(name.as_ptr(), buf.as_mut_ptr() as *mut std::os::raw::c_char) };
    if n <= 0 {
        return String::new();
    }
    String::from_utf8_lossy(&buf[..(n as usize).min(buf.len())]).into_owned()
}

/// `android_main`: the editor in this activity, until it is destroyed.
pub fn run(app: AndroidApp) {
    logcat::install();
    log::info!("rustudio {} starting (Android API {})", env!("CARGO_PKG_VERSION"), AndroidApp::sdk_version());
    let _ = APP.set(app.clone());
    export_env();
    log::info!(
        "files {} / data {} / cache {}",
        files_dir().display(),
        data_dir().display(),
        cache_dir().display()
    );
    // A panic or fatal signal leaves a report in the cache folder, listed
    // in Output at the next start (and logcat gets the panic message).
    crate::crash::install();
    if let Err(e) = assets::unpack(&app) {
        log::error!("assets: {e}");
    }
    // os.clock counts from here; the Luau flags are process-global and set
    // before any VM exists.
    let _ = crate::runtime::process_start();
    crate::runtime::luau_flags();
    crate::matpack::init_from_env();
    // Studio's built-in content (the CDN download, ~60 MB compressed) in the
    // background when it isn't there yet, as on a desktop; the editor works
    // without it meanwhile. Pure-Rust TLS (ureq + rustls/ring), no OpenSSL.
    if jni::network_available().unwrap_or(true) {
        crate::studio_content::spawn_background_install_if_missing();
    } else {
        log::warn!("no network: Studio content download postponed");
    }
    // A place or model this activity was started with (VIEW intent,
    // content: URI): copied into the app's folder and opened.
    let open = storage::launch_file(&app);

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default().with_title("rustudio").with_app_id("rustudio"),
        // Vulkan first, GLES 3 where there is no usable Vulkan driver
        // (crate::wgpu_options); vsync.
        wgpu_options: crate::wgpu_options(false),
        // eframe's own window state file would go through `directories`,
        // which has no Android folders.
        persistence_path: Some(files_dir().join("eframe.ron")),
        android_app: Some(app),
        ..Default::default()
    };
    let result = eframe::run_native(
        "rustudio",
        options,
        Box::new(move |cc| {
            lifecycle::note_context(&cc.egui_ctx);
            let mut app = crate::app::RustudioApp::new(cc, open, crate::app::Harness::interactive(), false);
            touch::setup(&mut app, &cc.egui_ctx);
            Ok(Box::new(app))
        }),
    );
    if let Err(e) = result {
        log::error!("eframe: {e}");
    }
    lifecycle::LOOP_DONE.store(true, std::sync::atomic::Ordering::SeqCst);
    // The next launch gets a fresh process: the handles kept in this one's
    // statics (APP, the folders, the destroy hook's context) belong to this
    // activity, and android-activity may run android_main again in the same
    // process otherwise.
    log::info!("rustudio: editor closed, leaving the process");
    std::process::exit(0);
}

/// Touch-first settings for the egui chrome: bigger hit targets, a wider
/// click slop, and the device defaults for the UI zoom and the 3D view's
/// render scale (both kept by the layout store once changed from View >
/// Zoom In / Out and View > Render Scale).
///
/// Points per pixel comes from the display density (winit reports it as
/// the window's scale factor), which a phone's screen turns into too few
/// points across for the editor's ribbon and panels, and a big tablet's
/// into controls smaller than a fingertip. The first run's zoom (of the
/// density) is the one that gives the window about [`touch::FIT_POINTS`]
/// points across, within [`touch::ZOOM_RANGE`]: a 1080p phone lands near
/// 0.8, a 10" tablet near 1.2.
pub mod touch {
    /// The window width (points) the first-run zoom aims for: a small
    /// laptop's worth of editor.
    pub const FIT_POINTS: f32 = 1100.0;
    pub const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.8..=1.6;
    /// The 3D view's first-run render scale: about half the pixels (phone
    /// GPUs at their screens' full resolution fell well under 60 Hz).
    const DEFAULT_RENDER_SCALE: f32 = 0.7;

    pub fn setup(app: &mut crate::app::RustudioApp, ctx: &egui::Context) {
        let saved = crate::layout_store::load();
        // (the window's size isn't known until the first frame: auto_zoom)
        app.ed.auto_zoom = saved.as_ref().and_then(|l| l.zoom).is_none();
        if saved.as_ref().and_then(|l| l.render_scale).is_none() && std::env::var_os("RUSTUDIO_RENDER_SCALE").is_none() {
            app.ed.render_scale = DEFAULT_RENDER_SCALE;
        }
        // Studio's SSAO costs five passes over the view for a subtle
        // darkening in corners a phone's screen barely shows.
        if saved.as_ref().and_then(|l| l.ssao).is_none() && std::env::var_os("RUSTUDIO_SSAO").is_none() {
            app.ed.ssao = false;
        }
        ctx.options_mut(|o| {
            // A finger lands less precisely than a pointer: a press that
            // drifts this far is still a tap.
            o.input_options.max_click_dist = 12.0;
            o.input_options.max_double_click_delay = 0.35;
            o.input_options.line_scroll_speed = 60.0;
        });
        ctx.all_styles_mut(|s| {
            s.spacing.interact_size = egui::vec2(44.0, 30.0);
            s.spacing.button_padding = egui::vec2(8.0, 6.0);
            s.spacing.item_spacing = egui::vec2(8.0, 6.0);
            s.spacing.scroll = egui::style::ScrollStyle::solid();
            s.interaction.interact_radius = 8.0;
            s.interaction.resize_grab_radius_side = 10.0;
            s.interaction.resize_grab_radius_corner = 14.0;
            // Tooltips can't be hovered for; a long press shows one.
            s.interaction.tooltip_delay = 0.6;
        });
    }

    /// The first frame with a window: the zoom that fits [`FIT_POINTS`]
    /// across the longer side of the screen (landscape), once per fresh
    /// layout.
    pub fn auto_zoom(ed: &mut crate::app::Editor, ctx: &egui::Context) {
        if !ed.auto_zoom {
            return;
        }
        let rect = ctx.content_rect();
        if !rect.is_positive() {
            return;
        }
        let native = ctx.native_pixels_per_point().unwrap_or(1.0).max(0.5);
        let longer_px = rect.width().max(rect.height()) * ctx.pixels_per_point();
        let zoom = (longer_px / (native * FIT_POINTS)).clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
        let zoom = (zoom * 20.0).round() / 20.0;
        log::info!("first run: {longer_px:.0} px across at density {native:.2}: zoom {zoom:.2}");
        ed.ui.zoom = zoom;
        ed.auto_zoom = false;
    }
}

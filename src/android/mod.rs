//! The Android app. `NativeActivity` loads `librustudio_android.so`
//! (crates/rustudio-android), whose `android_main` calls [`run`] with the
//! `AndroidApp` handle android-activity made for it; everything after that
//! is the same `RustudioApp` as on a desktop, in eframe's Android backend.
//!
//! What differs from a desktop and lives here:
//!
//! * The folders (crates/rustudio-core/src/android_dirs.rs, told to the core
//!   crate by [`run`]): the app's own stand in for `~/.local/share` and
//!   friends (`Context.getFilesDir()`, `getExternalFilesDir()`,
//!   `getCacheDir()`), and `api-dump.json` is unpacked from the APK's
//!   assets ([`assets`]).
//! * Logging: `log` lines, and anything printed to stdout / stderr, go to
//!   logcat under the tag `rustudio` ([`logcat`]).
//! * The lifecycle: eframe's wgpu backend drops the window and its surface
//!   in `suspended()` and makes them again in `resumed()`, so the GPU
//!   objects survive the activity going to the background; a rotation
//!   arrives as a resize and the swapchain is reconfigured.
//! * The window: the whole screen, the navigation and status bars hidden
//!   until a swipe from the edge shows them for a moment
//!   ([`system_bars`]).
//! * Text: the soft keyboard's room and its showing ([`keyboard`]), the
//!   system clipboard ([`clipboard`]) and, since the keyboard has no Ctrl
//!   and its paste key never reaches a NativeActivity, Cut / Copy / Paste
//!   / Select all on a bar above the keyboard ([`text_bar`]) and in the
//!   text fields' long-press menu ([`text_menu`]).
//! * The Roblox services with a device behind them ([`jni`]: Vibrator,
//!   orientation, window insets, connectivity; [`sensors`]: accelerometer,
//!   gyroscope, rotation, installed as the engine's
//!   `device_sensors::Source`; [`storage`]: files opened through the
//!   system picker / share sheet).
//! * The GPU driver: on Snapdragon devices the Turnip (Mesa) Vulkan
//!   driver shipped in the APK is swapped in for the Adreno driver before
//!   wgpu starts ([`turnip`]).

mod assets;
pub mod clipboard;
pub mod cutout;
pub mod frame_rate;
pub mod ime;
pub mod intent;
pub mod jni;
pub mod keyboard;
pub mod lifecycle;
mod logcat;
pub mod logs;
pub mod memory;
pub mod perf_hint;
pub mod sensors;
pub mod shader_cache;
pub mod storage;
pub mod system_bars;
pub mod text_bar;
pub mod text_menu;
pub mod turnip;

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

/// The app's folders (crates/rustudio-core/src/android_dirs.rs), told to the
/// core crate by [`run`] before any path is read.
pub use rustudio_core::android_dirs::{cache_dir, config_dir, data_dir, files_dir, pictures_dir, resource_dir, state_dir};

/// Hands the core crate the activity's folders (`getFilesDir()`,
/// `getExternalFilesDir(null)`).
fn init_dirs(app: &AndroidApp) {
    let files = app.internal_data_path().unwrap_or_else(|| PathBuf::from("/data/local/tmp/rustudio"));
    rustudio_core::android_dirs::init(files, app.external_data_path());
}

/// Sets the environment the desktop code and the `dirs` crate read
/// (rustudio-account's config / cache folders, `TMPDIR` for
/// `std::env::temp_dir`), so nothing outside this module needs to know it
/// runs on Android. HOME isn't set in an Android process.
fn export_env() {
    let files = files_dir();
    let cache = cache_dir();
    let set = |k: &str, v: &Path| {
        // SAFETY: called once, at the start of android_main, before this
        // process has any Rust thread that reads the environment. (The
        // block is what edition 2024 asks for; 2021 doesn't mind it.)
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(k, v)
        };
    };
    set("HOME", &files);
    set("XDG_CONFIG_HOME", &config_dir());
    set("XDG_STATE_HOME", &state_dir());
    set("XDG_CACHE_HOME", &cache);
    set("XDG_DATA_HOME", &data_dir());
    set("TMPDIR", &cache.join("tmp"));
    let _ = std::fs::create_dir_all(cache.join("tmp"));
    // A phone's physics pool: two threads, not the desktop's four. A phone
    // has four fast cores at most (a Snapdragon 865: one prime, three gold,
    // four slow A55s) and the render, gameplay and UI threads are already
    // on them; a fork-join step with four workers puts one on a slow core
    // and every tick waits for that one. (`physics::par_pool`; a
    // debug.rustudio.env value below still wins.)
    if std::env::var_os("RUSTUDIO_PHYSICS_THREADS").is_none() {
        // SAFETY: as above, before any other thread.
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var("RUSTUDIO_PHYSICS_THREADS", "2")
        };
    }
    // The editor's RUSTUDIO_* switches, for a device that has no shell
    // environment to launch from:
    //   adb shell setprop debug.rustudio.env "RUSTUDIO_NO_NATIVE=1 RUSTUDIO_QUALITY_TRACE=1"
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
pub(crate) fn system_property(name: &str) -> String {
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
    log::info!(
        "rustudio {} starting (Android API {}; {} build, {} {})",
        env!("CARGO_PKG_VERSION"),
        AndroidApp::sdk_version(),
        if cfg!(debug_assertions) { "debug" } else { "release" },
        std::env::consts::ARCH,
        std::env::consts::OS,
    );
    if APP.set(app.clone()).is_err() {
        // (not expected: the entry point keeps any activity after the
        // first away from the glue, intent.rs; returning finishes this one
        // without touching the process)
        log::error!("android_main ran again in this process: this activity is finished, the editor keeps running");
        return;
    }
    init_dirs(&app);
    export_env();
    // The session log file (logcat.rs): the lines logged so far go in first.
    logcat::open_session();
    // "Rebuild shader caches at the next start" (shader_cache.rs): the
    // pipeline cache and Mesa's shader cache go before any driver loads.
    shader_cache::apply_at_start();
    // The motion sensors behind UserInputService (sensors.rs), for the
    // engine's device_sensors.
    rustudio_engine::device_sensors::install(sensors::source());
    // A MicroProfiler export (written under the app's files folder, which
    // the Files apps can't open) is copied to Download/rustudio/<name>, as
    // the logs are, so a capture of a laggy minute can be sent from the phone.
    rustudio_profiler_ui::on_saved(|path| {
        let name = path.file_name()?.to_str()?.to_string();
        let bytes = std::fs::read(path).ok()?;
        match jni::export_download(&name, "application/octet-stream", &bytes) {
            Ok(shown) => {
                log::info!("profiler: capture copied to {shown} ({} bytes)", bytes.len());
                Some(shown)
            }
            Err(e) => {
                log::warn!("profiler: copy to Download failed: {e}");
                None
            }
        }
    });
    // The navigation and status bars out of the way (immersive mode), on
    // the main thread when it gets to it; the window grows to the screen.
    system_bars::hide();
    // And under the camera hole on its side (cutout.rs): the editor pads
    // itself by the hole's insets instead of Android letterboxing the side.
    cutout::request();
    // The soft keyboard's input connection (ime.rs): its Java class out of
    // the library, its view into the activity.
    ime::install();
    log::info!(
        "files {} / data {} / cache {}",
        files_dir().display(),
        data_dir().display(),
        cache_dir().display()
    );
    // A panic or fatal signal leaves a report in the cache folder, listed
    // in Output at the next start (and logcat gets the panic message).
    rustudio_core::crash::install();
    // Studio's installations of the engine's start-up hooks (src/studio_hooks.rs).
    crate::studio_hooks::install();
    // The Turnip (Mesa) Vulkan driver in place of the Adreno driver on
    // Snapdragon devices (turnip.rs): now, before eframe exists, because
    // wgpu loads libvulkan.so once, at its first instance.
    turnip::activate();
    if let Err(e) = assets::unpack(&app) {
        log::error!("assets: {e}");
    }
    // os.clock counts from here; the Luau flags are process-global and set
    // before any VM exists.
    let _ = rustudio_engine::runtime::process_start();
    rustudio_engine::runtime::luau_flags();
    rustudio_render::matpack::init_from_env();
    // Studio's built-in content (the CDN download, ~60 MB compressed) in the
    // background when it isn't there yet, as on a desktop; the editor works
    // without it meanwhile. Pure-Rust TLS (ureq + rustls/ring), no OpenSSL.
    // (`network_available` asks for NET_CAPABILITY_INTERNET only: VALIDATED
    // is missing on networks whose connectivity check is blocked or slow —
    // some vendors' and regions' — while the internet works, and the
    // download must not be given up on those.) Without a network, the
    // download starts when one comes, rather than at the next launch.
    if jni::network_available().unwrap_or(true) {
        rustudio_core::studio_content::spawn_background_install_if_missing();
    } else {
        log::warn!("no network yet: Studio content download waits for one");
        let _ = std::thread::Builder::new().name("studio-content-wait".into()).spawn(|| {
            for _ in 0..180 {
                std::thread::sleep(std::time::Duration::from_secs(10));
                if jni::network_available().unwrap_or(true) {
                    log::info!("network is up: starting the Studio content download");
                    rustudio_core::studio_content::spawn_background_install_if_missing();
                    return;
                }
            }
            log::warn!("no network for half an hour: Studio content download left to the next launch");
        });
    }
    // One request at start, so a report says whether HTTPS works from this
    // app at all (Output and logcat: "network check: …"): a no-network
    // phone, a DNS or certificate problem and a blocked CDN each read
    // differently there.
    network_check();
    // Studio Settings: the user's, kept in the app's folder (as a plain
    // desktop run keeps them; src/studio_settings.rs).
    crate::studio_settings::init(true);
    rustudio_core::rustudio_settings::init(true);
    // A place or model this activity was started with (VIEW intent,
    // content: URI): copied into the app's folder and opened.
    let open = storage::launch_file(&app);

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default().with_title("rustudio").with_app_id("rustudio"),
        // Vulkan first, GLES 3 where there is no usable Vulkan driver
        // (offscreen::wgpu_options_with, with Turnip's objects where
        // crate::wgpu_setup made them); vsync.
        wgpu_options: crate::offscreen::wgpu_options_with(false, crate::wgpu_setup),
        // eframe's own window state file would go through `directories`,
        // which has no Android folders.
        persistence_path: Some(files_dir().join("eframe.ron")),
        android_app: Some(app),
        ..Default::default()
    };
    // A panic on the editor's thread is caught here, not left to
    // android-activity's glue: the glue catches it too, but then finishes
    // the Activity gracefully, so the process ended as `reason=1
    // (EXIT_SELF) status=0` in `dumpsys activity exit-info` — a clean exit
    // to every tool — with the panic only in logcat. Now the report
    // (crash::install's hook wrote it) is named, the panic goes to logcat
    // at error level under the rustudio tag, and the process leaves with
    // status 101 (Rust's for a panic), so the exit-info tells a crash
    // from a close.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        eframe::run_native(
            "rustudio",
            options,
            Box::new(move |cc| {
                lifecycle::note_context(&cc.egui_ctx);
                let mut app = crate::app::RustudioApp::new(cc, open, crate::app::Harness::interactive(), false);
                touch::setup(&mut app, &cc.egui_ctx);
                Ok(Box::new(app))
            }),
        )
    }));
    perf_hint::stop();
    lifecycle::LOOP_DONE.store(true, std::sync::atomic::Ordering::SeqCst);
    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            // (eframe gave up: a surface or device it couldn't make)
            log::error!("eframe: {e}");
            log::error!("rustudio: the editor's loop ended with an error, leaving the process");
            logcat::sync_session();
            std::process::exit(2);
        }
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "<non-string panic payload>".into());
            log::error!("rustudio: panic on the editor's thread: {msg}");
            if let Some(p) = rustudio_core::crash::last_report() {
                log::error!("rustudio: crash report {} (echoed to Output and logcat at the next start)", p.display());
            }
            logcat::sync_session();
            std::process::exit(101);
        }
    }
    // The next launch gets a fresh process: the handles kept in this one's
    // statics (APP, the folders, the destroy hook's context) belong to this
    // activity, and android-activity may run android_main again in the same
    // process otherwise.
    log::info!("rustudio: editor closed, leaving the process");
    std::process::exit(0);
}

/// GPU errors go to logcat instead of ending the process.
///
/// wgpu's answer to a validation error nobody catches is a panic on the
/// thread that made the call (`wgpu_core.rs` "Handling wgpu errors as
/// fatal by default") — in the editor loop that is the exit 101 the
/// crash report names, for a frame that could have gone on without the
/// one copy or draw the error refused. Here the device gets a handler:
/// the error's text goes to logcat (`rustudio` tag; the first
/// [`GPU_ERRORS_LOGGED`] in full, a count after that, so a per-frame
/// error can't fill the log), [`gpu_errors`] counts them for the perf
/// log, and the run goes on with the device's driver as it is. Device
/// loss and out-of-memory are reported the same way; the frames after
/// them show what they show (the renderer's own buffer and texture
/// bounds are checked before the calls, render.rs `MaterialArrays`).
/// Replaces the handlers `rustudio_render::gpu_health::watch` set for the
/// device (an Output warning for the first error; a panic at the next
/// frame once the device is lost): here the run saves and restarts instead
/// (`gpu_watch`).
pub fn install_gpu_error_handler(device: &egui_wgpu::wgpu::Device) {
    device.on_uncaptured_error(std::sync::Arc::new(|e: egui_wgpu::wgpu::Error| {
        let n = GPU_ERRORS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if matches!(e, egui_wgpu::wgpu::Error::OutOfMemory { .. }) {
            GPU_OOM.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if n <= GPU_ERRORS_LOGGED {
            log::error!("rustudio: wgpu error {n} (the frame goes on): {e}");
            rustudio_core::crash::note(format!("wgpu error {n}: {e}"));
        } else if n == GPU_ERRORS_LOGGED + 1 {
            log::error!("rustudio: wgpu error {n}: {e} — further ones are counted, not logged");
            rustudio_core::crash::note(format!("wgpu error {n}: {e} (further ones counted)"));
        }
    }));
    // The driver reset the GPU (a hang, a fault, the system reclaiming it):
    // every call on this device fails from here on. wgpu says so only here.
    device.set_device_lost_callback(|reason, message| {
        let line = format!(
            "rustudio: the GPU device is lost ({reason:?}: {message}); nothing more can be drawn with it — the open place is saved to the recovery folder and rustudio restarts"
        );
        log::error!("{line}");
        rustudio_core::crash::note(line);
        GPU_LOST.store(true, std::sync::atomic::Ordering::Release);
        // (the renderer skips uploads that would panic on the device's
        // destroyed buffers in the rest of this frame: world_gui.rs `stage`)
        rustudio_render::gpu_health::mark_lost(format!("GPU device lost ({reason:?}: {message})"));
    });
}

/// How many GPU errors' texts go to logcat in full.
pub const GPU_ERRORS_LOGGED: usize = 20;
static GPU_ERRORS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static GPU_OOM: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static GPU_LOST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The GPU errors so far this run (`install_gpu_error_handler`).
pub fn gpu_errors() -> usize {
    GPU_ERRORS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Of them, the out-of-memory ones.
pub fn gpu_oom_errors() -> usize {
    GPU_OOM.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether wgpu reported the device lost (`install_gpu_error_handler`).
pub fn gpu_lost() -> bool {
    GPU_LOST.load(std::sync::atomic::Ordering::Acquire)
}

/// Once a frame, before the editor draws: a lost GPU device, or GPU
/// out-of-memory errors every frame for [`OOM_PATIENCE`], end the run in
/// order — the open place is written to `<data>/recovery/`, a report
/// says so for the next start, and the activity is relaunched (jni::relaunch)
/// — instead of a process that draws nothing until the system kills it.
/// (The vendored egui-wgpu skips a frame whose upload the device refuses,
/// renderer.rs `upload_failed`; a single refused frame recovers by
/// itself.) Returns true when the frame should draw nothing.
pub fn gpu_watch(ed: &mut crate::app::Editor) -> bool {
    use std::sync::atomic::Ordering;
    use std::time::Instant;
    const OOM_PATIENCE: std::time::Duration = std::time::Duration::from_secs(5);
    static LAST_OOM: std::sync::Mutex<(usize, Option<Instant>)> = std::sync::Mutex::new((0, None));
    static HANDLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if HANDLED.load(Ordering::Relaxed) {
        return true;
    }
    let why = if gpu_lost() {
        Some("the GPU device was lost (the graphics driver reset it)".to_string())
    } else {
        let oom = gpu_oom_errors();
        let mut g = LAST_OOM.lock().unwrap_or_else(|e| e.into_inner());
        let (seen, since) = *g;
        *g = if oom > seen {
            (oom, Some(since.unwrap_or_else(Instant::now)))
        } else {
            (oom, None)
        };
        match g.1 {
            Some(t) if t.elapsed() >= OOM_PATIENCE => Some(format!(
                "the GPU ran out of memory for {} s ({oom} out-of-memory errors)",
                OOM_PATIENCE.as_secs()
            )),
            _ => None,
        }
    };
    let Some(why) = why else { return false };
    HANDLED.store(true, Ordering::Relaxed);
    // The open place, if one is (the Start Page's empty document isn't).
    // During Play `game.doc` is the session's server copy (characters,
    // run-time clones): the edit document Stop would bring back is what
    // goes to the recovery folder.
    let open = !(ed.start.visible && ed.game.doc.id == ed.start.doc_id);
    let doc = ed.game.play_saved_doc().unwrap_or(&ed.game.doc);
    let saved = if open {
        let dir = rustudio_core::android_dirs::data_dir().join("recovery");
        let _ = std::fs::create_dir_all(&dir);
        let mut name: String = doc
            .title
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .take(60)
            .collect();
        if name.is_empty() {
            name = "place".into();
        }
        let path = dir.join(format!("{name}-{}.rbxl", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        match doc.encode_all(false).and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
            Ok(()) => format!("The open place was saved to {}", path.display()),
            Err(e) => format!("The open place could not be saved ({e})"),
        }
    } else {
        "No place was open".to_string()
    };
    let line = format!("rustudio: {why}; nothing more can be drawn. {saved}. rustudio restarts; open the saved copy from Recent or the recovery folder.");
    log::error!("{line}");
    eprintln!("{line}");
    logcat::sync_session();
    let _ = rustudio_core::crash::write_event(&why, &line);
    if let Err(e) = jni::relaunch() {
        log::error!("rustudio: relaunch: {e}");
    }
    // (the report and the save are on disk; the relaunch intent is queued)
    std::thread::sleep(std::time::Duration::from_millis(300));
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

    /// (The 3D view's first-run quality is Automatic from the lowest
    /// level, in weak-GPU mode, at 75% 3D resolution: gpu_caps.rs
    /// `weak_reason`, rustudio_settings.rs `choice_default`.)
    pub fn setup(app: &mut crate::app::RustudioApp, ctx: &egui::Context) {
        let saved = crate::layout_store::load();
        // (the window's size isn't known until the first frame: auto_zoom)
        app.ed.auto_zoom = saved.as_ref().and_then(|l| l.zoom).is_none();
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

/// Roblox's small client-version page and its asset CDN, reached phase by
/// phase (`rustudio_account::http::probe`) on a thread of its own; the
/// lines go to Output (through the Studio content messages) and to logcat.
/// `adb logcat -s rustudio` after a start shows where a slow network's
/// time goes.
fn network_check() {
    let _ = std::thread::Builder::new().name("network-check".into()).spawn(|| {
        for url in [
            "https://clientsettingscdn.roblox.com/v2/client-version/WindowsStudio64",
            "https://assetdelivery.roblox.com/v1/asset/?id=1",
        ] {
            let (err, line) = match rustudio_account::http::probe(url) {
                Ok(s) => (false, format!("network check: {s}")),
                Err(e) => (true, format!("network check: {e}; nothing from Roblox — sign-in, assets, Studio content — can work until this does")),
            };
            if err { log::warn!("{line}") } else { log::info!("{line}") }
            rustudio_core::studio_content::note(err, line);
        }
    });
}

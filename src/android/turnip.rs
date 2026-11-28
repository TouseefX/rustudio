//! The Turnip (Mesa) Vulkan driver in place of the device's Adreno driver
//! on Snapdragon phones, swapped in before wgpu loads Vulkan.
//!
//! How it works. The APK's lib/arm64-v8a carries `libadrenotools.so`
//! (TouseefX's libadrenotools-native, a fork of bylaws/libadrenotools made
//! for apps with no Java of their own; tools/android/fetch-turnip.sh puts
//! it there with its hook libraries) and `libvulkan_freedreno.so` (Mesa's
//! Turnip, nihui's Android build). libadrenotools' `adrenotools_open_libvulkan`
//! opens a private copy of the system's Vulkan loader (`libvulkan.so`) in a
//! linker namespace of its own, with a hook that makes that copy load the
//! Turnip driver instead of the vendor's `vulkan.adreno.so`, and returns
//! that copy's handle: its `vkGetInstanceProcAddr` leads to Turnip, with
//! the loader's own swapchain code on top (Mesa's Android builds leave
//! window surfaces to the loader, as the vendor driver does). [`activate`]
//! runs in `android_main` before eframe starts, while the launch screen
//! is still up: it loads the shim with its own initialisation switched
//! off (`ADRENOTOOLS_DISABLE=1`, below; its constructor's Mesa environment
//! still goes in), opens a Turnip-backed loader of its own and probes it
//! (which driver answers: the log, Graphics Settings > GPU driver, the
//! perf line's adapter name). Then [`wgpu_setup`] builds wgpu's instance,
//! adapter, device and queue on that loader — ash's `Entry::from_static_fn`
//! on its `vkGetInstanceProcAddr`, the instance exactly as wgpu-hal's own
//! `Instance::init` makes one, `wgpu_hal::vulkan::Instance::from_raw`,
//! `wgpu::Instance::from_hal` — and `wgpu_options` (crates/rustudio-player/src/offscreen.rs, finished by lib.rs `wgpu_setup`) hands them to eframe as
//! `egui_wgpu::WgpuSetup::Existing`. wgpu itself is unchanged.
//!
//! Why not the fork's patch of the system loader. Left to itself the fork
//! initialises at load (`perform_init`, from `JNI_OnLoad` or a thread its
//! constructor starts): it opens a hooked copy of the loader with the
//! driver behind it and inline-hooks the system `libvulkan.so`'s
//! `vkGetInstanceProcAddr` / `vkGetDeviceProcAddr` (shadowhook, shared
//! mode) to answer from that copy, so that a plain `dlopen("libvulkan.so")`
//! — wgpu-hal's `ash::Entry::load()` — lands on Turnip, while callers from
//! the system's own libraries (hwui, WebView, …) are sent on to the
//! vendor's loader. Its 4a084a5 build didn't hold (its proxies leaked
//! shadowhook's per-thread frame, so only the first `vkGetInstanceProcAddr`
//! call per thread reached Turnip and every later one the vendor's loader;
//! the first device run showed exactly this: the probe's instance was
//! Turnip's, wgpu's right after it on the same thread was Adreno's), and
//! 4d4d482 (2026-11-17 12:42, the driver.cpp rewrite that hooks these two
//! functions only, scopes the frames and bypasses system callers) needs a
//! JavaVM, the Application context and a thread of its own to settle. A
//! loader handle of our own and the pre-built wgpu objects need none of
//! that, so the patch is switched off where the shim allows
//! (`ADRENOTOOLS_DISABLE`, which 4d4d482 reads and 4a084a5 doesn't;
//! [`Shim`] reads off the file which one is in the APK) and, with an
//! older shim, run but not relied on (its hooks' leaked frame is settled
//! for the ordinary start, `settle_system_loader`).
//!
//! PanVK for Mali. The same shim opens Mesa's PanVK driver
//! (`libvulkan_panfrost.so`: JimVulkan's mali-panvk build, Mesa 27.0-devel
//! on the stock Arm kbase kernel driver, the one Android's Mali devices
//! have — no root, no custom kernel; the fork's own `init_panvk_driver`
//! opens the same file name) on
//! devices whose GPU is a Mali (`ro.hardware.vulkan` / `ro.hardware.egl`
//! "mali", or `/dev/mali0`) with the kbase node `/dev/mali0` and a
//! Bifrost or newer core (`Mali-G…` per the kbase sysfs `gpuinfo`; the
//! Midgard `Mali-T…` and Utgard `Mali-4xx` are out: PanVK has no code for
//! them), the file in the APK (tools/android/fetch-turnip.sh `PANVK=1`,
//! the default; 18 MB) and Android 12+. Everything else below —
//! `adrenotools_open_libvulkan`, the probe, [`wgpu_setup`], the markers,
//! the window's switch — is the same path with the other file name
//! ([`Mesa`]). The shim's constructor tells a Mali the same way
//! (`/dev/kgsl-3d0` first, then `/dev/mali0`, the properties, the kbase
//! sysfs and the device tree) and sets PanVK's environment
//! (`PAN_I_WANT_A_BROKEN_VULKAN_DRIVER=1`, which the driver wants before
//! it enumerates a device, `PAN_MESA_DEBUG=kbase`, …; this module sets the
//! first of its own too). With the older 4a084a5 shim, whose own
//! initialisation can't be switched off, a device whose
//! `/proc/device-tree/model` says "Mali" is kept on its driver: that
//! build's PanVK path installs hooks that call a null Turnip pointer
//! ([`Device::shim_would_misfire`]). No Mali device was at hand for this:
//! the probe and the first-frames marker decide per device, as for Turnip.
//!
//! Only Adreno 650 and up, on Snapdragon (HyperOS with a new enough
//! libadrenotools, below): Turnip drives
//! Adreno GPUs through Qualcomm's KGSL kernel driver and nothing else
//! (a Mali, PowerVR or Xclipse named by `ro.hardware.vulkan` /
//! `ro.hardware.egl` is out at once), the device must have
//! `/dev/kgsl-3d0` (or name Qualcomm as its SoC maker in the system
//! properties), its Adreno must be a 650 or newer per
//! `/sys/class/kgsl/kgsl-3d0/gpu_model` (or `ro.soc.model` for the SoCs
//! listed in [`Device::adreno_model`]; an unknown model is out), and
//! Xiaomi's HyperOS (`ro.mi.os.version.name`) is out unless the APK's
//! libadrenotools is a build that looks for the system's Vulkan loader
//! under the vendor paths HyperOS keeps it at (libadrenotools-native from
//! 2026-11-17; [`Shim`] reads that off the file — the older builds found
//! nothing to hook there). Every other
//! device keeps its driver and the window doesn't
//! offer the switch. The swap is also skipped on other ABIs than
//! arm64-v8a, below Android 12 (the fork's floor), when the files aren't
//! in the APK, unless turned on (View > GPU Driver: the swap is opt-in,
//! off by default — [`wanted`]; `RUSTUDIO_TURNIP=0` keeps it off), and
//! after a start that didn't get to its first frames with Turnip (a
//! marker file this module keeps; the window offers "Try again").
//! `RUSTUDIO_TURNIP=1` overrides the window's choice
//! and the marker; `RUSTUDIO_TURNIP=force` also skips the device checks
//! (the GPU, model, HyperOS and Android version ones). `RUSTUDIO_GPU_TURBO=1` asks libadrenotools for the GPU's top
//! clocks (`adrenotools_set_turbo`; thermal limits still apply; an ioctl
//! some Adreno kernels crash on, so not for a device that has rebooted);
//! Turnip's own switches (`TU_DEBUG`): Graphics Settings > GPU driver
//! lists the useful ones ([`SWITCHES`]: LRZ, binning, tiled rendering,
//! …) and keeps them in `files/turnip/debug`, which the driver watches
//! (Mesa's `TU_DEBUG_FILE`: the ones Mesa can change while running change
//! at once, the others at the next start; `nolrz` is on by default, and
//! on an Adreno 6xx `noubwc` — with UBWC on, Turnip's first frame
//! restarted a Snapdragon 865 phone outright — see [`default_switches`]);
//! `RUSTUDIO_TU_DEBUG=<flags>` replaces the start-time set the fork's
//! constructor would leave ([`TU_DEBUG_BASE`]).

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use ash::vk;

/// Which Mesa driver the device gets: Turnip on Adreno (Snapdragon),
/// PanVK on Mali (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mesa {
    Turnip,
    PanVk,
}

impl Mesa {
    pub fn name(self) -> &'static str {
        match self {
            Mesa::Turnip => "Turnip",
            Mesa::PanVk => "PanVK",
        }
    }

    /// The driver's file in the APK's lib/arm64-v8a, the name the shim
    /// opens (and the fork's own init_turnip_driver / init_panvk_driver do).
    fn file(self) -> &'static str {
        match self {
            Mesa::Turnip => "libvulkan_freedreno.so",
            Mesa::PanVk => "libvulkan_panfrost.so",
        }
    }

    /// The GPUs it drives, for the window and the log.
    pub fn gpus(self) -> &'static str {
        match self {
            Mesa::Turnip => "Adreno",
            Mesa::PanVk => "Mali",
        }
    }
}

/// What [`activate`] decided, for the log and Graphics Settings.
#[derive(Clone, Debug, Default)]
pub struct Status {
    /// The Mesa driver (Turnip or PanVK) answers this process's Vulkan calls.
    pub active: bool,
    /// The device qualifies (arm64, Adreno 650+ or a kbase Mali, not an
    /// old HyperOS, Android 12+, files in the APK): the window offers the
    /// switch only then.
    pub available: bool,
    /// The driver this device would get (`None`: no Mesa driver for it,
    /// or no decision yet).
    pub mesa: Option<Mesa>,
    /// What happened, or why nothing did (one line).
    pub note: String,
    /// The Vulkan device as the probe saw it after the swap ("Turnip
    /// Adreno (TM) 650", "Adreno (TM) 650"); "" when no probe ran.
    pub device: String,
    /// Its driver ("Mesa 26.3.0", or the vendor's version).
    pub driver: String,
}

static STATUS: OnceLock<Status> = OnceLock::new();

/// The outcome of this start's [`activate`] (a default before it ran).
pub fn status() -> Status {
    STATUS.get().cloned().unwrap_or_default()
}

/// The Vulkan loader with the Mesa driver behind it that [`activate`] opened for this process
/// (`adrenotools_open_libvulkan`): its `vkGetInstanceProcAddr`, from
/// which [`wgpu_setup`] makes wgpu's instance. The loader stays loaded
/// for the life of the process.
static LOADER: OnceLock<vk::PFN_vkGetInstanceProcAddr> = OnceLock::new();

/// ash's entry points on that loader, when there is one.
pub fn entry() -> Option<ash::Entry> {
    // SAFETY: a vkGetInstanceProcAddr dlsym'd from a loaded copy of
    // libvulkan.so that is never closed.
    LOADER.get().map(|&gipa| unsafe { ash::Entry::from_static_fn(ash::StaticFn { get_instance_proc_addr: gipa }) })
}

/// Which driver the renderer's wgpu device was made on, once
/// `wgpu_options` ran ("Turnip Adreno (TM) 650 — …", or the device's
/// driver with the reason); "" before.
static RENDERER: OnceLock<String> = OnceLock::new();

pub fn renderer() -> String {
    RENDERER.get().cloned().unwrap_or_default()
}

/// libadrenotools' own initialisation ran (an older shim, without the
/// `ADRENOTOOLS_DISABLE` switch), so its patch of the system loader is (or
/// is about to be) in place.
static SHIM_HOOKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// For the ordinary wgpu start after the fork's patch went in but Turnip
/// isn't used: one pass of ash's loading through the system loader on
/// this thread, so that its first `vkGetInstanceProcAddr` call — the one
/// the fork's leaked shadowhook frame lets through to Turnip's loader
/// (module docs) — is spent here, on a pointer nobody calls, and
/// wgpu-hal's own calls, all later ones on the same thread, reach the
/// vendor's loader uniformly, as without the patch.
fn settle_system_loader() {
    // SAFETY: ash's ordinary dlopen / dlsym of libvulkan.so; the Entry is
    // dropped at once and nothing of it is called.
    match unsafe { ash::Entry::load() } {
        Ok(entry) => drop(entry),
        Err(e) => log::warn!("turnip: settling the system loader: {e}"),
    }
}

/// The marker files, under the app's files folder.
fn dir() -> PathBuf {
    super::files_dir().join("turnip")
}

/// Graphics Settings' choice: off unless the `on` marker is there. The
/// Mesa driver is opt-in: the swap was on by default for every eligible
/// phone until some of them crashed at start or in their first frames
/// with it (the failed-start marker below catches a crash before the
/// first frames, not one in the first minutes of a session), and a
/// phone's own driver is the one its games run on. View > GPU Driver
/// turns it on for the phones it helps. (An `off` marker from the time
/// it was opt-out still means off, so a choice made then holds.)
pub fn wanted() -> bool {
    let d = dir();
    d.join("on").exists() && !d.join("off").exists()
}

/// Graphics Settings' switch (takes effect at the next start). Turning it
/// on also forgets a failed start.
pub fn set_wanted(on: bool) {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    if on {
        let _ = std::fs::write(d.join("on"), b"");
        let _ = std::fs::remove_file(d.join("off"));
        let _ = std::fs::remove_file(d.join("failed"));
        // (a start under the old default that never reached its first
        // frames left `pending`; the next start would call it a failure
        // and skip the driver just asked for)
        let _ = std::fs::remove_file(d.join("pending"));
    } else {
        let _ = std::fs::remove_file(d.join("on"));
        let _ = std::fs::write(d.join("off"), b"");
    }
}

/// A start with Turnip didn't reach its first frames, so it is off until
/// [`retry`] / [`set_wanted`].
pub fn failed_last_time() -> bool {
    dir().join("failed").exists()
}

pub fn retry() {
    let _ = std::fs::remove_file(dir().join("failed"));
    let _ = std::fs::remove_file(dir().join("pending"));
}

/// The editor drew its first frames: the start with Turnip went well.
/// Called from the frame loop (app.rs); cheap after the first time.
pub fn mark_healthy() {
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| {
        if STATUS.get().map_or(false, |s| s.active) {
            let d = dir();
            let _ = std::fs::remove_file(d.join("pending"));
            // A good start after a failed one (RUSTUDIO_TURNIP=1) clears it.
            let _ = std::fs::remove_file(d.join("failed"));
            log::info!("turnip: first frames drawn, the driver swap is marked good");
        }
    });
}

// ---------------------------------------------------------------- switches

/// The `TU_DEBUG` libadrenotools' constructor sets (one list for Adreno
/// 5xx to 8xx) less `dynamic`, which the switches file owns below; the
/// names Mesa 26.3 doesn't know (`noconform`, `noflushall`, `deck_emu`)
/// do nothing there and stay for builds of Turnip that might know them
/// (`TURNIP_ZIP_URL`). `RUSTUDIO_TU_DEBUG` replaces the whole list.
pub const TU_DEBUG_BASE: &str = "noconform,hiprio,noflushall,deck_emu";

/// One of Turnip's `TU_DEBUG` options the window offers (Graphics
/// Settings > GPU driver). `live`: Mesa applies a change while running
/// (its `TU_DEBUG_FILE` watch); the others take effect at the next start.
pub struct Switch {
    pub flag: &'static str,
    pub name: &'static str,
    pub hint: &'static str,
    pub live: bool,
}

/// The switches, the likeliest fixes for missing or flickering geometry
/// first (Mesa docs/drivers/freedreno.rst; the runtime set is
/// `tu_runtime_debug_flags` in src/freedreno/vulkan/tu_util.cc). LRZ is
/// off by default: with it, Mesa 26.3.0 on an Adreno 650 dropped whole
/// terrain chunks (one draw each) from the viewport pass — draws of the
/// plainest kind for LRZ (depth test GREATER, depth write, no discard,
/// no blending, after opaque parts drawn the same way), which Turnip's
/// own rules (tu_lrz.cc `tu6_calculate_lrz_state`) accept; `nolrz`
/// brought them back on the spot, at no measurable cost on the baseplate
/// (120 fps either way).
pub const SWITCHES: &[Switch] = &[
    Switch { flag: "nolrz", name: "LRZ off", hint: "No low-resolution depth buffer (Turnip's early rejection of hidden pixels). Off by default: Mesa 26.3.0 on an Adreno 650 lost whole terrain chunks with it. Untick to try LRZ with a newer Turnip build; costs some speed in dense scenes when off.", live: true },
    Switch { flag: "nolrzfc", name: "LRZ fast clear off", hint: "Keeps LRZ but clears it the slow way (a known trouble spot on some Adreno 6xx).", live: true },
    Switch { flag: "nobin", name: "Binning off", hint: "No visibility pre-pass over the screen's tiles: every draw is rasterised in every tile it might touch.", live: true },
    Switch { flag: "sysmem", name: "Tiled rendering off", hint: "Render passes draw straight to memory instead of tile by tile: slower, fewer moving parts (no tile loads, stores, resolves).", live: true },
    Switch { flag: "flushall", name: "Flush caches after every draw", hint: "Slow. Tells a cache or synchronisation bug (fixed by this) from the rest (not).", live: true },
    Switch { flag: "dynamic", name: "Passes through dynamic rendering", hint: "libadrenotools' default: Turnip runs the renderer's (classic Vulkan) render passes through Mesa's dynamic-rendering path. Off: Turnip's own render pass code.", live: false },
    Switch { flag: "noubwc", name: "Bandwidth compression off", hint: "No UBWC on textures and render targets. On by default on an Adreno 6xx: with UBWC, Turnip's first frame restarted a Galaxy S20 FE (Snapdragon 865, Adreno 650) outright — a fatal bus error in the chip (TZBSP_ERR_FATAL_NOC_ERROR), the finding of WindWakerHD-Android-Plus v1.7 with Mesa 26.0 to 26.3, where turning UBWC off cost nothing measurable. Untick only on a 6xx that has proved it can take it. Adreno 7xx and up keep compression.", live: false },
];

/// The switches on by default everywhere: the fork's `dynamic` (the
/// first device runs were with it) and `nolrz` (the terrain chunks,
/// above). [`default_switches`] adds the GPU's own.
pub const DEFAULT_SWITCHES: &[&str] = &["dynamic", "nolrz"];

/// The switches on by default on this device: [`DEFAULT_SWITCHES`] and,
/// on an Adreno 6xx (650, 660, …), `noubwc`. With UBWC (the GPU's
/// bandwidth compression of textures and render targets) on, Turnip's
/// first frame took a Galaxy S20 FE (Snapdragon 865, Adreno 650) down to
/// a restart of the whole phone — a fatal bus error in the chip,
/// `TZBSP_ERR_FATAL_NOC_ERROR` — which WindWakerHD-Android-Plus v1.7
/// found with Mesa 26.0 to 26.3 and fixed with `noubwc` on Adreno 6xx
/// (at no measurable cost there: 18.2 against 19.5 ms of GPU time per
/// frame). A Snapdragon 865 device here restarted the same way with the
/// fork's reboot fixes of 2026-11-18 already in, so those were not the
/// whole story. libadrenotools-native 8c7d9f5's constructor defaults
/// `TU_DEBUG=noconform,noubwc,nolrz` on a 6xx as well, but only when
/// nothing set `TU_DEBUG` before it runs — this module does
/// ([`TU_DEBUG_BASE`]), so the
/// switch goes through the switches file instead, which Mesa reads at
/// start-up (`tu_env_init_once`: the file's start-time flags join
/// `start_debug`). Adreno 7xx and up keep compression: no such report,
/// and it is worth bandwidth there.
pub fn default_switches() -> Vec<&'static str> {
    let mut v = DEFAULT_SWITCHES.to_vec();
    if adreno_6xx() {
        v.push("noubwc");
    }
    v
}

static ADRENO_MODEL: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

/// The Adreno model (650, 740, …; 0 when unknown or not an Adreno), as
/// [`Device::adreno_model`] reads it, once.
pub fn adreno_model() -> u32 {
    *ADRENO_MODEL.get_or_init(|| Device::read().adreno_model())
}

fn adreno_6xx() -> bool {
    (600..700).contains(&adreno_model())
}

/// Mesa's `TU_DEBUG_FILE`: the options on, comma-separated, in the app's
/// files folder. Readable and writable from a shell too
/// (`run-as <package> cat files/turnip/debug`).
pub fn debug_file() -> PathBuf {
    dir().join("debug")
}

static FLAGS: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

fn parse_flags(s: &str) -> Vec<String> {
    s.split(|c: char| c == ',' || c.is_whitespace()).filter(|f| !f.is_empty()).map(str::to_string).collect()
}

/// The options in the file (read once; this module's writes keep it).
pub fn debug_flags() -> Vec<String> {
    let mut g = FLAGS.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(|| std::fs::read_to_string(debug_file()).map(|s| parse_flags(&s)).unwrap_or_default()).clone()
}

/// The file holds exactly the defaults ([`default_switches`]).
pub fn debug_flags_default() -> bool {
    let mut a = debug_flags();
    a.sort();
    let mut d: Vec<String> = default_switches().iter().map(|s| s.to_string()).collect();
    d.sort();
    a == d
}

/// One option on or off (the window's checkbox), written at once: a live
/// one is in effect a moment later.
pub fn set_debug_flag(flag: &str, on: bool) {
    let mut flags = debug_flags();
    flags.retain(|f| f != flag);
    if on {
        flags.push(flag.to_string());
    }
    write_debug_flags(flags);
}

pub fn reset_debug_flags() {
    write_debug_flags(default_switches().iter().map(|s| s.to_string()).collect());
}

/// A switch that joined the defaults after the file was first written
/// goes in once: a marker beside the file (`files/turnip/default-<flag>`)
/// remembers, so unticking it afterwards holds. Nothing happens where it
/// isn't a default ([`default_switches`]).
fn add_default_once(flag: &str) {
    if !default_switches().iter().any(|f| *f == flag) {
        return;
    }
    let marker = dir().join(format!("default-{flag}"));
    if marker.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(dir());
    let _ = std::fs::write(&marker, b"");
    if !debug_flags().iter().any(|f| f == flag) {
        log::info!("turnip: {flag} joins the switches on by default on this GPU (Adreno {}); Graphics Settings > GPU driver unticks it", adreno_model());
        set_debug_flag(flag, true);
    }
}

fn write_debug_flags(flags: Vec<String>) {
    let _ = std::fs::create_dir_all(dir());
    // One open, write and close: Mesa's watch fires on the close.
    if let Err(e) = std::fs::write(debug_file(), format!("{}\n", flags.join(","))) {
        log::warn!("turnip: writing {}: {e}", debug_file().display());
    }
    log::info!("turnip: switches (TU_DEBUG_FILE): {}", if flags.is_empty() { "none".to_string() } else { flags.join(",") });
    *FLAGS.lock().unwrap_or_else(|e| e.into_inner()) = Some(flags);
}

/// What the device is, as far as the swap cares.
struct Device {
    /// Qualcomm's GPU kernel driver node, what Turnip's KGSL backend opens.
    kgsl: bool,
    /// ro.soc.manufacturer (API 31+: "QTI" / "Qualcomm"), ro.soc.model
    /// ("SM8250"), ro.hardware ("qcom"), ro.board.platform ("kona",
    /// "lahaina", …), for the log and as the second opinion.
    soc: String,
    soc_model: String,
    hardware: String,
    platform: String,
    /// ro.hardware.egl / ro.hardware.vulkan: the GPU driver's family name
    /// ("adreno", "mali", "powervr", "xclipse", …), "" when unset.
    egl: String,
    vulkan: String,
    /// /sys/class/kgsl/kgsl-3d0/gpu_model ("Adreno650v3"), "" when unreadable.
    gpu: String,
    /// ARM's kbase kernel driver's device node (Mali): what PanVK's
    /// kbase build drives.
    mali0: bool,
    /// kbase's sysfs `gpuinfo` ("Mali-G52 MP2 r1p0 0x7402"), "" when none
    /// is readable.
    mali_gpu: String,
    /// The device tree's model string (/proc/device-tree/model), for the
    /// shim's own Mali test ([`Device::shim_would_misfire`]).
    dt_model: String,
    /// Xiaomi's HyperOS: ro.mi.os.version.name ("OS1.0", "OS2.0.1"), ""
    /// elsewhere; ro.miui.ui.version.name (MIUI, which HyperOS also sets)
    /// for the log.
    hyperos: String,
    miui: String,
}

/// The Adreno models Turnip is let drive here: 650 and up (the 6xx below
/// it — 610 to 644 — and every older generation keep the vendor's
/// driver). Snapdragon 865's Adreno 650 is the measured one.
const ADRENO_MIN: u32 = 650;

impl Device {
    fn read() -> Self {
        let prop = super::system_property;
        Device {
            kgsl: Path::new("/dev/kgsl-3d0").exists(),
            soc: prop("ro.soc.manufacturer"),
            soc_model: prop("ro.soc.model"),
            hardware: prop("ro.hardware"),
            platform: prop("ro.board.platform"),
            egl: prop("ro.hardware.egl"),
            vulkan: prop("ro.hardware.vulkan"),
            gpu: std::fs::read_to_string("/sys/class/kgsl/kgsl-3d0/gpu_model")
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
            mali0: Path::new("/dev/mali0").exists(),
            mali_gpu: mali_gpuinfo(),
            dt_model: ["/proc/device-tree/model", "/sys/firmware/devicetree/base/model"]
                .iter()
                .find_map(|p| std::fs::read(p).ok())
                .map(|b| String::from_utf8_lossy(&b).trim_matches(char::from(0)).trim().to_string())
                .unwrap_or_default(),
            hyperos: {
                let name = prop("ro.mi.os.version.name");
                if name.is_empty() { prop("ro.mi.os.version.code") } else { name }
            },
            miui: prop("ro.miui.ui.version.name"),
        }
    }

    fn qualcomm(&self) -> bool {
        let soc = self.soc.to_ascii_lowercase();
        self.kgsl || soc.contains("qualcomm") || soc == "qti" || self.hardware.to_ascii_lowercase().starts_with("qcom")
    }

    /// The GPU family another maker's, as the driver properties name it
    /// (Mali, PowerVR, Samsung's Xclipse, …): Turnip is Adreno's driver
    /// and nothing else's. Some("mali") etc.; None when they name Adreno
    /// or nothing.
    fn other_gpu(&self) -> Option<String> {
        for v in [&self.vulkan, &self.egl] {
            let v = v.trim().to_ascii_lowercase();
            if v.is_empty() || v.contains("adreno") {
                continue;
            }
            return Some(v);
        }
        None
    }

    /// A Mali GPU: named by the driver properties, or kbase's node present.
    fn mali(&self) -> bool {
        self.mali0 || self.other_gpu().is_some_and(|g| g.contains("mali"))
    }

    /// The Mali core per kbase's `gpuinfo`: Some(true) for a Bifrost or
    /// newer one ("Mali-G52", "Mali-G610", "Mali-G1-Ultra"), Some(false)
    /// for a Midgard ("Mali-T830") or Utgard ("Mali-450") one, None when
    /// nothing is readable.
    fn mali_bifrost_or_newer(&self) -> Option<bool> {
        let g = self.mali_gpu.trim();
        if g.is_empty() {
            return None;
        }
        let l = g.to_ascii_lowercase();
        if l.starts_with("mali-g") || l.starts_with("mali g") {
            return Some(true);
        }
        if l.starts_with("mali-t") || l.starts_with("mali t") || l.starts_with("mali-4") || l.starts_with("mali 4") {
            return Some(false);
        }
        None
    }

    /// libadrenotools-native 4a084a5's own Mali test (`isMaliGpu`) reads
    /// the KGSL model, then the device tree's model: a device whose tree
    /// says "Mali" sends that build's `perform_init` down its PanVK path,
    /// whose hooks of the system loader call through its Turnip pointer,
    /// null then — a crash at the first `vkGetInstanceProcAddr` through
    /// the system loader. Such a device is kept on its driver when the
    /// APK's shim is one that initialises itself whatever the caller does
    /// ([`Shim::opt_out`] false); 4d4d482 and newer are switched off
    /// instead and have no such path anyway (PanVK shares Turnip's init).
    fn shim_would_misfire(&self) -> bool {
        self.gpu.is_empty() && self.dt_model.to_ascii_lowercase().contains("mali")
    }

    /// The Adreno model (650 for "Adreno650v3" or "Adreno (TM) 650"),
    /// from the KGSL node; when that isn't readable, from the SoC model
    /// for the SoCs known here; 0 when unknown.
    fn adreno_model(&self) -> u32 {
        let digits: String = self.gpu.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<u32>() {
            if (500..1000).contains(&n) {
                return n;
            }
        }
        // ro.soc.model (API 31+) for the Snapdragons with an Adreno 650 or
        // newer: 865/870 (650), 888 (660), 8 Gen 1 / 8+ Gen 1 (730),
        // 8 Gen 2 (740), 8s Gen 3 (735), 8 Gen 3 (750), 8 Elite (830),
        // 7+ Gen 2 (725), 7 Gen 3 (720), 7s Gen 2 / 6 Gen 1 / 7s Gen 3
        // (710), 7 Gen 4 (722).
        const SOCS: &[(&str, u32)] = &[
            ("SM8250", 650), ("SM8350", 660), ("SM8450", 730), ("SM8475", 730), ("SM8550", 740),
            ("SM8635", 735), ("SM8650", 750), ("SM8750", 830),
            ("SM7475", 725), ("SM7550", 720), ("SM7435", 710), ("SM6450", 710), ("SM7635", 710), ("SM7675", 722),
        ];
        let model = self.soc_model.trim().to_ascii_uppercase();
        SOCS.iter().find(|(m, _)| *m == model).map_or(0, |(_, a)| *a)
    }

    fn describe(&self) -> String {
        format!(
            "kgsl {}, soc {:?} {:?}, hardware {:?}, platform {:?}, gpu {:?}, egl {:?}, vulkan {:?}, mali0 {}{}{}{}{}",
            if self.kgsl { "present" } else { "absent" },
            self.soc,
            self.soc_model,
            self.hardware,
            self.platform,
            self.gpu,
            self.egl,
            self.vulkan,
            if self.mali0 { "present" } else { "absent" },
            if self.mali_gpu.is_empty() { String::new() } else { format!(", mali gpuinfo {:?}", self.mali_gpu) },
            if self.dt_model.is_empty() { String::new() } else { format!(", device tree {:?}", self.dt_model) },
            if self.hyperos.is_empty() { String::new() } else { format!(", HyperOS {}", self.hyperos) },
            if self.miui.is_empty() { String::new() } else { format!(", MIUI {}", self.miui) },
        )
    }
}

/// kbase's `gpuinfo` under sysfs: `/sys/devices/platform/<addr>.mali/gpuinfo`
/// on most devices (the node's name varies: "mali", "mali0", "<addr>.mali",
/// "<addr>.gpu"), `/sys/bus/platform/drivers/mali/<addr>.mali/gpuinfo` and
/// `/sys/module/mali_kbase/...` on some. The first one readable, "" when
/// none is.
fn mali_gpuinfo() -> String {
    let mut found = String::new();
    for dir in ["/sys/devices/platform", "/sys/bus/platform/drivers/mali", "/sys/bus/platform/devices"] {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            if !(name.contains("mali") || name.ends_with(".gpu") || name == "gpu") {
                continue;
            }
            if let Ok(s) = std::fs::read_to_string(e.path().join("gpuinfo")) {
                let s = s.trim();
                if !s.is_empty() {
                    found = s.to_string();
                    break;
                }
            }
        }
        if !found.is_empty() {
            break;
        }
    }
    found
}

/// The folder the installer unpacked the APK's native libraries into
/// (`ApplicationInfo.nativeLibraryDir`): where this very library was
/// loaded from. A path with "!/" in it means the libraries were not
/// extracted (extractNativeLibs=false), which libadrenotools can't work
/// with.
fn native_lib_dir() -> Result<PathBuf, String> {
    // SAFETY: dladdr fills `info` for an address inside a loaded object;
    // the address of a function of this crate is one.
    let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::dladdr(native_lib_dir as *const () as *const c_void, &mut info) };
    if ok == 0 || info.dli_fname.is_null() {
        return Err("dladdr: can't tell where librustudio_android.so was loaded from".into());
    }
    // SAFETY: dli_fname is a NUL-terminated string the loader owns.
    let path = unsafe { CStr::from_ptr(info.dli_fname) }.to_string_lossy().into_owned();
    if path.contains("!/") {
        return Err(format!("the APK's native libraries aren't extracted ({path}); libadrenotools needs them as files"));
    }
    Path::new(&path)
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("no folder in {path}"))
}

fn dlerror() -> String {
    // SAFETY: dlerror returns NULL or a string valid until the next dl call
    // on this thread; it is copied at once.
    let p = unsafe { libc::dlerror() };
    if p.is_null() {
        "unknown error".into()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

/// `dlsym` on `handle`, as a function pointer of type `F`.
///
/// # Safety
/// `F` must be the symbol's actual signature.
unsafe fn symbol<F: Copy>(handle: *mut c_void, name: &CStr) -> Result<F, String> {
    assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<*mut c_void>());
    let p = libc::dlsym(handle, name.as_ptr());
    if p.is_null() {
        return Err(format!("{}: {}", name.to_string_lossy(), dlerror()));
    }
    Ok(std::mem::transmute_copy::<*mut c_void, F>(&p))
}

/// A copy of the system's Vulkan loader with the Turnip driver behind it,
/// through libadrenotools' own entry point (what the fork's initialisation
/// does for its hooks, once more for a handle of our own): its
/// `vkGetInstanceProcAddr`. `lib_dir` is the APK's native library folder,
/// where the hook libraries and `libvulkan_freedreno.so` are.
///
/// # Safety
/// `handle` must be the loaded `libadrenotools.so`.
unsafe fn open_turnip_loader(handle: *mut c_void, lib_dir: &Path, mesa: Mesa) -> Result<vk::PFN_vkGetInstanceProcAddr, String> {
    // include/adrenotools/driver.h: dlopenMode, featureFlags, tmpLibDir
    // (unused from API 29 on), hookLibDir, customDriverDir, customDriverName,
    // fileRedirectDir, userMappingHandle.
    type Open = unsafe extern "C" fn(c_int, c_int, *const c_char, *const c_char, *const c_char, *const c_char, *const c_char, *mut *mut c_void) -> *mut c_void;
    const ADRENOTOOLS_DRIVER_CUSTOM: c_int = 1 << 0;
    let open: Open = symbol(handle, c"adrenotools_open_libvulkan")?;
    let tmp = super::cache_dir().join("turnip_tmp");
    let _ = std::fs::create_dir_all(&tmp);
    let c = |s: String| CString::new(s).map_err(|e| e.to_string());
    let tmp = c(tmp.to_string_lossy().into_owned())?;
    let hook_dir = c(lib_dir.to_string_lossy().into_owned())?;
    let driver_dir = c(format!("{}/", lib_dir.display()))?;
    let driver_name = c(mesa.file().to_string())?;
    let loader = open(
        libc::RTLD_NOW | libc::RTLD_LOCAL,
        ADRENOTOOLS_DRIVER_CUSTOM,
        tmp.as_ptr(),
        hook_dir.as_ptr(),
        driver_dir.as_ptr(),
        driver_name.as_ptr(),
        std::ptr::null(),
        std::ptr::null_mut(),
    );
    if loader.is_null() {
        return Err("adrenotools_open_libvulkan returned no loader (its lines under logcat tag AdrenoToolsPatch say why)".into());
    }
    symbol::<vk::PFN_vkGetInstanceProcAddr>(loader, c"vkGetInstanceProcAddr")
}

/// What the loader answered after the swap.
struct Probe {
    device: String,
    driver: String,
    api: u32,
    /// The Mesa driver asked for (Turnip or PanVK) is the one answering.
    mesa: bool,
}

/// A Vulkan instance on `entry` (the Mesa driver's loader, the one wgpu's
/// instance is made on): whose driver is behind it. The instance is
/// destroyed again; the driver it loaded stays in the process.
fn probe(entry: &ash::Entry, mesa: Mesa) -> Result<Probe, String> {
    // SAFETY: each call below is made with valid arguments and the
    // instance is destroyed before the function returns.
    unsafe {
        let loader_version = entry
            .try_enumerate_instance_version()
            .map_err(|e| format!("vkEnumerateInstanceVersion: {e}"))?
            .unwrap_or(vk::API_VERSION_1_0);
        let app_name = c"rustudio";
        let app_info = vk::ApplicationInfo::default()
            .application_name(app_name)
            .application_version(1)
            .api_version(if loader_version >= vk::API_VERSION_1_1 { vk::API_VERSION_1_1 } else { vk::API_VERSION_1_0 });
        let info = vk::InstanceCreateInfo::default().application_info(&app_info);
        let instance = entry.create_instance(&info, None).map_err(|e| format!("vkCreateInstance: {e}"))?;
        let result = (|| {
            let devices = instance.enumerate_physical_devices().map_err(|e| format!("vkEnumeratePhysicalDevices: {e}"))?;
            let Some(&pd) = devices.first() else {
                return Err("no Vulkan physical device".to_string());
            };
            let props = instance.get_physical_device_properties(pd);
            let device = props.device_name_as_c_str().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let mut driver_id = vk::DriverId::default();
            let mut driver = String::new();
            // VK_KHR_driver_properties is core in 1.2 (Turnip: 1.3+; the
            // Adreno driver on Android 13 phones: 1.1, which has no name
            // for itself here) and an extension before it (PanVK on a
            // Bifrost Mali: Vulkan 1.0/1.1 with the extension).
            let has_driver_properties = props.api_version >= vk::API_VERSION_1_2
                || instance
                    .enumerate_device_extension_properties(pd)
                    .map(|exts| exts.iter().any(|e| e.extension_name_as_c_str().is_ok_and(|n| n == c"VK_KHR_driver_properties")))
                    .unwrap_or(false);
            if loader_version >= vk::API_VERSION_1_1 && has_driver_properties {
                let mut dp = vk::PhysicalDeviceDriverProperties::default();
                let mut p2 = vk::PhysicalDeviceProperties2::default().push_next(&mut dp);
                instance.get_physical_device_properties2(pd, &mut p2);
                driver_id = dp.driver_id;
                let name = dp.driver_name_as_c_str().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let info = dp.driver_info_as_c_str().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                driver = format!("{name} {info}").trim().to_string();
            }
            if driver.is_empty() {
                driver = format!(
                    "vendor 0x{:04x}, driver version {}.{}.{}",
                    props.vendor_id,
                    vk::api_version_major(props.driver_version),
                    vk::api_version_minor(props.driver_version),
                    vk::api_version_patch(props.driver_version)
                );
            }
            let is_mesa = match mesa {
                Mesa::Turnip => driver_id == vk::DriverId::MESA_TURNIP || device.starts_with("Turnip"),
                // (PanVK names its devices "Mali-G52 (Panfrost)" / "Mali-G610 (PanVK)")
                Mesa::PanVk => driver_id == vk::DriverId::MESA_PANVK || driver.to_ascii_lowercase().contains("panvk") || device.to_ascii_lowercase().contains("panvk") || device.to_ascii_lowercase().contains("panfrost"),
            };
            Ok(Probe { device, driver, api: props.api_version, mesa: is_mesa })
        })();
        instance.destroy_instance(None);
        result
    }
}

fn version(v: u32) -> String {
    format!("{}.{}.{}", vk::api_version_major(v), vk::api_version_minor(v), vk::api_version_patch(v))
}

/// The switch: `RUSTUDIO_TURNIP`.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Unset: on where the device qualifies, unless the window turned it
    /// off or the last start with it failed.
    Auto,
    Off,
    /// `1`: the window's choice and the failed marker are ignored.
    On,
    /// `force`: the device checks too.
    Force,
}

fn mode() -> Mode {
    match std::env::var("RUSTUDIO_TURNIP").ok().as_deref().map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("0") | Some("off") | Some("false") | Some("no") => Mode::Off,
        Some("1") | Some("on") | Some("true") | Some("yes") => Mode::On,
        Some("force") => Mode::Force,
        _ => Mode::Auto,
    }
}

/// Swaps the driver in where it applies, before wgpu exists. Never fails
/// the start: anything that goes wrong is logged under `rustudio` (and
/// libadrenotools' own lines under `AdrenoToolsPatch`) and the device's
/// driver stays.
pub fn activate() {
    let started = Instant::now();
    let status = match try_activate() {
        Ok(s) => s,
        Err(s) => s,
    };
    let took = started.elapsed();
    if status.active {
        log::info!(
            "turnip: ACTIVE — {} ({}) answers Vulkan in place of the device's {} driver; {:.0} ms",
            status.device,
            status.driver,
            status.mesa.map_or("own", Mesa::gpus),
            took.as_secs_f64() * 1e3
        );
    } else if status.device.is_empty() {
        log::info!("turnip: not used — {}", status.note);
    } else {
        log::warn!("turnip: not active — {} (Vulkan answered with {} / {}); {:.0} ms", status.note, status.device, status.driver, took.as_secs_f64() * 1e3);
    }
    if !status.active && SHIM_HOOKED.load(std::sync::atomic::Ordering::Relaxed) {
        settle_system_loader();
    }
    let _ = STATUS.set(status);
}

fn skip(note: impl Into<String>, available: bool) -> Status {
    Status { active: false, available, mesa: None, note: note.into(), device: String::new(), driver: String::new() }
}

/// [`skip`] for a device that has a Mesa driver here (the window names it).
fn skip_for(mesa: Mesa, note: impl Into<String>, available: bool) -> Status {
    Status { mesa: Some(mesa), ..skip(note, available) }
}

/// What the libadrenotools.so in the APK can do, read off the file (the
/// strings are plain in the binary, and nothing of the shim should run
/// before the decision to use it is made).
#[derive(Clone, Copy, Debug, Default)]
struct Shim {
    /// Looks for the system's Vulkan loader under the vendor paths too
    /// (libadrenotools-native from 2026-11-17: `find_libvulkan_path`
    /// probes /vendor/lib64, /odm/lib64, /product/lib64 after
    /// /system/lib64; the older builds hard-code /system/lib64/libvulkan.so,
    /// which HyperOS hasn't).
    vendor_paths: bool,
    /// Honours `ADRENOTOOLS_DISABLE=1`: no initialisation of its own — no
    /// Turnip / PanVK load, no hooks on the system loader, no JavaVM
    /// wanted; only `adrenotools_open_libvulkan` on request
    /// (libadrenotools-native 4d4d482, 2026-11-17 12:42, the driver.cpp
    /// rewrite; the older builds always run their `perform_init`).
    opt_out: bool,
}

impl Shim {
    fn read(shim: &std::path::Path) -> Self {
        let bytes = match std::fs::read(shim) {
            Ok(b) => b,
            // A plain APK (no shim): the files check right after says so.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                log::warn!("turnip: {}: {e}", shim.display());
                return Self::default();
            }
        };
        let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
        Self { vendor_paths: has(b"/vendor/lib64/libvulkan.so"), opt_out: has(b"ADRENOTOOLS_DISABLE\0") }
    }
}

/// Ok: a status to keep (active or not); Err: one that says why nothing
/// was done.
fn try_activate() -> Result<Status, Status> {
    let mode = mode();
    if mode == Mode::Off {
        return Err(skip("RUSTUDIO_TURNIP=0", true));
    }
    if cfg!(not(target_arch = "aarch64")) {
        return Err(skip("the Mesa drivers (Turnip, PanVK) ship for arm64-v8a only", false));
    }
    let sdk = super::AndroidApp::sdk_version();
    let dev = Device::read();
    let _ = ADRENO_MODEL.set(dev.adreno_model());
    log::info!("turnip: device: {}; Android API {sdk}", dev.describe());
    // Which Mesa driver this device gets: PanVK on a Mali, Turnip on an
    // Adreno (RUSTUDIO_TURNIP=force takes the same choice without the
    // checks below).
    let mesa = if dev.mali() { Mesa::PanVk } else { Mesa::Turnip };
    if mode != Mode::Force {
        match mesa {
            Mesa::PanVk => {
                // Mali with ARM's kbase kernel driver and a Bifrost or
                // newer core (and, below, a shim that won't go down its own
                // PanVK path).
                if !dev.mali0 {
                    return Err(skip_for(mesa, format!("{} GPU without /dev/mali0: PanVK's build here drives Mali GPUs through ARM's kbase kernel driver only; the device keeps its own driver", dev.other_gpu().unwrap_or_else(|| "Mali".into())), false));
                }
                match dev.mali_bifrost_or_newer() {
                    Some(true) => {}
                    Some(false) => return Err(skip_for(mesa, format!("{}: PanVK drives Bifrost (Mali-G31 and up) and newer Mali GPUs, not Midgard or Utgard ones; the device keeps its own driver", dev.mali_gpu), false)),
                    None => log::info!("turnip: Mali core unknown (no kbase gpuinfo under sysfs): PanVK is tried; the probe and the first frames decide"),
                }
            }
            Mesa::Turnip => {
                // Adreno only, 650 and up, on a stock-enough Android:
                // everything else keeps the vendor's driver.
                if let Some(gpu) = dev.other_gpu() {
                    return Err(skip(format!("{gpu} GPU: Turnip drives Adreno GPUs and PanVK Mali GPUs; nothing here for this one, the device keeps its own driver"), false));
                }
                if !dev.qualcomm() {
                    return Err(skip("not a Snapdragon (no /dev/kgsl-3d0, no Qualcomm SoC in the properties) and not a Mali: the device keeps its own driver", false));
                }
                let model = dev.adreno_model();
                if model == 0 {
                    return Err(skip_for(mesa, format!("Adreno model unknown (gpu {:?}, soc {:?}): only Adreno {ADRENO_MIN} and newer are tried", dev.gpu, dev.soc_model), false));
                }
                if model < ADRENO_MIN {
                    return Err(skip_for(mesa, format!("Adreno {model}: only Adreno {ADRENO_MIN} and newer are tried; the device keeps its own driver"), false));
                }
            }
        }
        if sdk < 31 {
            return Err(skip_for(mesa, format!("Android API {sdk}: libadrenotools-native wants Android 12 (API 31) or newer"), false));
        }
    }
    let skip = |note: String, available: bool| skip_for(mesa, note, available);
    let lib_dir = match native_lib_dir() {
        Ok(d) => d,
        Err(e) => return Err(skip(e, false)),
    };
    let shim = lib_dir.join("libadrenotools.so");
    let shim_can = Shim::read(&shim);
    if shim.exists() {
        log::info!("turnip: libadrenotools in this APK: vendor loader paths {}, own initialisation {}", if shim_can.vendor_paths { "yes" } else { "no" }, if shim_can.opt_out { "switchable off (ADRENOTOOLS_DISABLE)" } else { "always on" });
    }
    if mode != Mode::Force && !dev.hyperos.is_empty() {
        // Xiaomi's HyperOS keeps libvulkan.so under /vendor (or /odm,
        // /product), not /system/lib64: libadrenotools-native builds before
        // 2026-11-17 looked in /system/lib64 only and found nothing to hook,
        // so HyperOS kept its driver. Newer builds probe the vendor paths
        // (driver.cpp find_libvulkan_path); the shim in the APK says which
        // it is.
        if shim_can.vendor_paths {
            log::info!("turnip: HyperOS {}: libadrenotools in this APK probes the vendor's Vulkan loader paths; trying {} (a start that doesn't reach its first frames turns it off)", dev.hyperos, mesa.name());
        } else {
            return Err(skip(format!("HyperOS {}: this APK's libadrenotools looks for the Vulkan loader in /system/lib64 only, which HyperOS doesn't have (a 2026-11-17 or newer libadrenotools-native build probes the vendor paths); the device keeps its own driver", dev.hyperos), false));
        }
    }
    // An older shim initialises itself at load whatever the caller does
    // (perform_init on its fallback thread): on a Mali device its own
    // Mali test may send it down a PanVK path whose hooks of the system
    // loader call a null pointer (4a084a5). A shim with the switch does
    // nothing of its own here.
    if mode != Mode::Force && mesa == Mesa::PanVk && !shim_can.opt_out && dev.shim_would_misfire() {
        return Err(skip(format!("device tree model {:?} names a Mali: this APK's libadrenotools (a build without the ADRENOTOOLS_DISABLE switch) would take its own PanVK path at load, whose hooks call a null pointer; the device keeps its own driver", dev.dt_model), false));
    }
    let driver = lib_dir.join(mesa.file());
    let driver_what = format!("{} (the {} driver)", mesa.file(), mesa.name());
    for (what, p) in [("libadrenotools.so", &shim), (driver_what.as_str(), &driver), ("libmain_hook.so", &lib_dir.join("libmain_hook.so"))] {
        if !p.exists() {
            let how = match mesa {
                Mesa::Turnip => "tools/android/fetch-turnip.sh puts it in before the build",
                Mesa::PanVk => "tools/android/fetch-turnip.sh puts it in before the build unless PANVK=0 (the workflow's panvk input)",
            };
            return Err(skip(format!("{what} isn't in the APK ({}); {how}", lib_dir.display()), false));
        }
    }
    let markers = dir();
    let _ = std::fs::create_dir_all(&markers);
    if mode == Mode::Auto {
        if !wanted() {
            return Err(skip(format!("off (the default): View > GPU Driver turns the {} driver on, from the next start", mesa.name()), true));
        }
        if markers.join("pending").exists() {
            // The last start never got to its first frames.
            let _ = std::fs::rename(markers.join("pending"), markers.join("failed"));
            let _ = std::fs::write(markers.join("failed"), b"");
            log::warn!("turnip: the last start with {} didn't reach its first frames; keeping the device's driver until Graphics Settings > GPU driver > Try again (or RUSTUDIO_TURNIP=1)", mesa.name());
        }
        if failed_last_time() {
            return Err(skip(format!("off since a start with {} didn't reach its first frames (Graphics Settings > GPU driver > Try again)", mesa.name()), true));
        }
    }
    // From here on a crash would be the Mesa driver's: the marker stays
    // until the editor has drawn its first frames (mark_healthy).
    let _ = std::fs::write(markers.join("pending"), format!("{:?}", std::time::SystemTime::now()));

    // Turnip's switches, read when the driver initialises: the start-time
    // TU_DEBUG (libadrenotools' constructor sets one of its own at the
    // load below, so ours goes in again after it) and the switches file
    // the window edits (TU_DEBUG_FILE; made with the defaults the first
    // time, so that its content is the whole of it). PanVK reads neither
    // (its own switches are PANVK_DEBUG and PANVK_KBASE_*; the shim's
    // constructor sets PAN_MESA_DEBUG=kbase besides, Mesa's GL side's).
    if mesa == Mesa::Turnip {
        if !debug_file().exists() {
            reset_debug_flags();
        }
        // (the UBWC default came after the first builds; files written
        // by those get it once)
        add_default_once("noubwc");
    }
    let tu_debug = std::env::var("RUSTUDIO_TU_DEBUG").ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).unwrap_or_else(|| TU_DEBUG_BASE.to_string());
    let switches_file = debug_file();
    let set_tu_debug = || {
        if mesa != Mesa::Turnip {
            return;
        }
        // SAFETY: before any wgpu thread exists; android_main is the only
        // thread reading the environment at this point.
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var("TU_DEBUG", &tu_debug);
            std::env::set_var("TU_DEBUG_FILE", &switches_file);
        };
    };
    set_tu_debug();

    // The shim's own initialisation is left off where it can be
    // (ADRENOTOOLS_DISABLE, read by its constructor and its JNI_OnLoad):
    // this module opens the loader it wants itself, and nothing here goes
    // through the system loader, so its hooks on that (and the JavaVM and
    // Application context they need) have no work. PanVK's older builds
    // enumerate no device without PAN_I_WANT_A_BROKEN_VULKAN_DRIVER (the
    // shim's constructor sets it on a Mali; here too, in case its Mali
    // test and ours differ).
    // SAFETY: before any wgpu thread exists; android_main is the only
    // thread reading the environment at this point.
    #[allow(unused_unsafe)]
    unsafe {
        if shim_can.opt_out {
            std::env::set_var("ADRENOTOOLS_DISABLE", "1");
        }
        if mesa == Mesa::PanVk && std::env::var_os("PAN_I_WANT_A_BROKEN_VULKAN_DRIVER").is_none() {
            std::env::set_var("PAN_I_WANT_A_BROKEN_VULKAN_DRIVER", "1");
        }
    };
    let path = CString::new(shim.to_string_lossy().as_bytes()).map_err(|e| skip(e.to_string(), true))?;
    // SAFETY: dlopen of a library the APK ships; its constructor sets Mesa
    // environment variables and (an older build, or without the switch
    // above) starts a thread that waits for this dlopen to finish
    // (libadrenotools-native src/driver.cpp).
    let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
    if handle.is_null() {
        let _ = std::fs::remove_file(markers.join("pending"));
        return Err(skip(format!("dlopen {}: {}", shim.display(), dlerror()), true));
    }
    set_tu_debug();
    match mesa {
        Mesa::Turnip => {
            let switches = debug_flags();
            log::info!("turnip: TU_DEBUG={tu_debug}; switches ({}): {}", switches_file.display(), if switches.is_empty() { "none".to_string() } else { switches.join(",") });
        }
        Mesa::PanVk => {
            log::info!("turnip: PanVK: PANVK_DEBUG={:?}, PAN_MESA_DEBUG={:?}", std::env::var("PANVK_DEBUG").unwrap_or_default(), std::env::var("PAN_MESA_DEBUG").unwrap_or_default());
        }
    }

    if shim_can.opt_out {
        log::info!("turnip: libadrenotools' own initialisation left off (ADRENOTOOLS_DISABLE); its constructor's Mesa environment is in");
    } else {
        // An older shim: its own initialisation, the way System.loadLibrary
        // would run it, but here and now: it resolves nativeLibraryDir and
        // the cache folder through JNI, opens a hooked copy of libvulkan.so
        // with the driver behind it, and patches the system loader's
        // vkGetInstanceProcAddr / vkGetDeviceProcAddr. Its fallback thread
        // does the same a moment later when nobody calls this; a
        // std::call_once inside makes the two one, so by the time this
        // returns its swap is done (or has failed, which its logcat lines
        // say). Nothing here depends on that patch (module docs), but the
        // fallback start has to settle the system loader after it
        // (SHIM_HOOKED).
        SHIM_HOOKED.store(true, std::sync::atomic::Ordering::Relaxed);
        let vm = super::app().map(|a| a.vm_as_ptr()).unwrap_or(std::ptr::null_mut());
        if vm.is_null() {
            let _ = std::fs::remove_file(markers.join("pending"));
            return Err(skip("no JavaVM for libadrenotools' initialisation".into(), true));
        }
        type JniOnLoad = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
        let on_load: JniOnLoad = match unsafe { symbol(handle, c"JNI_OnLoad") } {
            Ok(f) => f,
            Err(e) => {
                let _ = std::fs::remove_file(markers.join("pending"));
                return Err(skip(format!("libadrenotools.so without an entry point: {e}"), true));
            }
        };
        let t = Instant::now();
        // SAFETY: the library's JNI entry, given the process's JavaVM; it
        // attaches this thread itself when it has to.
        let jni_version = unsafe { on_load(vm, std::ptr::null_mut()) };
        log::info!("turnip: libadrenotools initialised (JNI {:#x}) in {:.0} ms", jni_version, t.elapsed().as_secs_f64() * 1e3);
    }

    // Mesa's disk shader cache folder (the older shim's initialisation
    // sets it from the app's cache dir; the newer one, left off, doesn't).
    if std::env::var_os("MESA_SHADER_CACHE_DIR").is_none() {
        let d = super::cache_dir().join(match mesa {
            Mesa::Turnip => "turnip_shader_cache",
            Mesa::PanVk => "panvk_shader_cache",
        });
        let _ = std::fs::create_dir_all(&d);
        // SAFETY: as for TU_DEBUG above.
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var("MESA_SHADER_CACHE_DIR", &d)
        };
    }

    // A loader of our own with the Mesa driver behind it, for wgpu (the
    // module docs on why the fork's patch of the system loader isn't
    // relied on).
    let t = Instant::now();
    // SAFETY: `handle` is libadrenotools.so, loaded above.
    let gipa = match unsafe { open_turnip_loader(handle, &lib_dir, mesa) } {
        Ok(g) => g,
        Err(e) => {
            let _ = std::fs::remove_file(markers.join("pending"));
            return Err(skip(format!("libadrenotools couldn't open a {}-backed loader: {e}; the device's driver is in use", mesa.name()), true));
        }
    };
    log::info!("turnip: {}-backed loader opened in {:.0} ms", mesa.name(), t.elapsed().as_secs_f64() * 1e3);
    let _ = LOADER.set(gipa);
    let entry = entry().expect("set just above");

    // Which driver answers through it.
    let t = Instant::now();
    let probe = probe(&entry, mesa);
    let probe_ms = t.elapsed().as_secs_f64() * 1e3;
    let mut status = match probe {
        Ok(p) => {
            log::info!("turnip: probe: {} — {} (Vulkan {}) in {probe_ms:.0} ms", p.device, p.driver, version(p.api));
            Status {
                active: p.mesa,
                available: true,
                mesa: Some(mesa),
                note: if p.mesa {
                    format!("{} answers this run's Vulkan calls", mesa.name())
                } else {
                    format!("libadrenotools' loader answered with the device's driver ({} didn't load; its logcat lines, tag AdrenoToolsPatch, say why)", mesa.name())
                },
                device: p.device,
                driver: p.driver,
            }
        }
        Err(e) => {
            // The start goes on with the device's driver (wgpu makes its
            // own instance, and has OpenGL ES to fall back to).
            log::error!("turnip: the Vulkan probe on the {}-backed loader failed: {e}", mesa.name());
            Status { active: false, available: true, mesa: Some(mesa), note: format!("Vulkan probe on the {}-backed loader failed: {e}; the device's driver is in use", mesa.name()), device: String::new(), driver: String::new() }
        }
    };
    if !status.active {
        // The device's driver is in use: nothing to guard.
        let _ = std::fs::remove_file(markers.join("pending"));
    }

    // The GPU's top clocks, on request (a KGSL property; thermal limits
    // still apply). Independent of which driver is in use. Never on by
    // itself: the KGSL_PROP_PWRCTRL ioctl behind it crashes some Adreno
    // kernels — a reboot of the whole device — which is why
    // libadrenotools-native stopped sending it at init (its 2026-11-18
    // reboot fixes); the build pinned in tools/android/fetch-turnip.sh
    // has that.
    if std::env::var("RUSTUDIO_GPU_TURBO").map_or(false, |v| v.trim() == "1") {
        type SetTurbo = unsafe extern "C" fn(bool);
        match unsafe { symbol::<SetTurbo>(handle, c"adrenotools_set_turbo") } {
            // SAFETY: the library's documented entry; takes a bool.
            Ok(f) => {
                unsafe { f(true) };
                log::info!("turnip: adrenotools_set_turbo(true) (RUSTUDIO_GPU_TURBO=1)");
                status.note.push_str("; GPU turbo asked for");
            }
            Err(e) => log::warn!("turnip: RUSTUDIO_GPU_TURBO: {e}"),
        }
    }
    Ok(status)
}

/// The renderer's wgpu instance, adapter, device and queue on the
/// Turnip-backed loader, for `egui_wgpu::WgpuSetup::Existing`: what
/// `wgpu_options` hands eframe when [`activate`] succeeded. `template` is
/// the configuration the ordinary start would use; its instance flags,
/// memory thresholds and device descriptor apply here too. None (with the
/// reason logged and kept for Graphics Settings) when Turnip isn't active
/// or anything fails: the ordinary start goes ahead on the device's
/// driver.
pub fn wgpu_setup(template: &egui_wgpu::WgpuSetupCreateNew) -> Option<egui_wgpu::WgpuSetupExisting> {
    if !STATUS.get().is_some_and(|s| s.active) {
        return None;
    }
    let t = Instant::now();
    match build_wgpu(template) {
        Ok((setup, note)) => {
            log::info!("turnip: wgpu: {note}; {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
            let _ = RENDERER.set(note);
            Some(setup)
        }
        Err(e) => {
            log::error!("turnip: wgpu on the Mesa driver's loader failed, the device's driver is in use: {e}");
            let _ = RENDERER.set(format!("the device's driver ({e})"));
            // Nothing of the Mesa driver's is in use: nothing to guard.
            let _ = std::fs::remove_file(dir().join("pending"));
            settle_system_loader();
            None
        }
    }
}

/// [`wgpu_setup`]'s work: the objects and a line saying what they are on.
fn build_wgpu(template: &egui_wgpu::WgpuSetupCreateNew) -> Result<(egui_wgpu::WgpuSetupExisting, String), String> {
    use egui_wgpu::wgpu;
    let entry = entry().ok_or("no loader with the Mesa driver")?;
    let desc = &template.instance_descriptor;
    // No validation layers in the APK, and debug utils would want a
    // messenger of ours: release behaviour either way.
    let flags = desc.flags.difference(wgpu::InstanceFlags::DEBUG | wgpu::InstanceFlags::VALIDATION | wgpu::InstanceFlags::GPU_BASED_VALIDATION);
    let sdk = super::AndroidApp::sdk_version().max(0) as u32;
    // SAFETY: the calls wgpu-hal's Instance::init makes, on an entry whose
    // loader stays loaded; the instance is handed to wgpu-hal, which owns
    // it from then on.
    let hal = unsafe {
        let instance_api_version = entry
            .try_enumerate_instance_version()
            .map_err(|e| format!("vkEnumerateInstanceVersion: {e}"))?
            .unwrap_or(vk::API_VERSION_1_0);
        let app_info = vk::ApplicationInfo::default()
            .application_name(c"rustudio")
            .application_version(1)
            .engine_name(c"wgpu-hal")
            .engine_version(2)
            // wgpu-hal's choice: a 1.0 loader takes nothing else, 1.3 is
            // the most it knows.
            .api_version(if instance_api_version < vk::API_VERSION_1_1 { vk::API_VERSION_1_0 } else { vk::API_VERSION_1_3 });
        let extensions = wgpu::hal::vulkan::Instance::desired_extensions(&entry, instance_api_version, flags).map_err(|e| format!("instance extensions: {e}"))?;
        let names: Vec<*const c_char> = extensions.iter().map(|e| e.as_ptr()).collect();
        let create_info = vk::InstanceCreateInfo::default().application_info(&app_info).enabled_extension_names(&names);
        let raw = entry.create_instance(&create_info, None).map_err(|e| format!("vkCreateInstance: {e}"))?;
        wgpu::hal::vulkan::Instance::from_raw(entry, raw, instance_api_version, sdk, None, extensions, flags, desc.memory_budget_thresholds, false, None)
            .map_err(|e| format!("wgpu-hal instance: {e}"))?
    };
    // SAFETY: a Vulkan instance made for it just above; wgpu owns it from
    // here on.
    let instance = unsafe { wgpu::Instance::from_hal::<wgpu::hal::api::Vulkan>(hal) };
    let adapter = block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN))
        .into_iter()
        .next()
        .ok_or("the instance on the Mesa driver's loader has no adapter")?;
    let info = adapter.get_info();
    let device_desc = (template.device_descriptor)(&adapter);
    let (device, queue) = block_on(adapter.request_device(&device_desc)).map_err(|e| format!("request_device on {}: {e}", info.name))?;
    let note = format!("{} — {} {}", info.name, info.driver, info.driver_info.replace('\n', " ")).trim().to_string();
    Ok((egui_wgpu::WgpuSetupExisting { instance, adapter, device, queue }, note))
}

/// Runs a wgpu future to its end on this thread. wgpu's native futures
/// are ready at their first poll; this only has to be correct.
fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        match fut.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(v) => return v,
            std::task::Poll::Pending => std::thread::yield_now(),
        }
    }
}

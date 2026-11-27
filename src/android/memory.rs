//! The process's memory, in logcat. A phone gives no warning before the
//! kernel or `lmkd` ends a process that has grown too far, and a run
//! that ends that way leaves nothing but `dumpsys activity exit-info`
//! (`pss=5.3GB`). While the app runs, [`tick`] reads the resident size
//! (`/proc/self/statm`) once a second and logs each 256 MB it grows past
//! (and each 512 MB it falls back under), with the place being opened, so
//! `adb logcat -s rustudio` tells what the memory went into and when:
//!
//! ```text
//! memory: 1280 MB resident (+256 MB in 2.1 s), opening Game.rbxl
//! ```
//!
//! Each line is followed by what the editor holds — the renderer's
//! meshes, textures and items, the shown DataModel's size (`App::ui`
//! passes it) — and the Play worker logs its own two Lua heaps and
//! DataModels (play_worker.rs `memory_log`), so a growth can be placed.
//!
//! The first line also tells the device's memory (`ActivityManager
//! .getMemoryInfo`: total, available now, and the level under which the
//! system starts ending background processes). When the system sends the
//! low-memory notice (`Activity.onLowMemory`, hooked in lifecycle.rs) the
//! free heap pages go back at once (`heap::trim`) and a line says so.
//!
//! With each line goes the GPU allocator's account ([`gpu_line`]): on a
//! phone the GPU's memory is system memory — Turnip's device heap is
//! host memory, counted in the resident size — and a run of 2026-11-25
//! reached 5.3 GB resident with 29 + 144 MB of Lua heaps and 48 MB of
//! meshes before the device was lost to `Out of memory`; the resident
//! size alone could not say whether the rest was the GPU's.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How often the size is read.
const EVERY: Duration = Duration::from_secs(1);
/// A line each time the size passes another of these going up.
const STEP_UP_MB: u64 = 256;
/// ... and when it has fallen this far below the last line's size.
const STEP_DOWN_MB: u64 = 512;

struct Watch {
    read: Instant,
    /// The size the last line reported, and when.
    logged: Option<(u64, Instant)>,
}

static WATCH: Mutex<Option<Watch>> = Mutex::new(None);

/// Resident set size in MB, from `/proc/self/statm` (pages).
pub fn resident_mb() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    // SAFETY: sysconf has no preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let page = if page > 0 { page as u64 } else { 4096 };
    Some(pages * page / (1024 * 1024))
}

/// The device's memory as `ActivityManager.getMemoryInfo` tells it, in MB.
#[derive(Clone, Copy, Debug, Default)]
pub struct DeviceMemory {
    pub total_mb: u64,
    pub available_mb: u64,
    /// `MemoryInfo.threshold`: available memory under which the system
    /// starts ending background processes.
    pub threshold_mb: u64,
    pub low: bool,
}

/// `ActivityManager.getMemoryInfo`, when the Activity is at hand.
pub fn device_memory() -> Option<DeviceMemory> {
    use jni::objects::JValue;
    use jni::{jni_sig, jni_str};
    super::jni::with_activity(|env, activity| {
        let am = super::jni::system_service(env, activity, "activity")?;
        if am.is_null() {
            return Ok(None);
        }
        let info = env.new_object(jni_str!("android/app/ActivityManager$MemoryInfo"), jni_sig!("()V"), &[])?;
        env.call_method(
            &am,
            jni_str!("getMemoryInfo"),
            jni_sig!("(Landroid/app/ActivityManager$MemoryInfo;)V"),
            &[JValue::Object(&info)],
        )?;
        let mb = |v: i64| (v.max(0) as u64) / (1024 * 1024);
        Ok(Some(DeviceMemory {
            total_mb: mb(env.get_field(&info, jni_str!("totalMem"), jni_sig!("J"))?.j()?),
            available_mb: mb(env.get_field(&info, jni_str!("availMem"), jni_sig!("J"))?.j()?),
            threshold_mb: mb(env.get_field(&info, jni_str!("threshold"), jni_sig!("J"))?.j()?),
            low: env.get_field(&info, jni_str!("lowMemory"), jni_sig!("Z"))?.z()?,
        }))
    })
    .unwrap_or_else(|e| {
        log::warn!("memory: {e}");
        None
    })
}

fn device_line() -> String {
    match device_memory() {
        Some(d) => format!(
            "; device: {} MB, {} MB available, low under {} MB{}",
            d.total_mb,
            d.available_mb,
            d.threshold_mb,
            if d.low { " (low now)" } else { "" }
        ),
        None => String::new(),
    }
}

/// Once a frame: logs the resident size when it has moved a step (the
/// module comment), and answers the system's low-memory notice. `doing`
/// names what the app is at (the place opening).
pub fn tick(doing: &str, detail: impl FnOnce() -> String) {
    let now = Instant::now();
    if super::lifecycle::take_low_memory() {
        rustudio_core::heap::trim();
        log::warn!(
            "memory: the system is low on memory at {} MB resident{}{}",
            resident_mb().unwrap_or(0),
            device_line(),
            if doing.is_empty() { String::new() } else { format!(", {doing}") }
        );
    }
    let mut guard = WATCH.lock().unwrap_or_else(|e| e.into_inner());
    let w = guard.get_or_insert_with(|| Watch { read: now - EVERY, logged: None });
    if now.duration_since(w.read) < EVERY {
        return;
    }
    w.read = now;
    let Some(mb) = resident_mb() else { return };
    let first = w.logged.is_none();
    let line = match w.logged {
        None => true,
        Some((last, _)) => mb >= last + STEP_UP_MB || mb + STEP_DOWN_MB <= last,
    };
    if !line {
        return;
    }
    let change = match w.logged {
        Some((last, when)) => format!(
            " ({}{} MB in {:.1} s)",
            if mb >= last { "+" } else { "-" },
            mb.abs_diff(last),
            now.duration_since(when).as_secs_f32()
        ),
        None => String::new(),
    };
    let doing = if doing.is_empty() { String::new() } else { format!(", {doing}") };
    let device = if first { device_line() } else { String::new() };
    log::info!("memory: {mb} MB resident{change}{doing}{device}");
    w.logged = Some((mb, now));
    // (the lock goes before `detail()` reads the editor's own counters)
    drop(guard);
    // What the editor holds (the renderer's meshes, textures and items;
    // the shown DataModel), with each line: which of them grew.
    let detail = detail();
    if !detail.is_empty() {
        log::info!("memory: {detail}");
    }
}

/// The GPU allocator's account (wgpu's Vulkan backend sub-allocates
/// through gpu-allocator; `Device::generate_allocator_report`): what is
/// allocated and what the memory blocks reserve, and the largest
/// allocations by their label, so a growth can be placed — a texture
/// pool, the GUI's images, egui's buffers. "" where the backend has no
/// report (OpenGL).
pub fn gpu_line(device: &egui_wgpu::wgpu::Device) -> String {
    let Some(report) = device.generate_allocator_report() else { return String::new() };
    let mut by_name: std::collections::HashMap<&str, (u64, usize)> = std::collections::HashMap::new();
    for a in &report.allocations {
        let e = by_name.entry(a.name.as_str()).or_default();
        e.0 += a.size;
        e.1 += 1;
    }
    let mut top: Vec<(&str, (u64, usize))> = by_name.into_iter().collect();
    top.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    let top: Vec<String> = top
        .iter()
        .take(8)
        .map(|(n, (bytes, count))| format!("{} {} MB ×{count}", if n.is_empty() { "(unnamed)" } else { n }, bytes >> 20))
        .collect();
    format!(
        "gpu: {} MB allocated, {} MB reserved ({} allocations in {} blocks); largest: {}",
        report.total_allocated_bytes >> 20,
        report.total_reserved_bytes >> 20,
        report.allocations.len(),
        report.blocks.len(),
        top.join(", ")
    )
}

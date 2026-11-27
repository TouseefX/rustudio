//! Says which GPU rustudio runs on and why it lost it. A lost device (driver
//! reset, a Windows TDR on a long frame, out of GPU memory) used to surface
//! as an egui-wgpu staging-buffer panic with no cause; now the reason goes to
//! the diagnostic log and the crash report.

use egui_wgpu::wgpu;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;

static ADAPTER: OnceLock<String> = OnceLock::new();
static LOST: OnceLock<String> = OnceLock::new();
/// Set once any GPU allocation hits OutOfMemory (uncaptured or caught by a
/// scoped fallback). Tells the renderer to step down heavy optional features
/// (4× MSAA, HBAO, DoF, extra mip copies) and compact texture resolution so
/// particles, beams, trails, world GUIs and `egui_wgpu` keep drawing.
static OOM_SEEN: AtomicBool = AtomicBool::new(false);
/// Monotonically bumped on every `OutOfMemory` event so texture/mip/target
/// caches immediately reclaim optional VRAM instead of shutting down effects.
static OOM_EPOCH: AtomicU64 = AtomicU64::new(0);
/// The first GPU error nothing captured (out of memory, validation), for the
/// editor's Output; taken once.
static UNCAPTURED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static UNCAPTURED_COUNT: AtomicUsize = AtomicUsize::new(0);

fn describe(info: &wgpu::AdapterInfo) -> String {
    format!(
        "{} ({:?}, {:?}, driver {} {})",
        info.name, info.backend, info.device_type, info.driver, info.driver_info
    )
}

/// Log the adapter and watch the device for loss. Call once per device.
pub fn watch(adapter: &wgpu::Adapter, device: &wgpu::Device) {
    let gpu = describe(&adapter.get_info());
    log::info!(target: "rustudio::gpu", "GPU: {gpu}");
    let _ = ADAPTER.set(gpu);
    // wgpu's default for an error no error scope catches is a panic: an
    // allocation the GPU refused (a 2.4M-instance place on an RTX 4070 Ti,
    // 2026-11-22: "wgpu error: Out of Memory" in Device::create_texture)
    // took the whole editor down. Log it with what failed and keep going;
    // the resource that failed just doesn't draw.
    device.on_uncaptured_error(std::sync::Arc::new(|error: wgpu::Error| {
        let n = UNCAPTURED_COUNT.fetch_add(1, Ordering::Relaxed);
        let gpu = ADAPTER.get().map(String::as_str).unwrap_or("unknown GPU");
        let kind = match &error {
            wgpu::Error::OutOfMemory { .. } => {
                OOM_SEEN.store(true, Ordering::Relaxed);
                OOM_EPOCH.fetch_add(1, Ordering::Relaxed);
                "out of GPU memory"
            }
            wgpu::Error::Validation { .. } => "GPU validation error",
            wgpu::Error::Internal { .. } => "internal GPU error",
        };
        // Every one goes to the diagnostic log (the first 200 in full).
        if n < 200 {
            log::error!(target: "rustudio::gpu", "{kind} on {gpu}: {error:?}");
        }
        if n == 0 {
            let detail = error.to_string();
            let first = detail.lines().next().unwrap_or("").to_string();
            if let Ok(mut slot) = UNCAPTURED.lock() {
                if slot.is_none() {
                    *slot = Some(format!(
                        "{kind} on {gpu}: {first}. Some parts or textures may not draw; the diagnostic log has the details."
                    ));
                }
            }
        }
    }));
    device.set_device_lost_callback(|reason, message| {
        if matches!(reason, wgpu::DeviceLostReason::Destroyed) {
            return;
        }
        let gpu = ADAPTER.get().map(String::as_str).unwrap_or("unknown GPU");
        let text = format!("GPU device lost on {gpu}: {message}");
        log::error!(target: "rustudio::gpu", "{text}");
        let _ = LOST.set(text);
    });
}

/// True once any GPU allocation has reported `OutOfMemory`.
/// Used to keep heavy optional features (4× MSAA, HBAO, DoF, extra mip copies)
/// stepped down so VRAM does not bounce back into OOM — never to stop drawing
/// particles, beams, trails, or world GUIs.
pub fn oom_seen() -> bool {
    OOM_SEEN.load(Ordering::Relaxed)
}

/// Incremented on every `OutOfMemory` event so caches immediately reclaim
/// optional GPU memory (mip copies, unused GUI/FX textures, DoF targets).
pub fn oom_epoch() -> u64 {
    OOM_EPOCH.load(Ordering::Relaxed)
}

/// Record an `OutOfMemory` caught by a scoped allocation fallback so the
/// renderer reclaims optional caches and compacts texture resolution instead
/// of halting effects or producing black frames.
pub fn mark_oom(label: &str, detail: &str) {
    let first = !OOM_SEEN.swap(true, Ordering::Relaxed);
    OOM_EPOCH.fetch_add(1, Ordering::Relaxed);
    let gpu = ADAPTER.get().map(String::as_str).unwrap_or("unknown GPU");
    if first {
        log::warn!(target: "rustudio::gpu", "GPU memory tight while allocating {label} on {gpu}: {detail}");
        if let Ok(mut slot) = UNCAPTURED.lock() {
            if slot.is_none() {
                *slot = Some(format!(
                    "GPU memory tight on {gpu} ({label}): compacting textures and trimming optional caches to keep rendering."
                ));
            }
        }
    }
}

/// Log once when GUI/FX texture uploads reach their VRAM budget and begin
/// LRU eviction / resolution compaction.
pub fn warn_gui_budget(used_bytes: usize, budget_bytes: usize) {
    log::warn!(
        target: "rustudio::gpu",
        "GUI/FX texture VRAM budget reached ({} / {} MiB); evicting unused textures and compacting resolution",
        used_bytes >> 20,
        budget_bytes >> 20,
    );
}

/// Log when optional VRAM caches are purged to recover from memory pressure.
pub fn note_vram_reclaimed(label: &str, bytes: usize) {
    if bytes > 0 {
        log::info!(
            target: "rustudio::gpu",
            "reclaimed {} KiB of GPU memory from {label}",
            bytes >> 10,
        );
    }
}

/// The first uncaptured GPU error, once (for a warning in Output).
pub fn take_uncaptured() -> Option<String> {
    UNCAPTURED.lock().ok()?.take()
}

/// Why the device was lost, once it has been.
pub fn lost() -> Option<&'static str> {
    LOST.get().map(String::as_str)
}

/// Records the device as lost (for a handler that replaces `watch`'s, like
/// the Android build's, and for a renderer step that finds the device's
/// buffers gone before wgpu has said why): `lost` answers from here on and
/// uploads that would panic on destroyed buffers are skipped
/// (world_gui.rs `stage`).
pub fn mark_lost(text: String) {
    let _ = LOST.set(text);
}

/// `device.poll(kind)` unless the device is lost: wgpu makes a poll on a
/// lost device a fatal error ("Error in Device::poll: Parent device is
/// lost" — a panic, not a callback), and a frame's per-frame polls
/// (gpu_timing.rs, clouds.rs, render.rs's upload flush) would end the
/// process before the loss is handled in order (the Android build saves
/// the open place and restarts, src/android gpu_watch; the desktop's
/// `check` at the next frame's top). False when the poll was skipped.
pub fn poll(device: &wgpu::Device, kind: wgpu::PollType) -> bool {
    if lost().is_some() {
        return false;
    }
    let _ = device.poll(kind);
    true
}

/// Call at the top of a frame: once the device is lost nothing more can be
/// drawn, so stop with the reason in the crash report (shown on the next
/// start) instead of a frozen window.
pub fn check() {
    if let Some(why) = lost() {
        panic!("{why}. Update the graphics driver; WGPU_BACKEND=dx12 or WGPU_BACKEND=vulkan picks the other graphics API.");
    }
}

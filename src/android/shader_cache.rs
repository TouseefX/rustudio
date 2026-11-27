//! The shader caches on the device, and the switch to rebuild them.
//!
//! Nothing in rustudio is precompiled: every shader is WGSL that this
//! device's Vulkan driver compiles at run time (at the start, and at a
//! pass's first use; the render crate's warmup.rs makes most of it happen
//! on a worker in the first seconds). What is kept between runs are the
//! driver's own caches, both written by this device for this driver — a
//! desktop's are never used here:
//!
//! - the wgpu/Vulkan pipeline cache blob under
//!   `<cache>/rustudio/pipelines/wgpu_pipeline_cache_vulkan_<vendor>_<device>.bin`
//!   (crates/rustudio-render/src/pipeline_cache.rs), keyed by adapter and
//!   driver version;
//! - Mesa's disk shader cache under `<cache>/turnip_shader_cache` or
//!   `<cache>/panvk_shader_cache` when the APK's Mesa driver draws
//!   (turnip.rs sets `MESA_SHADER_CACHE_DIR`), keyed by the driver build.
//!
//! "Rebuild shader caches at the next start" (View > GPU Driver) leaves
//! the marker [`set_rebuild`] reads; [`apply_at_start`] then deletes both
//! before the driver loads, and that start compiles every shader afresh
//! — a slower first frame once, then the caches fill again. The same with
//! `RUSTUDIO_SHADER_REBUILD=1` in `debug.rustudio.env`. For after a
//! driver update, or when frames stutter where they didn't.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The one-shot marker, under the app's files folder.
fn marker() -> PathBuf {
    super::files_dir().join("shader_cache_rebuild")
}

/// Whether the next start rebuilds the caches.
pub fn rebuild_wanted() -> bool {
    marker().exists()
}

/// The GPU Driver window's switch.
pub fn set_rebuild(on: bool) {
    let m = marker();
    if on {
        if let Some(d) = m.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Err(e) = std::fs::write(&m, b"") {
            log::warn!("shader cache: writing {}: {e}", m.display());
        }
    } else {
        let _ = std::fs::remove_file(&m);
    }
}

/// What this start did to the caches, for View > GPU Driver (None: nothing).
static NOTE: OnceLock<String> = OnceLock::new();

pub fn note() -> Option<&'static str> {
    NOTE.get().map(String::as_str)
}

/// The folders a shader cache can be in: the pipeline cache's, and Mesa's
/// (ours, and the names Mesa picks itself under `XDG_CACHE_HOME`, which
/// export_env points at the app's cache folder).
fn cache_dirs() -> Vec<PathBuf> {
    let cache = super::cache_dir();
    let mut dirs = vec![rustudio_core::assets::cache_dir().join("pipelines")];
    for name in ["turnip_shader_cache", "panvk_shader_cache", "mesa_shader_cache", "mesa_shader_cache_db", "mesa_shader_cache_sf"] {
        dirs.push(cache.join(name));
    }
    dirs
}

/// Removes `dir` and all in it; (files, bytes) removed.
fn remove_tree(dir: &Path) -> (usize, u64) {
    let (mut files, mut bytes) = (0, 0);
    fn walk(dir: &Path, files: &mut usize, bytes: &mut u64) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, files, bytes);
                let _ = std::fs::remove_dir(&p);
            } else {
                *bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
                if std::fs::remove_file(&p).is_ok() {
                    *files += 1;
                }
            }
        }
    }
    walk(dir, &mut files, &mut bytes);
    let _ = std::fs::remove_dir(dir);
    (files, bytes)
}

/// At the start, after `export_env` and before any GPU driver loads: with
/// the marker there, or `RUSTUDIO_SHADER_REBUILD` set, every shader cache
/// goes, and this run compiles everything afresh on this device.
pub fn apply_at_start() {
    let env = std::env::var_os("RUSTUDIO_SHADER_REBUILD").is_some_and(|v| v != "0");
    if !(env || rebuild_wanted()) {
        return;
    }
    let _ = std::fs::remove_file(marker());
    let (mut files, mut bytes) = (0usize, 0u64);
    let mut names = Vec::new();
    for d in cache_dirs() {
        if !d.exists() {
            continue;
        }
        let (f, b) = remove_tree(&d);
        if f > 0 {
            names.push(d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        }
        files += f;
        bytes += b;
    }
    let line = if files == 0 {
        "Shader caches: nothing to rebuild (none on the device yet); every shader is compiled afresh this run.".to_string()
    } else {
        format!(
            "Shader caches rebuilt this start: {files} file(s), {} KB removed ({}); every shader was compiled afresh by this device's driver.",
            bytes / 1024,
            names.join(", ")
        )
    };
    log::info!("shader cache: {line}");
    eprintln!("[startup] {line}");
    let _ = NOTE.set(line);
}

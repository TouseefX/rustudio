//! The shader warm-up: the passes a session otherwise compiles at their
//! first use — the viewport's pipeline sets for the sample counts it will
//! draw with, the world GUI, highlight, adornment and cloud passes, the
//! depth-reading post passes, the ViewportFrame blit — built on a worker
//! thread from the moment the renderer exists, so the first frame, the
//! first Play, the first selection or the first cloudy place don't pay
//! the driver's compiler on the UI thread.
//!
//! Every shader is WGSL compiled by this device's driver, here, at run
//! time: nothing is precompiled into a build, and the only things kept
//! between runs are the driver's own caches (pipeline_cache.rs; Mesa's
//! disk shader cache). The warm-up makes the first run on a driver fill
//! them in the background, and a primed run have every pass ready a few
//! hundred milliseconds in.
//!
//! The worker owns nothing of the renderer but reference-counted handles
//! (the device, queue and bind group layouts); what it makes is handed
//! over on a channel and the frame loop takes it ([`Warmup::poll`]). A
//! frame that needs a pass the worker hasn't delivered makes it itself
//! (and the worker skips it, `done`); the viewport sets are the exception,
//! worth waiting for when the worker is about to deliver one
//! ([`Warmup::wait_viewport`]): thirty pipelines through the compiler,
//! made twice otherwise.
//!
//! On by default on Android (where the first run on a driver shows a black
//! window for seconds and a mid-session compile is a hitch the Automatic
//! quality controller reads as a slow frame); elsewhere the worker builds
//! the one-sample viewport set only when a session is likely to draw with
//! it (as before this module), the full warm-up with
//! `RUSTUDIO_SHADER_WARMUP=1`. `RUSTUDIO_SHADER_WARMUP=0` (or the older
//! `RUSTUDIO_NO_PIPELINE_PREBUILD`) turns it all off. Not on OpenGL: a
//! second thread compiling holds the shared GL context (render.rs).
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use egui_wgpu::wgpu;

use super::{sample_slot, ViewportPipelines, DEPTH_FORMAT, DISPLAY_FORMAT, HDR_FORMAT};

/// A pass the worker builds, by kind; with a sample slot it names one
/// `done` bit (the worker's and the frame loop's "made already" flags).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    Viewport,
    WorldGui,
    Highlight,
    Adorn,
    Clouds,
    PostfxDepth,
    VfBlit,
}

fn bit(kind: Kind, slot: usize) -> u32 {
    1 << (kind as u32 * 4 + slot as u32)
}

/// What the worker delivered, for the renderer's slots.
pub(super) enum Warmed {
    Viewport(u32, ViewportPipelines),
    WorldGui(u32, crate::world_gui::WorldGuiPass),
    Highlight(u32, crate::highlight::HighlightPass),
    Adorn(u32, crate::adorn::AdornPass),
    Clouds(u32, crate::clouds::CloudsGpu),
    PostfxDepth(u32, crate::postfx::DepthPipelines),
    VfBlit(super::vf::Blit),
}

/// What the worker builds with: the renderer's layouts (reference-counted
/// handles) and the sample counts the session is going to draw with.
pub(super) struct Job {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub globals: wgpu::BindGroupLayout,
    pub tex: wgpu::BindGroupLayout,
    pub glass: wgpu::BindGroupLayout,
    pub terrain: wgpu::BindGroupLayout,
    pub reduced: bool,
    /// The clouds' globals layout (the sky's with the reduced binding set)
    /// and the shader prelude they are compiled with.
    pub clouds_globals: wgpu::BindGroupLayout,
    pub clouds_source: String,
    /// The depth post passes' pipeline layout and HDR format, at one sample
    /// and multisampled (postfx.rs `depth_pipes_job`).
    pub postfx_depth: [(wgpu::PipelineLayout, wgpu::TextureFormat); 2],
    pub vertex_stride: u64,
    /// Viewport sets to build, first the one the first frame most likely
    /// needs; a frame that needs the next one waits for it.
    pub viewport: Vec<u32>,
    /// Sample counts the first-use passes are built for (empty: only the
    /// viewport sets, the pre-warm-up behaviour).
    pub passes: Vec<u32>,
}

/// The worker's side as the renderer sees it.
pub(super) struct Warmup {
    rx: Receiver<Warmed>,
    /// Viewport sets the worker still owes, in its order.
    viewport: Vec<u32>,
    done: Arc<AtomicU32>,
    /// The worker is gone (everything delivered, or it failed).
    finished: bool,
}

/// What the last warm-up did, for View > GPU Driver.
static SUMMARY: Mutex<Option<String>> = Mutex::new(None);

/// One line on the warm-up's outcome, once the worker finished.
pub fn summary() -> Option<String> {
    SUMMARY.lock().ok().and_then(|s| s.clone())
}

impl Warmup {
    /// Spawns the worker (None when the thread can't be made).
    pub(super) fn start(job: Job) -> Option<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        let done = Arc::new(AtomicU32::new(0));
        let viewport = job.viewport.clone();
        let worker_done = done.clone();
        let spawned = std::thread::Builder::new()
            .name("rustudio-pipelines".into())
            .spawn(move || work(job, tx, worker_done));
        if spawned.is_err() {
            log::warn!("shader warm-up: no worker thread; passes are built at their first use");
            return None;
        }
        if let Ok(mut s) = SUMMARY.lock() {
            *s = Some(format!("shader warm-up: building the viewport sets at {viewport:?} sample(s) a pixel on the worker…"));
        }
        Some(Warmup { rx, viewport, done, finished: false })
    }

    /// The frame loop built `kind` for `samples` itself: the worker skips it.
    pub(super) fn mark_local(&mut self, kind: Kind, samples: u32) {
        let slot = if kind == Kind::PostfxDepth { usize::from(samples > 1) } else { sample_slot(samples) };
        self.done.fetch_or(bit(kind, slot), Ordering::Relaxed);
        if kind == Kind::Viewport {
            self.viewport.retain(|&v| v != samples);
        }
    }

    /// Whatever the worker delivered since the last call (never blocks).
    pub(super) fn poll(&mut self) -> Vec<Warmed> {
        let mut got = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(item) => {
                    self.note(&item);
                    got.push(item);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.finished = true;
                    self.viewport.clear();
                    break;
                }
            }
        }
        got
    }

    /// Waits for the viewport set for `samples` when it is the next one the
    /// worker delivers (everything delivered before it comes along);
    /// returns at once otherwise, and the caller builds the set itself.
    pub(super) fn wait_viewport(&mut self, samples: u32) -> Vec<Warmed> {
        let mut got = Vec::new();
        if self.viewport.first() != Some(&samples) {
            return got;
        }
        loop {
            match self.rx.recv() {
                Ok(item) => {
                    let hit = matches!(item, Warmed::Viewport(s, _) if s == samples);
                    self.note(&item);
                    got.push(item);
                    if hit {
                        break;
                    }
                }
                Err(_) => {
                    self.finished = true;
                    self.viewport.clear();
                    break;
                }
            }
        }
        got
    }

    pub(super) fn finished(&self) -> bool {
        self.finished
    }

    fn note(&mut self, item: &Warmed) {
        if let Warmed::Viewport(s, _) = item {
            self.viewport.retain(|&v| v != *s);
        }
    }
}

/// The worker: each set in turn, skipping what the frame loop made
/// meanwhile, until the renderer is gone.
fn work(job: Job, tx: Sender<Warmed>, done: Arc<AtomicU32>) {
    let t0 = Instant::now();
    let (mut built, mut skipped, mut pipelines) = (0u32, 0u32, 0u64);
    let made_before = crate::pipeline_cache::made();
    let mut deliver = |kind: Kind, slot: usize, make: &mut dyn FnMut() -> Warmed| -> bool {
        if done.load(Ordering::Relaxed) & bit(kind, slot) != 0 {
            skipped += 1;
            return true;
        }
        // (pipeline_cache::save waits for the set: one set at a time)
        let building = crate::pipeline_cache::building();
        let item = make();
        drop(building);
        done.fetch_or(bit(kind, slot), Ordering::Relaxed);
        built += 1;
        tx.send(item).is_ok()
    };
    let d = &job.device;
    let q = &job.queue;
    for &s in &job.viewport {
        if !deliver(Kind::Viewport, sample_slot(s), &mut || {
            Warmed::Viewport(s, ViewportPipelines::new(d, q, &job.globals, &job.tex, &job.glass, &job.terrain, s, job.reduced))
        }) {
            return;
        }
    }
    // The first frame's post targets want the depth passes at its sample
    // count; the world GUI and highlights at that count come next (their
    // other slots are dropped as a frame draws at another count, so only
    // the one the session starts with is worth making ahead).
    for &s in &job.passes {
        let slot = usize::from(s > 1);
        let (pl, hdr) = &job.postfx_depth[slot];
        if !deliver(Kind::PostfxDepth, slot, &mut || Warmed::PostfxDepth(s, crate::postfx::build_depth_pipes(d, pl, *hdr, s))) {
            return;
        }
    }
    if let Some(&s) = job.passes.first() {
        if !deliver(Kind::WorldGui, sample_slot(s), &mut || Warmed::WorldGui(s, crate::world_gui::WorldGuiPass::new(d, q, DISPLAY_FORMAT, s))) {
            return;
        }
        if !deliver(Kind::Highlight, sample_slot(s), &mut || {
            Warmed::Highlight(s, crate::highlight::HighlightPass::new(d, DISPLAY_FORMAT, DEPTH_FORMAT, s, job.vertex_stride))
        }) {
            return;
        }
    }
    for &s in &job.passes {
        if !deliver(Kind::Clouds, sample_slot(s), &mut || {
            Warmed::Clouds(s, crate::clouds::CloudsGpu::new(d, q, &job.clouds_globals, &job.clouds_source, HDR_FORMAT, DEPTH_FORMAT, s))
        }) {
            return;
        }
        if !deliver(Kind::Adorn, sample_slot(s), &mut || Warmed::Adorn(s, crate::adorn::AdornPass::new(d, q, DISPLAY_FORMAT, s))) {
            return;
        }
    }
    if !job.passes.is_empty() && !deliver(Kind::VfBlit, 0, &mut || Warmed::VfBlit(super::vf::Blit::new(d))) {
        return;
    }
    pipelines += crate::pipeline_cache::made().saturating_sub(made_before);
    let line = format!(
        "shader warm-up: {built} pass set(s) built on the worker in {} ms ({skipped} made by a frame first; {pipelines} pipeline(s) through the cache; viewport sets at {:?} sample(s) a pixel)",
        t0.elapsed().as_millis(),
        job.viewport
    );
    log::info!("{line}");
    if rustudio_core::load_trace::startup_trace() {
        eprintln!("[startup] {line}");
    }
    if let Ok(mut s) = SUMMARY.lock() {
        *s = Some(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_are_distinct() {
        let kinds = [Kind::Viewport, Kind::WorldGui, Kind::Highlight, Kind::Adorn, Kind::Clouds, Kind::PostfxDepth, Kind::VfBlit];
        let mut seen = std::collections::HashSet::new();
        for k in kinds {
            for slot in 0..3 {
                assert!(seen.insert(bit(k, slot)), "{k:?} {slot}");
            }
        }
    }
}

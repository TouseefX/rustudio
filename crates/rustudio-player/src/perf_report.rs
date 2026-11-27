//! A frame report every few seconds, one line to the log: how the frames
//! of the last window went and where their time was — the phone's
//! "where is it slow" that otherwise takes a profiler capture to answer
//! (a logcat line a user can paste). `RUSTUDIO_PERF_LOG=1` turns it on
//! (a phone has it on by default: src/android), `=0` off.
//!
//! Each line: the frames' spacing (mean, p95, worst) and how much of it
//! the CPU spent (eframe's measure of the whole frame; the app's own
//! update inside it; what the frame limit slept), the GPU's time per
//! pass (gpu_timing.rs spans, when the adapter has timestamp queries),
//! the 3D view's level, size and samples, pipelines made in the window,
//! the heaviest sections (bench.rs scopes, ms a frame) and what asked for
//! the frames (egui's repaint causes: a widget animating, an effect, the
//! game).
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Seconds a window covers.
pub const WINDOW_S: f32 = 5.0;

/// The last line, for a settings panel to show (and copy) without a log.
static LAST: Mutex<Option<String>> = Mutex::new(None);

pub fn last_line() -> Option<String> {
    LAST.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[derive(Default)]
pub struct PerfReport {
    started: Option<Instant>,
    last_frame: Option<Instant>,
    /// Frame spacings in the window (ms).
    gaps: Vec<f32>,
    /// Frames and their CPU sums (ms): eframe's whole frame, the app's
    /// update, the limit's sleep.
    frames: u32,
    cpu_ms: f64,
    app_ms: f64,
    slept_ms: f64,
    /// Section totals at the window's start (`bench::sections_snapshot`).
    sections_at: HashMap<&'static str, f64>,
    pipelines_at: u64,
    /// Frames asked by each egui repaint cause (file:line).
    causes: HashMap<String, u32>,
    /// The last window's line.
    pub last: Option<String>,
}

/// One frame's figures, handed in after egui ran.
pub struct FrameIn<'a> {
    /// eframe's CPU time of the previous frame (seconds), or None.
    pub cpu_s: Option<f32>,
    /// The app's own update this frame (seconds).
    pub app_s: f32,
    /// The frame limit's sleep before this frame (seconds).
    pub slept_s: f32,
    /// egui's repaint causes for this frame.
    pub causes: Vec<egui::RepaintCause>,
    /// The 3D view (`Game::perf_view`).
    pub view: (Option<u8>, Option<([u32; 2], u32)>),
    /// The GPU adapter's name, for the first line.
    pub adapter: &'a str,
}

impl PerfReport {
    /// Records a frame; returns the window's line when one closes.
    pub fn frame(&mut self, f: FrameIn) -> Option<String> {
        let now = Instant::now();
        let started = *self.started.get_or_insert_with(|| {
            self.sections_at = rustudio_core::bench::sections_snapshot().into_iter().map(|(n, ms, _)| (n, ms)).collect();
            self.pipelines_at = rustudio_render::pipeline_cache::made();
            rustudio_render::gpu_timing::set_detailed(true);
            // (drop what accumulated before the window)
            let _ = rustudio_render::gpu_timing::take();
            now
        });
        if let Some(last) = self.last_frame.replace(now) {
            self.gaps.push(now.duration_since(last).as_secs_f32() * 1000.0);
        }
        self.frames += 1;
        self.cpu_ms += f64::from(f.cpu_s.unwrap_or(0.0)) * 1000.0;
        self.app_ms += f64::from(f.app_s) * 1000.0;
        self.slept_ms += f64::from(f.slept_s) * 1000.0;
        for c in &f.causes {
            let key = format!("{}:{}", c.file.rsplit(['/', '\\']).next().unwrap_or(c.file), c.line);
            *self.causes.entry(key).or_default() += 1;
        }
        let elapsed = now.duration_since(started).as_secs_f32();
        if elapsed < WINDOW_S {
            return None;
        }
        let line = self.close(elapsed, &f);
        self.started = Some(now);
        self.gaps.clear();
        self.frames = 0;
        self.cpu_ms = 0.0;
        self.app_ms = 0.0;
        self.slept_ms = 0.0;
        self.causes.clear();
        self.sections_at = rustudio_core::bench::sections_snapshot().into_iter().map(|(n, ms, _)| (n, ms)).collect();
        self.pipelines_at = rustudio_render::pipeline_cache::made();
        self.last = Some(line.clone());
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some(line.clone());
        Some(line)
    }

    fn close(&mut self, elapsed: f32, f: &FrameIn) -> String {
        use std::fmt::Write;
        let n = self.frames.max(1) as f64;
        let mut s = String::new();
        // Frames: spacing.
        let mut gaps = std::mem::take(&mut self.gaps);
        gaps.sort_by(|a, b| a.total_cmp(b));
        let mean = if gaps.is_empty() { 0.0 } else { gaps.iter().sum::<f32>() / gaps.len() as f32 };
        let p95 = gaps.get((gaps.len() as f32 * 0.95) as usize).or(gaps.last()).copied().unwrap_or(0.0);
        let worst = gaps.last().copied().unwrap_or(0.0);
        let _ = write!(s, "perf: {} frames in {elapsed:.1} s: {mean:.1} ms apart (p95 {p95:.1}, worst {worst:.0})", self.frames);
        // CPU: eframe's frame, the app inside it.
        let cpu = self.cpu_ms / n;
        let app = self.app_ms / n;
        let _ = write!(s, "; CPU {cpu:.1} ms a frame = app {app:.1} + egui/present {:.1}", (cpu - app).max(0.0));
        if self.slept_ms > 0.0 {
            let _ = write!(s, ", slept {:.1}", self.slept_ms / n);
        }
        // GPU spans.
        let (labels, totals) = rustudio_render::gpu_timing::take();
        if !totals.is_empty() {
            let frames = totals.len() as f64;
            let total: f64 = totals.iter().sum::<f64>() / frames;
            let _ = write!(s, "; GPU {total:.1} ms a drawn frame ({} measured) =", totals.len());
            let mut labels: Vec<_> = labels.into_iter().filter(|(_, ms)| *ms / frames >= 0.05).collect();
            labels.sort_by(|a, b| b.1.total_cmp(&a.1));
            for (i, (label, ms)) in labels.iter().take(8).enumerate() {
                let _ = write!(s, "{} {} {:.1}", if i == 0 { "" } else { " +" }, label.trim_start_matches("gpu: "), ms / frames);
            }
        } else {
            s.push_str("; GPU not timed");
        }
        // The 3D view.
        match f.view {
            (level, Some((size, samples))) => {
                let _ = write!(s, "; 3D view {}x{} px", size[0], size[1]);
                match level {
                    Some(l) => { let _ = write!(s, " at level {l}"); }
                    None => s.push_str(" (manual quality)"),
                }
                if samples > 1 {
                    let _ = write!(s, ", MSAA {samples}x");
                }
            }
            _ => s.push_str("; no 3D view"),
        }
        let made = rustudio_render::pipeline_cache::made().saturating_sub(self.pipelines_at);
        if made > 0 {
            let _ = write!(s, "; {made} pipeline(s) built");
        }
        // Sections (ms a frame), the heaviest first.
        let mut secs: Vec<(&str, f64)> = rustudio_core::bench::sections_snapshot()
            .into_iter()
            .map(|(name, ms, _)| (name, (ms - self.sections_at.get(name).copied().unwrap_or(0.0)) / n))
            .filter(|(_, ms)| *ms >= 0.3)
            .collect();
        secs.sort_by(|a, b| b.1.total_cmp(&a.1));
        if !secs.is_empty() {
            s.push_str("; sections (ms a frame):");
            for (i, (name, ms)) in secs.iter().take(10).enumerate() {
                let _ = write!(s, "{} {name} {ms:.1}", if i == 0 { "" } else { "," });
            }
        }
        // Who asked for the frames.
        let mut causes: Vec<(&String, &u32)> = self.causes.iter().collect();
        causes.sort_by(|a, b| b.1.cmp(a.1));
        if !causes.is_empty() {
            s.push_str("; frames asked by:");
            for (i, (key, count)) in causes.iter().take(4).enumerate() {
                let _ = write!(s, "{} {key} x{count}", if i == 0 { "" } else { "," });
            }
        }
        if !f.adapter.is_empty() && self.last.is_none() {
            let _ = write!(s, "; GPU {}", f.adapter);
        }
        s
    }
}

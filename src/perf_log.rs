//! A line about the frames every few seconds (`log::info!`, so logcat on
//! Android: `adb logcat -s rustudio`; RUSTUDIO_PERF_LOG=1 elsewhere): how
//! far apart the frames came, how much of that was the editor's own work
//! on the main thread, how much eframe's (egui's layout and painting), and
//! what the rest waited for (the GPU, vsync) — with the editor's sections
//! (src/bench.rs) that took the most. The way to tell a GPU-bound frame
//! from a CPU-bound one on a phone, where there's no profiler at hand.
//!
//! An idle editor logs nothing: no input, no frames.

use std::time::{Duration, Instant};

const PERIOD: Duration = Duration::from_secs(5);

/// logcat on Android (the `log` crate is an Android dependency), stderr
/// elsewhere.
fn say(line: String) {
    #[cfg(target_os = "android")]
    log::info!("{line}");
    #[cfg(not(target_os = "android"))]
    eprintln!("{line}");
}

#[derive(Default)]
pub struct PerfLog {
    since: Option<Instant>,
    last_frame: Option<Instant>,
    /// Frame intervals (ms), the editor's own work in each and eframe's
    /// whole CPU time for each (ms).
    intervals: Vec<f32>,
    work: Vec<f32>,
    cpu: Vec<f32>,
    /// (ms, calls) per section over the period.
    sections: Vec<(&'static str, f64, u64)>,
    said_device: bool,
}

/// Whether the log is wanted: always on a phone, by request elsewhere.
pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| cfg!(target_os = "android") || std::env::var_os("RUSTUDIO_PERF_LOG").is_some())
}

impl PerfLog {
    /// Once per frame, at its end: `work_ms` the editor's own time this
    /// frame, `cpu_ms` eframe's for the previous one (`Frame::info().cpu_usage`),
    /// `extra` the line's tail (view sizes, counters), `device` the GPU
    /// (said once).
    pub fn frame(&mut self, work_ms: f32, cpu_ms: Option<f32>, extra: impl FnOnce() -> String, device: impl FnOnce() -> Option<String>) {
        let now = Instant::now();
        let since = *self.since.get_or_insert(now);
        if let Some(last) = self.last_frame {
            self.intervals.push(now.duration_since(last).as_secs_f32() * 1000.0);
            self.work.push(work_ms);
            if let Some(c) = cpu_ms {
                self.cpu.push(c * 1000.0);
            }
        }
        self.last_frame = Some(now);
        // A frame's sections are gathered whether or not a bench is on
        // (`--bench` takes them per frame itself; the two don't run together).
        for (name, ms, calls) in crate::bench::drain_sections() {
            match self.sections.iter_mut().find(|e| e.0 == name) {
                Some(e) => {
                    e.1 += ms;
                    e.2 += calls;
                }
                None => self.sections.push((name, ms, calls)),
            }
        }
        let elapsed = now.duration_since(since);
        if elapsed < PERIOD {
            return;
        }
        if !self.said_device {
            self.said_device = true;
            if let Some(d) = device() {
                say(format!("perf: {d}"));
            }
        }
        let n = self.intervals.len();
        if n == 0 {
            self.since = Some(now);
            return;
        }
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
        let mut sorted = self.intervals.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let p95 = sorted[(sorted.len() * 95 / 100).min(sorted.len() - 1)];
        let worst = sorted[sorted.len() - 1];
        let interval = mean(&self.intervals);
        let work = mean(&self.work);
        let cpu = if self.cpu.is_empty() { work } else { mean(&self.cpu) };
        let wait = (interval - cpu).max(0.0);
        let mut top: Vec<(&'static str, f64)> = self.sections.iter().map(|(name, ms, _)| (*name, *ms / n as f64)).collect();
        top.sort_by(|a, b| b.1.total_cmp(&a.1));
        let top: Vec<String> = top.iter().take(8).filter(|(_, ms)| *ms >= 0.05).map(|(name, ms)| format!("{name} {ms:.1}")).collect();
        say(format!(
            "perf: {n} frames in {:.1} s: {interval:.1} ms apart (p95 {p95:.1}, worst {worst:.0}); editor {work:.1} ms + egui {:.1} ms on the CPU, {wait:.1} ms GPU/vsync wait; {}; sections (ms/frame): {}",
            elapsed.as_secs_f32(),
            (cpu - work).max(0.0),
            extra(),
            if top.is_empty() { "-".to_string() } else { top.join(", ") },
        ));
        self.since = Some(now);
        self.intervals.clear();
        self.work.clear();
        self.cpu.clear();
        self.sections.clear();
    }
}

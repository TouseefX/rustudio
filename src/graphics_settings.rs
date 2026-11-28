//! View > GPU Driver: which graphics adapter and driver this run draws
//! with, whether weak-GPU mode is on (gpu_caps.rs), and, on Android, the
//! switch for the Mesa Vulkan driver the APK ships for the device — Turnip
//! on Snapdragon, PanVK on Mali (src/android/turnip.rs) — with Turnip's
//! debug switches. The 3D view's quality itself (Automatic or manual
//! quality, 3D resolution, shading detail, shadows, ambient occlusion,
//! bloom, the frame limit) is File > Studio Settings > rustudio >
//! Performance (crates/rustudio-core/src/rustudio_settings.rs). Built for
//! a thumb: check boxes and buttons, nothing to drag.

use egui::RichText;

use crate::app::Editor;

impl Editor {
    /// The window, while View > GPU Driver has it open.
    pub fn graphics_settings_ui(&mut self, ctx: &egui::Context) {
        if !self.ui.graphics_settings {
            return;
        }
        let mut open = true;
        let screen = ctx.content_rect();
        egui::Window::new("GPU Driver")
            .id(egui::Id::new("graphics_settings_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(380.0)
            .default_pos(screen.center() - egui::vec2(200.0, (screen.height() * 0.45).min(320.0)))
            // (a phone's screen is shorter than the rows: the rest scrolls)
            .vscroll(true)
            .show(ctx, |ui| self.graphics_settings_body(ui));
        self.ui.graphics_settings = open;
    }

    fn graphics_settings_body(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().interact_size.y = ui.spacing().interact_size.y.max(28.0);
        match self.game.render_state.as_ref() {
            Some(rs) => {
                let i = rs.adapter.get_info();
                ui.label(RichText::new(&i.name).strong());
                ui.small(format!("{:?}, {:?}; driver {} {}", i.backend, i.device_type, i.driver, i.driver_info).trim_end().to_string());
                ui.small(format!("Surface {:?}", rs.target_format));
            }
            None => {
                ui.label(RichText::new("No GPU renderer").strong());
            }
        }
        if rustudio_core::rustudio_settings::weak_gpu() {
            ui.small("Weak-GPU mode: Automatic quality starts at its lowest level and climbs as frame times allow (RUSTUDIO_LOWEND=0 turns it off).");
        }
        ui.small(RichText::new("Graphics quality, 3D resolution, shading detail, shadows and the frame limit: File > Studio Settings > rustudio > Performance.").weak());
        #[cfg(target_os = "android")]
        gpu_driver_section(ui);
        #[cfg(target_os = "android")]
        shader_section(ui);
        frame_report_section(ui);
    }
}

/// The last frame report (perf_report.rs: the same line that goes to the
/// log every few seconds), to read on the phone and to copy.
fn frame_report_section(ui: &mut egui::Ui) {
    let Some(line) = rustudio_player::perf_report::last_line() else { return };
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Frame report").strong());
        if ui.small_button("Copy").on_hover_text("Copies the line below (also in the log every few seconds).").clicked() {
            ui.ctx().copy_text(line.clone());
        }
    });
    ui.small(RichText::new("The last few seconds: frame spacing, CPU and GPU time per frame and where it went, the 3D view's level and size, and what asked for the frames.").weak());
    ui.add(egui::Label::new(RichText::new(&line).small()).wrap());
}

/// Android: which Vulkan driver this run has, and the switch for the
/// Mesa driver the APK ships for the device — Turnip on Snapdragon, PanVK
/// on Mali (src/android/turnip.rs; takes effect at the next start).
#[cfg(target_os = "android")]
fn gpu_driver_section(ui: &mut egui::Ui) {
    use crate::android::turnip;
    ui.separator();
    ui.label(RichText::new("Vulkan driver").strong());
    let st = turnip::status();
    if st.device.is_empty() {
        ui.small(format!("This run: the device's driver ({}).", st.note));
    } else {
        ui.small(format!("This run: {} — {}.", st.device, st.driver));
    }
    let renderer = turnip::renderer();
    if !renderer.is_empty() {
        ui.small(format!("Renderer on: {renderer}."));
    }
    if !(st.available || st.active) {
        return;
    }
    let mesa = st.mesa.unwrap_or(turnip::Mesa::Turnip);
    let mut on = turnip::wanted();
    if ui
        .checkbox(&mut on, format!("{} (Mesa) Vulkan driver", mesa.name()))
        .on_hover_text(format!("Mesa's open-source {} driver in place of the phone's, from the next start on. Off by default: it draws faster on many phones and crashes on some; try it, and untick it if the app stops starting or closes in its first minutes.", mesa.gpus()))
        .changed()
    {
        turnip::set_wanted(on);
    }
    if !on && !st.active {
        ui.small(RichText::new(format!("Off by default. {} is faster on many {} phones and crashes on some; worth a try if the 3D view is slow.", mesa.name(), mesa.gpus())).weak());
    }
    if turnip::failed_last_time() {
        ui.horizontal_wrapped(|ui| {
            ui.small(RichText::new("Off since a start with it didn't reach the first frame.").weak());
            if ui.small_button("Try again").clicked() {
                turnip::retry();
            }
        });
    } else if on != st.active {
        ui.small(RichText::new("Takes effect at the next start.").weak());
    }
    // (TU_DEBUG is Turnip's; PanVK has switches of its own, PAN_MESA_DEBUG,
    // none of which the window offers yet)
    if st.active && mesa == turnip::Mesa::Turnip {
        turnip_switches(ui);
    }
}

/// Android: the shaders are compiled on this device (nothing comes
/// precompiled), what the startup warm-up and the driver's caches did
/// this run, and the switch that has the next start compile everything
/// afresh (src/android/shader_cache.rs).
#[cfg(target_os = "android")]
fn shader_section(ui: &mut egui::Ui) {
    use crate::android::shader_cache;
    ui.separator();
    ui.label(RichText::new("Shaders").strong());
    ui.small(RichText::new("Every shader is compiled on this phone by its driver: at the start, on a worker thread (the warm-up), and at a pass's first use. Nothing comes precompiled from a desktop; the driver's caches below are this phone's own, kept between runs.").weak());
    ui.small(rustudio_render::render::shader_warmup_summary().unwrap_or_else(|| "Shader warm-up: off this run (RUSTUDIO_SHADER_WARMUP=0, or an OpenGL adapter).".to_string()));
    ui.small(rustudio_render::pipeline_cache::summary());
    if let Some(note) = shader_cache::note() {
        ui.small(note);
    }
    let mut rebuild = shader_cache::rebuild_wanted();
    if ui
        .checkbox(&mut rebuild, "Rebuild shader caches at the next start")
        .on_hover_text("Deletes the pipeline cache and Mesa's shader cache, so the next start compiles every shader afresh on this phone (a slower first frame once; the caches fill again). For after a driver update, or when frames stutter where they didn't.")
        .changed()
    {
        shader_cache::set_rebuild(rebuild);
    }
    if rebuild {
        ui.small(RichText::new("Takes effect at the next start.").weak());
    }
}

/// Turnip's own switches (`TU_DEBUG` options), for when something draws
/// wrong with it: the live ones change the running driver as the box is
/// ticked (Mesa watches the file they are kept in), the others at the
/// next start. The choice stays until "Default".
#[cfg(target_os = "android")]
fn turnip_switches(ui: &mut egui::Ui) {
    use crate::android::turnip;
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Turnip switches").strong());
        if !turnip::debug_flags_default() && ui.small_button("Default").clicked() {
            turnip::reset_debug_flags();
        }
    });
    ui.small(RichText::new("LRZ is off by default (Mesa 26.3.0 lost terrain chunks with it on an Adreno 650); on an Adreno 6xx bandwidth compression is off too (with it, Turnip's first frame restarted a Snapdragon 865 phone outright). For other missing or flickering geometry: try the rest top down, one at a time; the ones marked take effect at the next start.").weak());
    let flags = turnip::debug_flags();
    for s in turnip::SWITCHES {
        let mut on = flags.iter().any(|f| f == s.flag);
        let label = if s.live { s.name.to_string() } else { format!("{} (next start)", s.name) };
        if ui.checkbox(&mut on, label).on_hover_text(format!("{}\nTU_DEBUG={}", s.hint, s.flag)).changed() {
            turnip::set_debug_flag(s.flag, on);
        }
    }
}

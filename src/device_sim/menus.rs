//! The bar's menus and popovers: the device dropdown, Display scaling, and
//! the network and performance popovers (shown greyed: Rustudio simulates
//! neither) and the touch controls' help.

use egui::{pos2, vec2, Align2, FontId, Rect, RichText, Sense};
use rustudio_core::devices::{self, DeviceRef};
use rustudio_core::mech;
use rustudio_engine::theme;
use rustudio_player::device_view::Scaling;

use super::{DeviceSim, Popup};

/// Shows the open popup under its bar button. Returns the action picked
/// and whether it should close (a pick, or a press outside it).
pub fn show(ctx: &egui::Context, sim: &DeviceSim, anchor: Rect, max_height: f32) -> (Option<String>, bool) {
    let Some(which) = sim.popup else { return (None, false) };
    let right_aligned = !matches!(which, Popup::Devices);
    let pivot = if right_aligned { Align2::RIGHT_TOP } else { Align2::LEFT_TOP };
    let at = if right_aligned { anchor.right_bottom() + vec2(0.0, 4.0) } else { anchor.left_bottom() + vec2(0.0, 4.0) };
    let mut picked = None;
    let area = egui::Area::new(egui::Id::new(("devsim_popup", which as u8)))
        .order(egui::Order::Foreground)
        .pivot(pivot)
        .fixed_pos(at)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(theme::BG)
                .corner_radius(6.0)
                .inner_margin(egui::Margin::same(4))
                .show(ui, |ui| match which {
                    Popup::Devices => picked = devices_menu(ui, sim, max_height),
                    Popup::Scaling => picked = scaling_menu(ui, sim),
                    Popup::Lan => lan_popover(ui),
                    Popup::Quality => quality_popover(ui, sim),
                    Popup::Touch => touch_popover(ui),
                });
        });
    let outside = ctx.input(|i| {
        i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p) && !anchor.contains(p))
    });
    let close = picked.is_some() || outside || ctx.input(|i| i.key_pressed(egui::Key::Escape));
    (picked, close)
}

/// One menu row; `check` draws Studio's tick in the left column.
fn row(ui: &mut egui::Ui, width: f32, text: &str, check: bool, chip: Option<&str>, enabled: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(width, mech::DEVICE_MENU_ROW), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if enabled && resp.hovered() {
        p.rect_filled(r, 4.0, theme::HOVER_BG);
    }
    let col = if enabled { theme::TEXT } else { theme::TEXT_DIM };
    let font = FontId::proportional(12.0);
    if check {
        rustudio_engine::ti::paint(p, Rect::from_center_size(pos2(r.min.x + 8.0, r.center().y), vec2(10.0, 10.0)), "check", col);
    }
    let left = r.min.x + 16.0;
    let mut right = r.max.x - 4.0;
    if let Some(c) = chip {
        let g = p.layout_no_wrap(c.to_string(), FontId::proportional(11.0), theme::TEXT);
        let cr = Rect::from_min_size(pos2(right - g.size().x - 8.0, r.center().y - 7.5), vec2(g.size().x + 8.0, 15.0));
        p.rect_filled(cr, 3.0, theme::DEVICE_CHIP_BG);
        p.galley(pos2(cr.min.x + 4.0, cr.center().y - g.size().y / 2.0), g, theme::TEXT);
        right = cr.min.x - 6.0;
    }
    // names past the column end in an ellipsis (Studio)
    let mut job = egui::text::LayoutJob::single_section(text.to_string(), egui::TextFormat::simple(font, col));
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - left).max(10.0));
    let g = p.layout_job(job);
    p.galley(pos2(left, r.center().y - g.size().y / 2.0), g, col);
    resp
}

fn header(ui: &mut egui::Ui, width: f32, text: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(width, mech::DEVICE_MENU_ROW), Sense::hover());
    ui.painter().text(pos2(r.min.x + 16.0, r.center().y), Align2::LEFT_CENTER, text, FontId::proportional(11.0), theme::TEXT_DIM);
}

fn divider(ui: &mut egui::Ui, width: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(width, 7.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, egui::Stroke::new(1.0, theme::DIVIDER));
}

/// The device dropdown: the current form's devices by group, each with its
/// size, the current one ticked, then "Manage devices".
fn devices_menu(ui: &mut egui::Ui, sim: &DeviceSim, max_height: f32) -> Option<String> {
    let w = mech::DEVICE_MENU_WIDTH;
    let mut picked = None;
    let groups = devices::dropdown(sim.form(), &visible_custom(sim));
    egui::ScrollArea::vertical().max_height(max_height.max(60.0)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (i, g) in groups.iter().enumerate() {
            if i > 0 {
                divider(ui, w);
            }
            header(ui, w, g.title);
            for item in &g.items {
                match item {
                    DeviceRef::Current => {
                        if row(ui, w, "Current device", sim.device.is_none(), None, true).clicked() {
                            picked = Some("device:none".to_string());
                        }
                    }
                    DeviceRef::Device(d) => {
                        if sim.custom.hidden_standard.contains(&d.id) {
                            continue;
                        }
                        let size = format!("{} \u{00d7} {}", d.size[0], d.size[1]);
                        if row(ui, w, &d.name, sim.device.as_deref() == Some(&d.id), Some(&size), true).clicked() {
                            picked = Some(format!("device:{}", d.id));
                        }
                    }
                }
            }
        }
        divider(ui, w);
        if row(ui, w, "Manage devices", false, None, true).clicked() {
            picked = Some("device_manage".to_string());
        }
    });
    picked
}

fn visible_custom(sim: &DeviceSim) -> Vec<devices::Device> {
    sim.custom.devices.clone()
}

/// Display scaling: the three modes, ticked, then Calibrate.
fn scaling_menu(ui: &mut egui::Ui, sim: &DeviceSim) -> Option<String> {
    let w = 160.0;
    let mut picked = None;
    ui.spacing_mut().item_spacing.y = 0.0;
    for (mode, label, key) in [
        (Scaling::PhysicalSize, "Physical size", "physical"),
        (Scaling::ActualResolution, "Actual resolution", "actual"),
        (Scaling::FitToWindow, "Fit to window", "fit"),
    ] {
        if row(ui, w, label, sim.scaling == mode, None, true).clicked() {
            picked = Some(format!("device_scaling:{key}"));
        }
    }
    divider(ui, w);
    if row(ui, w, "Calibrate physical scaling", false, None, true).clicked() {
        picked = Some("device_calibrate".to_string());
    }
    picked
}

fn combo(ui: &mut egui::Ui, label: &str, value: &str, options: &[&str]) {
    ui.horizontal(|ui| {
        label_cell(ui, 130.0, RichText::new(label).size(12.0).color(theme::TEXT_DIM));
        ui.add_enabled_ui(false, |ui| {
            egui::ComboBox::from_id_salt(("devsim_combo", label)).selected_text(value).width(110.0).show_ui(ui, |ui| {
                for o in options {
                    let _ = ui.selectable_label(*o == value, *o);
                }
            });
        });
    });
}

/// A popover's left-aligned label column (Studio's).
fn label_cell(ui: &mut egui::Ui, w: f32, text: RichText) {
    ui.allocate_ui_with_layout(vec2(w, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_width(w);
        ui.label(text);
    });
}

fn unit_field(ui: &mut egui::Ui, label: &str, value: &str, unit: &str) {
    ui.horizontal(|ui| {
        label_cell(ui, 100.0, RichText::new(label).size(12.0).color(theme::TEXT_DIM));
        let mut v = value.to_string();
        ui.add_enabled(false, egui::TextEdit::singleline(&mut v).desired_width(130.0));
        ui.label(RichText::new(unit).size(12.0).color(theme::TEXT_DIM));
    });
}

fn loss_row(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        label_cell(ui, 100.0, RichText::new("Packet loss").size(12.0).color(theme::TEXT_DIM));
        let mut v = 0.0f32;
        ui.add_enabled(false, egui::Slider::new(&mut v, 0.0..=100.0).show_value(false));
        ui.label(RichText::new("0.00%").size(12.0).color(theme::TEXT_DIM));
    });
}

/// A section of a popover with Studio's open triangle.
fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).size(12.0)).id_salt(("devsim_section", title)).default_open(true).show(ui, body);
}

/// Network simulation (Studio's popover, every control greyed: Rustudio
/// doesn't simulate the network).
fn lan_popover(ui: &mut egui::Ui) {
    ui.set_width(290.0);
    ui.label(RichText::new("Network simulation").size(14.0));
    ui.add_space(4.0);
    combo(ui, "Preset", "Current Device", &["Current Device"]);
    for side in ["Inbound", "Outbound"] {
        section(ui, side, |ui| {
            unit_field(ui, "Latency", "0", "ms");
            loss_row(ui);
            unit_field(ui, "Jitter", "0", "ms");
        });
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_enabled_ui(false, |ui| {
            let _ = ui.button("Apply");
            let _ = ui.button("Save");
            let _ = ui.button("Reset");
        });
    });
}

/// Performance simulation (Studio's popover, every control greyed:
/// Rustudio doesn't simulate graphics quality or memory). The quality is
/// the device's own level; Device RAM shows 4 GB while memory isn't
/// simulated, as Studio's did for an iPhone 14 (6 GB; estimate: Studio's
/// default).
fn quality_popover(ui: &mut egui::Ui, sim: &DeviceSim) {
    ui.set_width(290.0);
    let d = sim.current();
    let q = d.map_or("Automatic".to_string(), |d| d.quality.to_string());
    ui.label(RichText::new("Performance simulation").size(14.0));
    ui.add_space(4.0);
    section(ui, "Graphics", |ui| {
        combo(ui, "Mode", if d.is_some() { "Selected device" } else { "Not simulated" }, &["Not simulated", "Selected device", "Manual"]);
        combo(ui, "Edit quality", &q, &[q.as_str()]);
        combo(ui, "Play quality", &q, &[q.as_str()]);
    });
    section(ui, "Memory", |ui| {
        combo(ui, "Mode", "Not simulated", &["Not simulated", "Selected device", "Manual"]);
        combo(ui, "Device RAM", "4 GB", &["4 GB"]);
        combo(ui, "Background", "Low", &["Low", "Mid", "High"]);
    });
}

/// The touch controls' help (Studio's text).
fn touch_popover(ui: &mut egui::Ui) {
    ui.set_width(230.0);
    ui.label(RichText::new("While playtesting:").size(12.0));
    ui.label(RichText::new("Pinch + rotate: LeftAlt + Drag").size(12.0).color(theme::TEXT_DIM));
    ui.label(RichText::new("Two-finger pan: LeftAlt + LeftShift + Drag").size(12.0).color(theme::TEXT_DIM));
}

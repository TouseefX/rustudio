//! Calibrate physical scaling (Studio's dialog): the person sizes a picture
//! of an ID card to a real one, which gives the screen's pixels per inch for
//! Display scaling > Physical size.

use egui::{pos2, vec2, Color32, Rect, RichText, Stroke};
use rustudio_core::mech;
use rustudio_engine::theme;

/// Studio's text (its template with the card's metric and imperial widths).
pub const TEXT: &str = "To accurately scale to physical size, we need to estimate your screen size. \
Adjust the slider until the image below is about the size of a student ID or library card (8.56 cm or 3.37 in wide).";

/// The card's width in screen pixels at a DPI.
pub fn card_px(dpi: f32) -> f32 {
    mech::DEVICE_CARD_INCHES[0] * dpi
}

/// The slider's position (0 to 1) for a DPI, and back: linear along the
/// track (measured).
pub fn slider_t(dpi: f32) -> f32 {
    let (lo, hi, _) = mech::DEVICE_CALIBRATE_DPI;
    ((dpi - lo) / (hi - lo)).clamp(0.0, 1.0)
}

pub fn slider_dpi(t: f32) -> f32 {
    let (lo, hi, _) = mech::DEVICE_CALIBRATE_DPI;
    lo + t.clamp(0.0, 1.0) * (hi - lo)
}

pub enum Outcome {
    Open,
    Accept(f32),
    Cancel,
}

/// The dialog, while `dpi` (the slider's value) is Some.
pub fn window(ctx: &egui::Context, dpi: &mut f32) -> Outcome {
    let mut out = Outcome::Open;
    let mut open = true;
    let ppp = ctx.pixels_per_point();
    egui::Window::new("Calibrate physical scaling")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .default_width(460.0)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(RichText::new(TEXT).size(12.5));
            ui.add_space(10.0);
            // the card between fixed dashed guides, in real screen pixels
            let (lo, hi, _) = mech::DEVICE_CALIBRATE_DPI;
            let max_w = card_px(hi) / ppp;
            let h = mech::DEVICE_CARD_INCHES[1] / mech::DEVICE_CARD_INCHES[0] * max_w;
            let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), h.min(260.0) + 20.0), egui::Sense::hover());
            let w = card_px(*dpi) / ppp;
            let ch = w * mech::DEVICE_CARD_INCHES[1] / mech::DEVICE_CARD_INCHES[0];
            let card = Rect::from_min_size(pos2(area.min.x + 20.0, area.center().y - ch / 2.0), vec2(w, ch));
            let p = ui.painter_at(area);
            for x in [card.min.x, card.max.x] {
                let mut y = area.min.y;
                while y < area.max.y {
                    p.vline(x, y..=(y + 4.0).min(area.max.y), Stroke::new(1.0, theme::TEXT_DIM));
                    y += 8.0;
                }
            }
            p.rect_filled(card, 8.0 * w / 323.0, Color32::from_rgb(70, 110, 190));
            let photo = Rect::from_min_size(card.min + vec2(w * 0.07, ch * 0.18), vec2(w * 0.26, ch * 0.55));
            p.rect_filled(photo, 4.0, Color32::from_rgb(200, 210, 230));
            for i in 0..3 {
                let y = card.min.y + ch * (0.25 + 0.17 * i as f32);
                p.rect_filled(Rect::from_min_size(pos2(card.min.x + w * 0.42, y), vec2(w * (0.45 - 0.08 * i as f32), ch * 0.07)), 2.0, Color32::from_rgb(200, 210, 230));
            }
            ui.add_space(6.0);
            let mut t = slider_t(*dpi);
            ui.spacing_mut().slider_width = ui.available_width() - 10.0;
            if ui.add(egui::Slider::new(&mut t, 0.0..=1.0).show_value(false)).changed() {
                *dpi = slider_dpi(t);
            }
            let _ = (lo, hi);
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    out = Outcome::Cancel;
                }
                if ui.button("Accept").clicked() {
                    out = Outcome::Accept(*dpi);
                }
            });
        });
    if !open {
        out = Outcome::Cancel;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_width_maps_to_dpi() {
        assert!((card_px(96.0) - 323.5).abs() < 0.1);
        assert!((card_px(80.0) - 269.6).abs() < 0.1);
    }

    #[test]
    fn slider_spans_80_to_250() {
        assert_eq!(slider_dpi(0.0), 80.0);
        assert_eq!(slider_dpi(1.0), 250.0);
        assert!((slider_t(96.0) - 0.0941).abs() < 1e-3);
    }
}

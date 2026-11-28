//! The Controller Emulator panel (Studio's Device Simulator > Virtual
//! controller): the layout dropdown, "Edit mappings" (greyed: the map is
//! fixed) and a gamepad drawing with the keys that drive each part, placed
//! where Studio places them. The drawing is Rustudio's own; the key map is
//! crates/rustudio-player/src/virtual_pad.rs. Headsets: Studio's VR
//! controller panel shows greyed (Rustudio has no VR).

use egui::{pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Shape, Stroke};
use rustudio_core::devices::{Form, FormButton};
use rustudio_engine::theme;

use super::DeviceSim;

/// Studio's two gamepad layouts: the same keys; Xbox adds the guide
/// button and is picked on consoles (estimate: Studio showed it on Xbox One).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PadLayout {
    GenericGamepad,
    Xbox,
}

impl PadLayout {
    pub fn label(self) -> &'static str {
        match self {
            PadLayout::GenericGamepad => "Generic Gamepad",
            PadLayout::Xbox => "Xbox",
        }
    }
}

impl DeviceSim {
    /// The layout the panel shows: the one picked, else the device's own.
    pub fn pad_layout(&self) -> PadLayout {
        self.pad_layout.unwrap_or_else(|| match self.current().map(|d| d.form) {
            Some(Form::Console) => PadLayout::Xbox,
            _ => PadLayout::GenericGamepad,
        })
    }
}

/// Where Studio's panel puts things, in points from the panel's top-left
/// at its default width (measured from ui-virtual-controller.png, 480 wide).
const DESIGN_WIDTH: f32 = 480.0;
const DIAGRAM_TOP: f32 = 40.0;

const LINE: Color32 = Color32::from_rgb(118, 121, 138);
const BODY: Color32 = Color32::from_rgb(176, 178, 186);
const DOT: Color32 = Color32::from_rgb(196, 197, 202);
const LADDER: Color32 = Color32::from_rgb(150, 156, 180);

/// Key labels: (text, centre).
const LABELS: &[(&str, [f32; 2])] = &[
    ("E", [93.0, 93.0]),
    ("R", [93.0, 122.0]),
    ("Q", [96.0, 146.0]),
    ("W", [57.0, 184.0]),
    ("A", [35.0, 206.0]),
    ("F", [57.0, 206.0]),
    ("D", [78.0, 206.0]),
    ("S", [57.0, 227.0]),
    ("1", [62.0, 258.0]),
    ("2", [41.0, 278.0]),
    ("4", [85.0, 278.0]),
    ("3", [62.0, 300.0]),
    ("P", [393.0, 93.0]),
    ("O", [391.0, 122.0]),
    ("U", [390.0, 146.0]),
    ("7", [427.0, 195.0]),
    ("8", [405.0, 216.0]),
    ("0", [448.0, 216.0]),
    ("9", [427.0, 237.0]),
    ("I", [427.0, 259.0]),
    ("J", [405.0, 279.0]),
    ("H", [427.0, 279.0]),
    ("L", [448.0, 279.0]),
    ("K", [427.0, 301.0]),
];

/// Leader lines from a key group to the part it drives (the last point
/// gets a dot).
const LEADERS: &[&[[f32; 2]]] = &[
    &[[105.0, 90.0], [150.0, 90.0], [167.0, 104.0]],   // E: L2
    &[[105.0, 119.0], [157.0, 119.0], [166.0, 124.0]], // R: L1
    &[[108.0, 143.0], [193.0, 143.0], [222.0, 174.0]], // Q: Select
    &[[90.0, 203.0], [163.0, 203.0], [171.0, 183.0]],  // W A S D F: left stick
    &[[97.0, 278.0], [165.0, 278.0], [204.0, 211.0]],  // 1 2 3 4: d-pad
    &[[378.0, 90.0], [332.0, 90.0], [312.0, 104.0]],   // P: R2
    &[[375.0, 119.0], [325.0, 119.0], [316.0, 124.0]], // O: R1
    &[[373.0, 143.0], [290.0, 143.0], [260.0, 174.0]], // U: Start
    &[[392.0, 214.0], [350.0, 214.0], [312.0, 175.0]], // 7 8 9 0: face buttons
    &[[393.0, 279.0], [318.0, 279.0], [277.0, 220.0]], // I J K L H: right stick
];

/// The body's left half, top centre round to bottom centre (mirrored for
/// the right half).
const BODY_LEFT: &[[f32; 2]] = &[
    [240.0, 134.0],
    [206.0, 134.0],
    [196.0, 130.0],
    [172.0, 128.0],
    [152.0, 132.0],
    [143.0, 137.0],
    [131.0, 155.0],
    [119.0, 186.0],
    [109.0, 220.0],
    [104.0, 245.0],
    [106.0, 258.0],
    [116.0, 264.0],
    [131.0, 260.0],
    [150.0, 250.0],
    [172.0, 242.0],
    [205.0, 239.0],
    [240.0, 239.0],
];

/// The left bumper and trigger outlines.
const BUMPER_LEFT: &[[f32; 2]] = &[[143.0, 137.0], [146.0, 126.0], [158.0, 118.0], [180.0, 116.0], [200.0, 120.0], [204.0, 133.0]];
const TRIGGER_LEFT: &[[f32; 2]] = &[[152.0, 120.0], [156.0, 104.0], [162.0, 97.0], [170.0, 96.0], [176.0, 104.0], [182.0, 117.0]];

/// Draws the panel; returns the actions it asks for.
pub fn panel(ui: &mut egui::Ui, sim: &DeviceSim) -> Vec<String> {
    let mut out = Vec::new();
    let full = ui.available_rect_before_wrap();
    let headset = sim.current().is_some_and(|d| d.button() == FormButton::Headset);
    let layout = sim.pad_layout();
    // The top row: layout dropdown, Edit mappings.
    let row = Rect::from_min_size(full.min + vec2(9.0, 7.0), vec2(full.width() - 18.0, 26.0));
    let mut top = ui.new_child(egui::UiBuilder::new().max_rect(row).layout(egui::Layout::left_to_right(egui::Align::Center)));
    egui::ComboBox::from_id_salt("devsim_pad_layout").selected_text(layout.label()).width(186.0).show_ui(&mut top, |ui| {
        for l in [PadLayout::GenericGamepad, PadLayout::Xbox] {
            if ui.selectable_label(l == layout, l.label()).clicked() {
                out.push(format!("device_pad_layout:{}", if l == PadLayout::Xbox { "xbox" } else { "generic" }));
            }
        }
        if headset {
            ui.add_enabled(false, egui::Button::selectable(false, "Quest3"));
        }
    });
    top.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.add_enabled(false, egui::Button::new("Edit mappings"))
            .on_disabled_hover_text("The key map is fixed in Rustudio");
    });
    let sep = full.min.y + 39.0;
    ui.painter().hline(full.x_range(), sep, Stroke::new(1.0, theme::DIVIDER));
    // The gamepad, centred, scaled down when the panel is narrower.
    let k = (full.width() / DESIGN_WIDTH).min(1.0);
    let origin = pos2(full.center().x - DESIGN_WIDTH * k / 2.0, full.min.y + DIAGRAM_TOP);
    let at = |p: [f32; 2]| origin + vec2(p[0], p[1] - DIAGRAM_TOP) * k;
    let p = ui.painter_at(full);
    draw_pad(&p, &at, k, layout);
    for leader in LEADERS {
        let pts: Vec<Pos2> = leader.iter().map(|q| at(*q)).collect();
        p.add(Shape::line(pts.clone(), Stroke::new(1.0, LINE)));
        p.circle_filled(*pts.last().unwrap(), 3.2 * k.max(0.7), DOT);
    }
    let font = FontId::new(15.0 * k.max(0.7), egui::FontFamily::Name("bold".into()));
    for (text, c) in LABELS {
        p.text(at(*c), Align2::CENTER_CENTER, *text, font.clone(), theme::TEXT);
    }
    if headset {
        vr_section(ui, full, at([0.0, 330.0]).y + 10.0);
    }
    ui.allocate_rect(full, egui::Sense::hover());
    out
}

/// Catmull-Rom through the points (open), for the body's curves.
fn smooth(pts: &[Pos2]) -> Vec<Pos2> {
    let mut v = Vec::new();
    for i in 0..pts.len().saturating_sub(1) {
        let p0 = pts[i.saturating_sub(1)];
        let (p1, p2) = (pts[i], pts[i + 1]);
        let p3 = pts[(i + 2).min(pts.len() - 1)];
        for s in 0..8 {
            let t = s as f32 / 8.0;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f32, b: f32, c: f32, d: f32| 0.5 * (2.0 * b + (c - a) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (3.0 * b - a - 3.0 * c + d) * t3);
            v.push(pos2(f(p0.x, p1.x, p2.x, p3.x), f(p0.y, p1.y, p2.y, p3.y)));
        }
    }
    v.extend(pts.last().copied());
    v
}

fn mirror(p: [f32; 2]) -> [f32; 2] {
    [DESIGN_WIDTH - p[0], p[1]]
}

fn draw_pad(p: &egui::Painter, at: &dyn Fn([f32; 2]) -> Pos2, k: f32, layout: PadLayout) {
    let body = Stroke::new(1.0, BODY.gamma_multiply(0.8));
    let thin = Stroke::new(0.8, BODY.gamma_multiply(0.6));
    for half in [false, true] {
        let m = |q: [f32; 2]| at(if half { mirror(q) } else { q });
        let pts: Vec<Pos2> = BODY_LEFT.iter().map(|q| m(*q)).collect();
        p.add(Shape::line(smooth(&pts), body));
        for part in [BUMPER_LEFT, TRIGGER_LEFT] {
            let pts: Vec<Pos2> = part.iter().map(|q| m(*q)).collect();
            p.add(Shape::line(smooth(&pts), thin));
        }
        // The trigger's pressure ladder above it.
        let (cx, base) = (if half { 312.0 } else { 168.0 }, 83.0);
        for i in 0..8 {
            let y = base - i as f32 * 3.6;
            let w = 20.0 - i as f32 * 1.6;
            p.line_segment([at([cx - w / 2.0, y]), at([cx + w / 2.0, y])], Stroke::new(1.2 * k.max(0.7), LADDER));
        }
    }
    // The bar between the bumpers.
    p.line_segment([at([204.0, 120.0]), at([276.0, 120.0])], thin);
    p.line_segment([at([206.0, 133.0]), at([274.0, 133.0])], thin);
    let ring = |c: [f32; 2], r: f32, s: Stroke| p.circle_stroke(at(c), r * k, s);
    // Sticks: an outer ring, the cap a little lower.
    for (outer, cap) in [([173.0, 174.0], [171.0, 183.0]), ([276.0, 212.0], [277.0, 220.0])] {
        ring(outer, 16.0, body);
        ring(cap, 10.5, body);
        ring(cap, 8.0, thin);
    }
    // D-pad.
    let c = [204.0, 211.0];
    let (a, w) = (20.0, 6.5);
    let cross = [
        [c[0] - w, c[1] - a],
        [c[0] + w, c[1] - a],
        [c[0] + w, c[1] - w],
        [c[0] + a, c[1] - w],
        [c[0] + a, c[1] + w],
        [c[0] + w, c[1] + w],
        [c[0] + w, c[1] + a],
        [c[0] - w, c[1] + a],
        [c[0] - w, c[1] + w],
        [c[0] - a, c[1] + w],
        [c[0] - a, c[1] - w],
        [c[0] - w, c[1] - w],
    ];
    p.add(Shape::closed_line(cross.iter().map(|q| at(*q)).collect(), body));
    // Select and Start.
    ring([222.0, 174.0], 5.0, thin);
    ring([260.0, 174.0], 5.0, thin);
    // Face buttons.
    let small = FontId::proportional(8.5 * k.max(0.8));
    for (label, c) in [("Y", [309.0, 157.0]), ("X", [291.0, 174.0]), ("B", [327.0, 171.0]), ("A", [309.0, 189.0])] {
        ring(c, 8.5, thin);
        p.text(at(c), Align2::CENTER_CENTER, label, small.clone(), BODY);
    }
    if layout == PadLayout::Xbox {
        // The guide button (a plain ring: no maker's logo).
        ring([240.0, 157.0], 7.5, body);
        p.line_segment([at([233.0, 125.0]), at([247.0, 125.0])], thin);
    }
}

/// Studio's VR controller options for a headset, greyed: Rustudio has no VR.
fn vr_section(ui: &mut egui::Ui, full: Rect, y: f32) {
    let r = Rect::from_min_max(pos2(full.min.x + 9.0, y), pos2(full.max.x - 9.0, full.max.y));
    ui.painter().hline(full.x_range(), y - 4.0, Stroke::new(1.0, theme::DIVIDER));
    let mut c = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min)));
    c.disable();
    c.add_space(6.0);
    c.label("VR controllers aren't simulated in Rustudio.");
    c.horizontal(|ui| {
        ui.label("Move and rotate:");
        ui.add(egui::RadioButton::new(false, ""));
    });
    c.horizontal(|ui| {
        let (mut a, mut b, mut d) = (true, false, false);
        ui.checkbox(&mut a, "Headset");
        ui.checkbox(&mut b, "Left Controller");
        ui.checkbox(&mut d, "Right Controller");
    });
    c.label("Translation sensitivity:");
    let mut s = 5.0f32;
    c.add(egui::Slider::new(&mut s, 1.0..=10.0));
}

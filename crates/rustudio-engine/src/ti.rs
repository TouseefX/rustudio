//! Tabler Icons (MIT, https://tabler.io/icons; app/assets/tabler, credited
//! in CREDITS.md): the outline and filled icon fonts, drawn by name. Every
//! ribbon, menu and Explorer icon comes from this one set so they read as
//! one family; colours are rustudio's own.
//!
//! The editor's text font is Studio's Builder Sans, read at run time from
//! the content folder like the other Studio files (CREDITS.md), else egui's.

use std::collections::HashMap;
use std::sync::OnceLock;

use egui::{Align2, Color32, FontFamily, FontId, Painter, Rect};

const OUTLINE: &[u8] = include_bytes!("../../../assets/tabler/tabler-icons.ttf");
const FILLED: &[u8] = include_bytes!("../../../assets/tabler/tabler-icons-filled.ttf");

fn table(text: &'static str) -> HashMap<&'static str, char> {
    text.lines()
        .filter_map(|l| {
            let (n, c) = l.split_once(' ')?;
            Some((n, char::from_u32(u32::from_str_radix(c, 16).ok()?)?))
        })
        .collect()
}

fn outline() -> &'static HashMap<&'static str, char> {
    static T: OnceLock<HashMap<&'static str, char>> = OnceLock::new();
    T.get_or_init(|| table(include_str!("../../../assets/tabler/codepoints.txt")))
}

fn filled() -> &'static HashMap<&'static str, char> {
    static T: OnceLock<HashMap<&'static str, char>> = OnceLock::new();
    T.get_or_init(|| table(include_str!("../../../assets/tabler/codepoints-filled.txt")))
}

/// rustudio's own drawn icons (not glyphs). `rs-model`: three coloured
/// shapes in a triangle (pink triangle on top, blue circle bottom left,
/// amber square bottom right), Roblox's colourful Model idea with
/// rustudio's own shapes.
const DRAWN: &[&str] = &["rs-model", "rs-virtual-controller"];

/// The Device Simulator's virtual controller: a gamepad outline with a gear
/// in a gap at its lower right (Tabler has no such icon; drawn on a
/// 12-unit grid, stroke 0.85, like the rest of the bar's icons).
fn paint_virtual_controller(p: &Painter, r: Rect, color: Color32) {
    use egui::{pos2, Pos2, Stroke};
    let n = r.width().min(r.height());
    let o = r.center() - egui::vec2(n, n) / 2.0;
    let at = |x: f32, y: f32| pos2(o.x + x / 12.0 * n, o.y + y / 12.0 * n);
    let stroke = Stroke::new((0.85 / 12.0 * n).max(1.0), color);
    let body: [(f32, f32); 15] = [
        (3.8, 2.1), (8.2, 2.1), (9.35, 2.5), (10.05, 3.85), (10.6, 7.75), (10.3, 8.7), (8.9, 8.8),
        (7.9, 7.9), (4.1, 7.9), (3.0, 8.8), (1.7, 8.7), (1.3, 7.75), (1.85, 3.85), (2.6, 2.5), (3.8, 2.1),
    ];
    let gear = (9.4, 9.0);
    let cut = 2.3;
    for w in body.windows(2) {
        // (the outline stops short of the gear's gap)
        let pts: Vec<Pos2> = (0..=8)
            .map(|i| {
                let t = i as f32 / 8.0;
                (w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t)
            })
            .filter(|(x, y)| ((x - gear.0).powi(2) + (y - gear.1).powi(2)).sqrt() > cut)
            .map(|(x, y)| at(x, y))
            .collect();
        if pts.len() >= 2 {
            p.line(pts, stroke);
        }
    }
    p.line_segment([at(4.0, 4.2), at(4.0, 5.4)], stroke);
    p.line_segment([at(3.4, 4.8), at(4.6, 4.8)], stroke);
    p.circle_filled(at(7.5, 4.5), stroke.width * 0.6, color);
    p.circle_filled(at(8.4, 5.3), stroke.width * 0.6, color);
    let c = at(gear.0, gear.1);
    let u = n / 12.0;
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::TAU / 8.0;
        let d = egui::vec2(a.cos(), a.sin());
        p.line_segment([c + d * 0.9 * u, c + d * 1.6 * u], Stroke::new(0.7 * u, color));
    }
    p.circle_stroke(c, 0.85 * u, Stroke::new(0.6 * u, color));
}

fn paint_model(p: &Painter, r: Rect) {
    use egui::{pos2, Pos2, Shape, Stroke};
    let n = r.width().min(r.height());
    let o = r.center() - egui::vec2(n, n) / 2.0;
    let pad = n * 0.06;
    let c = (n - 2.0 * pad) / 2.0;
    let g = n * 0.10;
    let h = c - g / 2.0;
    let (x0, y0) = (o.x + pad, o.y + pad);
    let (x1, y1) = (x0 + c + g / 2.0, y0 + c + g / 2.0);
    let cx = o.x + n / 2.0;
    let rgb = Color32::from_rgb;
    let tri: Vec<Pos2> = vec![pos2(cx, y0), pos2(cx + h / 2.0, y0 + h), pos2(cx - h / 2.0, y0 + h)];
    p.add(Shape::convex_polygon(tri, rgb(255, 95, 150), Stroke::NONE));
    p.circle_filled(pos2(x0 + h / 2.0, y1 + h / 2.0), h / 2.0, rgb(80, 170, 255));
    p.rect_filled(
        Rect::from_min_size(pos2(x1, y1), egui::vec2(h, h)),
        egui::CornerRadius::same((h * 0.22).round() as u8),
        rgb(255, 184, 60),
    );
}

/// Whether the set has an icon of that name (`-filled` suffix: the filled one).
pub fn has(name: &str) -> bool {
    if DRAWN.contains(&name) {
        return true;
    }
    match name.strip_suffix("-filled") {
        Some(n) => filled().contains_key(n),
        None => outline().contains_key(name),
    }
}

/// The glyph and font family for an icon name ("cube", "player-play-filled").
pub fn glyph(name: &str) -> Option<(char, FontFamily)> {
    match name.strip_suffix("-filled") {
        Some(n) => filled()
            .get(n)
            .map(|c| (*c, FontFamily::Name("tabler-filled".into()))),
        None => outline()
            .get(name)
            .map(|c| (*c, FontFamily::Name("tabler".into()))),
    }
}

/// A bundled icon for use as button or label text.
pub fn text(name: &str, size: f32) -> egui::RichText {
    let (glyph, family) = glyph(name).expect("named Tabler icon");
    egui::RichText::new(glyph.to_string()).font(FontId::new(size, family))
}

/// Paints icon `name` centred in `r`, sized to fit.
pub fn paint(p: &Painter, r: Rect, name: &str, color: Color32) {
    if name == "rs-virtual-controller" {
        paint_virtual_controller(p, r, color);
        return;
    }
    if name == "rs-model" {
        // its own colours, whatever the caller's tint
        let _ = color;
        paint_model(p, r);
        return;
    }
    let name = if has(name) { name } else { "square" };
    let Some((c, family)) = glyph(name) else {
        return;
    };
    let size = r.width().min(r.height());
    p.text(
        r.center(),
        Align2::CENTER_CENTER,
        c.to_string(),
        FontId::new(size, family),
        color,
    );
}

/// Fonts available even without Studio content or system fonts.
pub fn bundled_fonts() -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    // Egui already embeds Hack for monospace. Reuse its arrows and symbols
    // when the proportional and emoji faces have no matching glyph.
    let symbols = defs.families[&FontFamily::Monospace][0].clone();
    defs.families.entry(FontFamily::Proportional).or_default().push(symbols);
    defs.font_data.insert(
        "tabler".into(),
        std::sync::Arc::new(egui::FontData::from_static(OUTLINE)),
    );
    defs.font_data.insert(
        "tabler-filled".into(),
        std::sync::Arc::new(egui::FontData::from_static(FILLED)),
    );
    defs.families
        .insert(FontFamily::Name("tabler".into()), vec!["tabler".into()]);
    defs.families.insert(
        FontFamily::Name("tabler-filled".into()),
        vec!["tabler-filled".into()],
    );
    defs
}

/// Adds the icon fonts and Studio's Builder Sans to egui.
pub fn install(ctx: &egui::Context) {
    let mut defs = bundled_fonts();
    let fonts = rustudio_core::assets::content_dir().join("fonts");
    for (key, file, family) in [
        ("builder-sans", "BuilderSans-Regular.otf", FontFamily::Proportional),
        ("builder-sans-bold", "BuilderSans-Bold.otf", FontFamily::Name("bold".into())),
        // (the in-game menu's buttons and tabs are Builder Sans Medium)
        ("builder-sans-medium", "BuilderSans-Medium.otf", FontFamily::Name("medium".into())),
    ] {
        if let Ok(bytes) = std::fs::read(fonts.join(file)) {
            defs.font_data.insert(
                key.into(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            let list = defs.families.entry(family).or_default();
            list.insert(0, key.into());
        }
    }
    // Studio's Output is monospace, warnings and errors in bold: Builder
    // Mono from the content folder when it is there.
    for (key, file, family) in [
        ("builder-mono", "BuilderMono-Regular.otf", FontFamily::Monospace),
        ("builder-mono-bold", "BuilderMono-Bold.otf", FontFamily::Name("mono-bold".into())),
    ] {
        if let Ok(bytes) = std::fs::read(fonts.join(file)) {
            defs.font_data.insert(key.into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            defs.families.entry(family).or_default().insert(0, key.into());
        }
    }
    let mono = defs.families[&FontFamily::Monospace].clone();
    defs.families.entry(FontFamily::Name("mono-bold".into())).or_default().extend(mono);
    // Symbols and scripts the faces lack (emoji, Arabic, CJK, ...):
    // the same shared fallback fonts as game fonts get.
    for (i, bytes) in crate::gui::fallback_fonts().iter().enumerate() {
        let key = format!("rbx:fallback{i}");
        defs.font_data
            .insert(key.clone(), std::sync::Arc::new(crate::gui::fallback_font_data(bytes)));
        defs.families.entry(FontFamily::Proportional).or_default().push(key);
    }
    // Bold falls back to the regular face (and then egui's) when missing.
    let prop = defs.families[&FontFamily::Proportional].clone();
    defs.families
        .entry(FontFamily::Name("bold".into()))
        .or_default()
        .extend(prop.clone());
    defs.families
        .entry(FontFamily::Name("medium".into()))
        .or_default()
        .extend(prop);
    ctx.set_fonts(defs);
}

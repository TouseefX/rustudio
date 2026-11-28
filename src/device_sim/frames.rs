//! The device frame round the simulated screen, drawn by Rustudio from the
//! measurements in `devices::frame_art` (Studio's own frame images are
//! Roblox's art and aren't used), and the letterbox round it.

use egui::{epaint::Mesh, pos2, vec2, Color32, CornerRadius, Painter, Pos2, Rect, Shape};
use rustudio_core::devices::{FrameArt, FrameFamily, Orientation};
use rustudio_engine::theme;
use rustudio_player::device_view::DeviceView;

/// Maps the frame art (landscape, device points from the screen's top-left)
/// to window points: portrait turns it a quarter turn clockwise, so the
/// landscape left edge (the notch) is at the top.
struct Place<'a> {
    view: &'a DeviceView,
    portrait: bool,
}

impl Place<'_> {
    fn new(view: &DeviceView) -> Place<'_> {
        Place { view, portrait: view.orientation == Orientation::Portrait && view.device.rotates() }
    }

    /// A landscape rect (x, y, w, h) in window points.
    fn rect(&self, [x, y, w, h]: [f32; 4]) -> Rect {
        let r = if self.portrait {
            let lh = self.view.logical[0]; // the landscape height is the portrait width
            Rect::from_min_size(pos2(lh - y - h, x), vec2(h, w))
        } else {
            Rect::from_min_size(pos2(x, y), vec2(w, h))
        };
        Rect::from_min_max(self.view.to_window(r.min), self.view.to_window(r.max))
    }

    /// The body round the screen (landscape bezel insets, turned).
    fn grow(&self, top: f32, bottom: f32, left: f32, right: f32) -> Rect {
        let (t, b, l, r) = if self.portrait { (left, right, bottom, top) } else { (top, bottom, left, right) };
        let k = self.view.k;
        let s = self.view.screen_win;
        Rect::from_min_max(s.min - vec2(l, t) * k, s.max + vec2(r, b) * k)
    }
}

/// The letterbox: the editor's panel colour round the device (Studio:
/// #202227), and the view's scroll bar track at the right edge, with its
/// thumb when the device is taller than the panel (`scroll`: how far down,
/// `over`: how much doesn't fit).
pub fn paint_letterbox(p: &Painter, panel: Rect, scroll: f32, over: f32) {
    p.rect_filled(panel, 0.0, theme::PANEL_DARK);
    let track = scrollbar_track(panel);
    p.rect_filled(track, 0.0, theme::DIVIDER);
    if over > 0.0 {
        let visible = track.height() / (track.height() + over);
        let h = (track.height() * visible).max(16.0);
        let y = track.min.y + (track.height() - h) * (scroll / over).clamp(0.0, 1.0);
        p.rect_filled(Rect::from_min_size(pos2(track.min.x + 1.0, y), vec2(track.width() - 2.0, h)), 3.0, theme::TEXT_DIM);
    }
}

pub fn scrollbar_track(panel: Rect) -> Rect {
    let w = rustudio_core::mech::DEVICE_SCROLLBAR;
    Rect::from_min_max(pos2(panel.max.x - w, panel.min.y), panel.max)
}

/// The body, side frame and buttons: drawn before the screen's picture.
pub fn paint_body(p: &Painter, view: &DeviceView, art: &FrameArt) {
    let at = Place::new(view);
    let k = view.k;
    let b = art.bezel;
    for &btn in art.side_buttons {
        let r = at.rect(btn);
        p.rect_filled(r, CornerRadius::same(r.width().min(r.height()).min(255.0) as u8 / 2), theme::DEVICE_SIDE_BUTTON);
    }
    if art.rim > 0.0 {
        let outer = at.grow(b.top + art.rim, b.bottom + art.rim, b.left + art.rim, b.right + art.rim);
        p.rect_filled(outer, radius((art.outer_radius + art.rim) * k), theme::DEVICE_SIDE_BUTTON);
    }
    let body = at.grow(b.top, b.bottom, b.left, b.right);
    let fill = if art.grey_body { theme::DEVICE_FRAME_BODY_GREY } else { theme::DEVICE_FRAME_BODY };
    p.rect_filled(body, radius(art.outer_radius * k), fill);
    for &f in art.features {
        let r = at.rect(f);
        let round = r.width().min(r.height()) / 2.0;
        p.rect_filled(r, radius(round), theme::DEVICE_BEZEL_FEATURE);
        if matches!(art.family, FrameFamily::HomeButtonIphone | FrameFamily::Tablet { camera_and_home: true })
            && r.width() > 30.0 * k
        {
            // the home button's ring
            p.circle_stroke(r.center(), round - 1.0, egui::Stroke::new((2.0 * k).max(1.0), Color32::from_rgb(52, 52, 52)));
        }
    }
}

/// What the body covers over the picture: the screen's rounded corners and
/// the notch, island, teardrop or punch hole.
pub fn paint_over(p: &Painter, view: &DeviceView, art: &FrameArt) {
    let at = Place::new(view);
    let k = view.k;
    let fill = if art.grey_body { theme::DEVICE_FRAME_BODY_GREY } else { theme::DEVICE_FRAME_BODY };
    let s = view.screen_win;
    if art.screen_radius > 0.0 {
        // between the screen's rounded corner and the body's: the body's
        // colour over the picture's square corner, never past the body
        // (the picture's square corner can reach past a round body: the
        // letterbox covers it there, then the side frame and the body)
        let r = (art.screen_radius * k).min(s.width() / 2.0).min(s.height() / 2.0);
        let b = art.bezel;
        let body = at.grow(b.top, b.bottom, b.left, b.right);
        let ro = (art.outer_radius * k).min(body.width() / 2.0).min(body.height() / 2.0);
        let rim = art.rim * k;
        let far = ro + rim + 2.0 * r;
        let corners = |rect: Rect, rad: f32| {
            [
                (rect.left_top() + vec2(rad, rad), std::f32::consts::PI),
                (rect.right_top() + vec2(-rad, rad), -std::f32::consts::FRAC_PI_2),
                (rect.right_bottom() + vec2(-rad, -rad), 0.0),
                (rect.left_bottom() + vec2(rad, -rad), std::f32::consts::FRAC_PI_2),
            ]
        };
        for ((inner, start), ((outer, _), (screen, _))) in corners(body, ro).into_iter().zip(corners(body, ro).into_iter().zip(corners(s, r))) {
            p.add(corner_ring(outer, ro + rim, outer, far, start, theme::PANEL_DARK));
            if rim > 0.0 {
                p.add(corner_ring(outer, ro, outer, ro + rim, start, theme::DEVICE_SIDE_BUTTON));
            }
            p.add(corner_ring(screen, r, inner, ro, start, fill));
        }
    }
    if let Some(c) = art.cutout {
        let r = at.rect(c);
        let short = r.width().min(r.height());
        let shape = match art.family {
            FrameFamily::PunchHoleAndroid => Shape::circle_filled(r.center(), short / 2.0, fill),
            FrameFamily::IslandIphone => Shape::rect_filled(r, radius(short / 2.0), fill),
            // the notch and teardrop hang off the screen's edge: rounded
            // only on the side that faces into the screen
            FrameFamily::TeardropAndroid => edge_rounded(r, short / 2.0, at.portrait, fill),
            _ => edge_rounded(r, (8.0 * k).min(short / 2.0), at.portrait, fill),
        };
        p.add(shape);
    }
}

fn radius(r: f32) -> CornerRadius {
    CornerRadius::same(r.clamp(0.0, 255.0).round() as u8)
}

/// A cut-out on the screen's leading edge (left in landscape, top in
/// portrait), rounded on its inner corners.
fn edge_rounded(r: Rect, rad: f32, portrait: bool, fill: Color32) -> Shape {
    let q = rad.clamp(0.0, 255.0).round() as u8;
    let cr = if portrait {
        CornerRadius { nw: 0, ne: 0, sw: q, se: q }
    } else {
        CornerRadius { nw: 0, ne: q, sw: 0, se: q }
    };
    Shape::rect_filled(r, cr, fill)
}

/// The band between two quarter arcs (the screen's corner inside, the
/// body's outside) over a quarter turn from `start`.
fn corner_ring(inner: Pos2, ri: f32, outer: Pos2, ro: f32, start: f32, fill: Color32) -> Shape {
    let mut mesh = Mesh::default();
    let n: u32 = 16;
    for i in 0..=n {
        let a = start + std::f32::consts::FRAC_PI_2 * i as f32 / n as f32;
        let d = vec2(a.cos(), a.sin());
        mesh.colored_vertex(inner + ri * d, fill);
        mesh.colored_vertex(outer + ro * d, fill);
    }
    for i in 0..n {
        let j = 2 * i;
        mesh.add_triangle(j, j + 1, j + 2);
        mesh.add_triangle(j + 1, j + 3, j + 2);
    }
    Shape::mesh(mesh)
}

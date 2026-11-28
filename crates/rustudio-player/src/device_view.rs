//! Where a simulated device's screen sits in the 3D panel (Device
//! Simulator), its scale, and the maps between window points and the
//! device's own points.

use egui::{emath::TSTransform, pos2, vec2, Pos2, Rect};
use rustudio_core::devices::{logical_size, Device, Insets, Orientation};
use rustudio_core::mech;
use rustudio_engine::runtime::ext::DeviceInput;

/// The Display scaling menu's modes.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Scaling {
    #[default]
    FitToWindow,
    ActualResolution,
    PhysicalSize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeviceView {
    pub device: Device,
    pub orientation: Orientation,
    /// The whole device screen, in window points.
    pub screen_win: Rect,
    /// Window points per device point.
    pub k: f32,
    /// The screen in device points as scripts see it (floored, as Studio).
    pub logical: [f32; 2],
    pub safe: Insets,
}

impl DeviceView {
    /// Lays the device out in `panel`. `bezel` is the frame drawn around the
    /// screen (landscape, device points), `dpi` the calibrated screen density
    /// (Physical size) and `ppp` the window's pixels per point.
    pub fn layout(panel: Rect, device: &Device, o: Orientation, scaling: Scaling, bezel: Insets, dpi: f32, ppp: f32) -> Self {
        let pts = device.points(o);
        let [w, h] = [pts[0] as f32, pts[1] as f32];
        let bezel = match o {
            Orientation::Portrait if device.rotates() => {
                Insets::new(bezel.left, bezel.right, bezel.bottom, bezel.top)
            }
            _ => bezel,
        };
        let (outer_w, outer_h) = (w + bezel.left + bezel.right, h + bezel.top + bezel.bottom);
        let ppp = ppp.max(0.01);
        // (the panel in whole pixels: a fraction of a pixel more or less
        // doesn't change the scale, so the size scripts see stays put)
        let whole = |v: f32| (v * ppp + 1e-3).floor() / ppp;
        let k = match scaling {
            Scaling::FitToWindow => {
                // (a frameless device fills the panel: Studio's Xbox One is
                // 1080 high in a 718 px panel, measured 1919x1080)
                let pad = if device.frame.is_some() { mech::DEVICE_FIT_PADDING_PX } else { 0.0 };
                ((whole(panel.width()) - 2.0 * pad) / outer_w).min((whole(panel.height()) - 2.0 * pad) / outer_h)
            }
            Scaling::ActualResolution => 1.0,
            Scaling::PhysicalSize => device.resolution_scale / device.ppi.max(1.0) * dpi / ppp,
        }
        .max(0.01);
        // (the floored screen, centred with its frame)
        let logical = logical_size(pts, f64::from(k) * f64::from(ppp));
        let outer = vec2(logical[0] + bezel.left + bezel.right, logical[1] + bezel.top + bezel.bottom) * k;
        let outer_min = panel.center() - outer / 2.0;
        let snap = |v: f32| (v * ppp).round() / ppp;
        let min = pos2(snap(outer_min.x + bezel.left * k), snap(outer_min.y + bezel.top * k));
        Self {
            device: device.clone(),
            orientation: o,
            screen_win: Rect::from_min_size(min, vec2(logical[0], logical[1]) * k),
            k,
            logical,
            safe: device.safe_insets(o),
        }
    }

    pub fn to_device(&self, p: Pos2) -> Pos2 {
        pos2((p.x - self.screen_win.min.x) / self.k, (p.y - self.screen_win.min.y) / self.k)
    }

    pub fn to_window(&self, p: Pos2) -> Pos2 {
        self.screen_win.min + p.to_vec2() * self.k
    }

    /// Device points to window points (the GUI pass's shapes).
    pub fn gui_transform(&self) -> TSTransform {
        TSTransform::new(self.screen_win.min.to_vec2(), self.k)
    }

    /// Window points to device points (the GUI's pointer).
    pub fn pointer_transform(&self) -> TSTransform {
        self.gui_transform().inverse()
    }

    pub fn screen_rect_dev(&self) -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(self.logical[0], self.logical[1]))
    }

    pub fn safe_rect_dev(&self) -> Rect {
        let s = self.safe;
        Rect::from_min_max(pos2(s.left, s.top), pos2(self.logical[0] - s.right, self.logical[1] - s.bottom))
    }

    /// The safe area in window points.
    pub fn safe_rect_win(&self) -> Rect {
        let r = self.safe_rect_dev();
        Rect::from_min_max(self.to_window(r.min), self.to_window(r.max))
    }

    /// Camera.ViewportSize: the safe area in device points.
    pub fn safe_size(&self) -> [f32; 2] {
        let r = self.safe_rect_dev();
        [r.width(), r.height()]
    }

    /// The device's own input for the client (the Controller Emulator and
    /// the keyboard flip are applied by `Game::device_input`).
    pub fn input(&self) -> DeviceInput {
        let s = self.safe;
        DeviceInput {
            flags: self.device.flags(),
            portrait: self.orientation == Orientation::Portrait && self.device.rotates(),
            screen: self.logical,
            safe: [s.left, s.top, s.right, s.bottom],
            display_size: self.device.display_size,
            simulated: true,
            flip: false,
            id: {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                self.device.id.hash(&mut h);
                h.finish() | 1
            },
            own_pad: self.device.native_gamepad(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustudio_core::devices::preset;

    fn iphone(panel: Rect, o: Orientation) -> DeviceView {
        DeviceView::layout(panel, preset("iphone_14").unwrap(), o, Scaling::FitToWindow, Insets::default(), 96.0, 1.0)
    }

    #[test]
    fn fit_centres_the_device_in_the_panel() {
        let panel = Rect::from_min_size(pos2(0.0, 0.0), vec2(1914.0, 718.0));
        let v = iphone(panel, Orientation::Landscape);
        let p = mech::DEVICE_FIT_PADDING_PX;
        let k = ((1914.0 - 2.0 * p) / 844.0).min((718.0 - 2.0 * p) / 390.0);
        assert!((v.k - k).abs() < 1e-4);
        assert!((v.screen_win.center() - panel.center()).length() < 1.0);
        assert_eq!(v.safe_size(), [v.logical[0] - 94.0, v.logical[1]]);
    }

    #[test]
    fn a_frameless_device_fills_the_panel() {
        // measured, Studio's Xbox One in a 1914x718 panel: 1919 x 1080
        let panel = Rect::from_min_size(pos2(0.0, 0.0), vec2(1914.0, 718.0));
        let v = DeviceView::layout(panel, preset("xbox").unwrap(), Orientation::Landscape, Scaling::FitToWindow, Insets::default(), 96.0, 1.0);
        assert_eq!(v.logical, [1919.0, 1080.0]);
        assert_eq!(v.safe_size(), [1919.0, 1080.0]);
    }

    #[test]
    fn device_and_window_points_round_trip() {
        let v = iphone(Rect::from_min_size(pos2(100.0, 50.0), vec2(1914.0, 718.0)), Orientation::Landscape);
        let q = pos2(123.25, 45.5);
        assert!((v.to_device(v.to_window(q)) - q).length() < 1e-3);
        assert_eq!(v.to_window(pos2(0.0, 0.0)), v.screen_win.min);
        assert!((v.gui_transform() * q - v.to_window(q)).length() < 1e-3);
        assert!((v.pointer_transform() * v.to_window(q) - q).length() < 1e-3);
    }

    #[test]
    fn view_size_is_stable_under_subpixel_panel_changes() {
        let a = iphone(Rect::from_min_size(Pos2::ZERO, vec2(1914.0, 718.0)), Orientation::Landscape);
        let b = iphone(Rect::from_min_size(Pos2::ZERO, vec2(1914.3, 718.2)), Orientation::Landscape);
        assert_eq!(a.safe_size(), b.safe_size());
    }

    #[test]
    fn portrait_swaps_the_screen() {
        let v = iphone(Rect::from_min_size(Pos2::ZERO, vec2(1914.0, 718.0)), Orientation::Portrait);
        assert!(v.logical[0] < v.logical[1]);
        assert_eq!(v.safe_size(), [v.logical[0], v.logical[1] - 47.0 - 34.0]);
        assert!(v.input().portrait);
    }

    #[test]
    fn actual_resolution_is_one_point_per_point() {
        let v = DeviceView::layout(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0)), preset("iphone_14").unwrap(), Orientation::Landscape, Scaling::ActualResolution, Insets::default(), 96.0, 1.0);
        assert_eq!((v.k, v.logical), (1.0, [844.0, 390.0]));
    }

    #[test]
    fn physical_size_uses_the_calibrated_dpi() {
        let v = DeviceView::layout(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0)), preset("iphone_14").unwrap(), Orientation::Landscape, Scaling::PhysicalSize, Insets::default(), 96.0, 1.0);
        assert!((v.k - 3.0 / 460.0 * 96.0).abs() < 1e-4);
    }
}

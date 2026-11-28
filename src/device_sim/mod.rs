//! The Device Simulator's editor state: which device is shown, its
//! orientation and display scaling, kept in Rustudio's settings for runs a
//! person drives (Studio keeps them across sessions too).

pub mod calibrate;
pub mod controller;
pub mod frames;
pub mod manage;
pub mod menus;
pub mod toolbar;

use egui::Rect;
use rustudio_core::devices::{self, CustomDevices, Device, FormButton, Insets, Orientation};
use rustudio_player::device_view::{DeviceView, Scaling};
use serde_json::{json, Value};

const SETTINGS_KEY: &str = "device_simulator";

#[derive(Clone, Debug, PartialEq)]
pub struct DeviceSim {
    /// Test > Device Simulator is on.
    pub shown: bool,
    /// The simulated device; None = Current device (the window itself).
    pub device: Option<String>,
    /// Kept across devices (Studio: a phone left in portrait makes the next
    /// phone start in portrait).
    pub orientation: Orientation,
    pub scaling: Scaling,
    /// Calibrated screen density (Physical size).
    pub dpi: f32,
    /// The last device picked for each form button (Mobile, Desktop,
    /// Console, Headset).
    pub last_per_form: [Option<String>; 4],
    /// The Controller Emulator is on (its panel is open): keys drive a gamepad.
    pub controller: bool,
    /// The panel's layout as picked (None: the device's own).
    pub pad_layout: Option<controller::PadLayout>,
    /// Custom devices and hidden Standard ones (Manage devices).
    pub custom: CustomDevices,
    /// A run a person drives: custom devices are read and written.
    pub persist: bool,
    /// The bar's open menu or popover.
    pub popup: Option<Popup>,
    /// Calibrate physical scaling: the DPI the slider shows while open.
    pub calibrating: Option<f32>,
    pub manage: Option<manage::Manage>,
    /// Actual resolution / Physical size: how far the view is scrolled when
    /// the device doesn't fit the panel.
    pub scroll: egui::Vec2,
}

/// The bar's menus and popovers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Popup {
    Devices,
    Scaling,
    Touch,
    Lan,
    Quality,
}

impl Default for DeviceSim {
    fn default() -> Self {
        Self {
            shown: false,
            device: None,
            orientation: Orientation::Landscape,
            scaling: Scaling::FitToWindow,
            dpi: 96.0,
            last_per_form: Default::default(),
            controller: false,
            pad_layout: None,
            custom: CustomDevices::default(),
            persist: false,
            popup: None,
            calibrating: None,
            manage: None,
            scroll: egui::Vec2::ZERO,
        }
    }
}

fn form_index(b: FormButton) -> usize {
    match b {
        FormButton::Mobile => 0,
        FormButton::Desktop => 1,
        FormButton::Console => 2,
        FormButton::Headset => 3,
    }
}

impl DeviceSim {
    /// The saved state (only in runs a person drives: rustudio_settings
    /// loads nothing otherwise).
    pub fn load() -> Self {
        let mut s = Self::default();
        let Some(v) = rustudio_core::rustudio_settings::saved_value(SETTINGS_KEY) else {
            return s;
        };
        s.shown = v["shown"].as_bool().unwrap_or(false);
        // (a custom device is checked once they are loaded: load_custom)
        s.device = v["device"].as_str().map(str::to_string);
        s.orientation = if v["portrait"].as_bool() == Some(true) { Orientation::Portrait } else { Orientation::Landscape };
        s.scaling = match v["scaling"].as_str() {
            Some("actual") => Scaling::ActualResolution,
            Some("physical") => Scaling::PhysicalSize,
            _ => Scaling::FitToWindow,
        };
        s.dpi = v["dpi"].as_f64().map_or(96.0, |d| d as f32);
        if let Some(list) = v["last_per_form"].as_array() {
            for (slot, id) in s.last_per_form.iter_mut().zip(list) {
                *slot = id.as_str().map(str::to_string);
            }
        }
        s
    }

    pub fn save(&self) {
        let scaling = match self.scaling {
            Scaling::FitToWindow => "fit",
            Scaling::ActualResolution => "actual",
            Scaling::PhysicalSize => "physical",
        };
        let v: Value = json!({
            "shown": self.shown,
            "device": self.device,
            "portrait": self.orientation == Orientation::Portrait,
            "scaling": scaling,
            "dpi": self.dpi,
            "last_per_form": self.last_per_form,
        });
        rustudio_core::rustudio_settings::set(SETTINGS_KEY, Some(v));
    }

    /// A preset or custom device by id.
    pub fn find(&self, id: &str) -> Option<&Device> {
        devices::preset(id).or_else(|| self.custom.devices.iter().find(|d| d.id == id))
    }

    /// The simulated device (None = Current device).
    pub fn current(&self) -> Option<&Device> {
        self.find(self.device.as_deref()?)
    }

    /// The form button the bar shows as the pill (Current device: Desktop).
    pub fn form(&self) -> FormButton {
        self.current().map_or(FormButton::Desktop, |d| d.button())
    }

    /// Picks a device (None = Current device). The orientation stays.
    pub fn select(&mut self, id: Option<&str>) {
        self.pad_layout = None;
        let Some(id) = id else {
            self.device = None;
            return;
        };
        let Some(d) = self.find(id) else { return };
        let (id, slot) = (d.id.clone(), form_index(d.button()));
        self.last_per_form[slot] = Some(id.clone());
        self.device = Some(id);
    }

    /// A form button: that form's last device, else Studio's first pick
    /// (iPhone 14, Current device, Xbox One, Meta Quest 3; estimate from
    /// the devices Studio showed).
    pub fn switch_form(&mut self, b: FormButton) {
        let last = self.last_per_form[form_index(b)].clone().filter(|id| self.find(id).is_some());
        let id = last.or_else(|| match b {
            FormButton::Mobile => Some("iphone_14".into()),
            FormButton::Desktop => None,
            FormButton::Console => Some("xbox".into()),
            FormButton::Headset => Some("meta_quest_3".into()),
        });
        self.select(id.as_deref());
    }

    /// The Virtual controller button: the controller (and its panel) on or
    /// off. Returns the new state.
    pub fn toggle_controller(&mut self) -> bool {
        self.controller = !self.controller;
        self.controller
    }

    pub fn rotate(&mut self) {
        self.orientation = match self.orientation {
            Orientation::Landscape => Orientation::Portrait,
            Orientation::Portrait => Orientation::Landscape,
        };
    }

    /// Where the device sits in the 3D panel; None while the simulator is
    /// off or shows Current device.
    pub fn view(&self, panel: Rect, ppp: f32) -> Option<DeviceView> {
        if !self.shown {
            return None;
        }
        let d = self.current()?;
        let extent = devices::frame_art(&d.id).map_or(Insets::default(), |f| f.extent(d.size));
        let mut v = DeviceView::layout(panel, d, self.orientation, self.scaling, extent, self.dpi, ppp);
        // A device bigger than the panel scrolls (Studio's scroll bars).
        let content = Self::content(&v, extent);
        let over = (content.size() - panel.size()).max(egui::Vec2::ZERO);
        let scroll = self.scroll.clamp(egui::Vec2::ZERO, over);
        let mut shift = egui::Vec2::ZERO;
        if over.x > 0.0 {
            shift.x = panel.min.x - scroll.x - content.min.x;
        }
        if over.y > 0.0 {
            shift.y = panel.min.y - scroll.y - content.min.y;
        }
        v.screen_win = v.screen_win.translate(shift);
        Some(v)
    }

    /// The device and its frame, in window points.
    pub fn content(v: &DeviceView, extent: Insets) -> Rect {
        let e = if v.orientation == Orientation::Portrait && v.device.rotates() {
            Insets::new(extent.left, extent.right, extent.bottom, extent.top)
        } else {
            extent
        };
        let k = v.k;
        Rect::from_min_max(v.screen_win.min - egui::vec2(e.left, e.top) * k, v.screen_win.max + egui::vec2(e.right, e.bottom) * k)
    }

    /// How far the device overflows the panel (0: it fits).
    pub fn overflow(&self, v: &DeviceView, panel: Rect) -> egui::Vec2 {
        let extent = devices::frame_art(&v.device.id).map_or(Insets::default(), |f| f.extent(v.device.size));
        (Self::content(v, extent).size() - panel.size()).max(egui::Vec2::ZERO)
    }

    /// Reads custom devices (runs a person drives).
    pub fn load_custom(&mut self) {
        self.persist = true;
        self.custom = devices::load_custom(&devices::custom_path());
        if self.device.as_deref().is_some_and(|id| self.find(id).is_none()) {
            self.device = None;
        }
    }

    pub fn save_custom(&self) {
        if self.persist {
            if let Err(e) = devices::save_custom(&devices::custom_path(), &self.custom) {
                eprintln!("rustudio: Device Simulator: can't save custom devices: {e}");
            }
        }
    }
}

impl crate::app::Editor {
    /// The Controller Emulator panel (a dock tab).
    pub(crate) fn controller_emulator_ui(&mut self, ui: &mut egui::Ui) {
        for a in controller::panel(ui, &self.device_sim) {
            self.run_action(&a);
        }
    }

    /// The bar's popups and the simulator's dialogs, then the actions the
    /// bar and they asked for.
    pub(crate) fn device_sim_windows(&mut self, ctx: &egui::Context, panel: Rect, mut actions: Vec<String>) {
        if let Some(which) = self.device_sim.popup {
            let key = match which {
                Popup::Devices => "devsim.pill",
                Popup::Scaling => "devsim.scaling",
                Popup::Touch => "devsim.touch",
                Popup::Lan => "devsim.lan",
                Popup::Quality => "devsim.quality",
            };
            match self.ui_rects.get(key).copied().filter(|_| self.device_sim.shown) {
                Some(anchor) => {
                    let (picked, close) = menus::show(ctx, &self.device_sim, anchor, panel.height() - 8.0);
                    if close {
                        self.device_sim.popup = None;
                    }
                    actions.extend(picked);
                }
                None => self.device_sim.popup = None,
            }
        }
        if let Some(mut dpi) = self.device_sim.calibrating {
            match calibrate::window(ctx, &mut dpi) {
                calibrate::Outcome::Open => self.device_sim.calibrating = Some(dpi),
                calibrate::Outcome::Accept(d) => {
                    self.device_sim.dpi = d;
                    self.device_sim.calibrating = None;
                    self.device_sim.save();
                }
                calibrate::Outcome::Cancel => self.device_sim.calibrating = None,
            }
        }
        if let Some(mut m) = self.device_sim.manage.take() {
            let out = manage::window(ctx, &mut m, &mut self.device_sim.custom);
            if out.changed {
                self.device_sim.save_custom();
                if self.device_sim.device.as_deref().is_some_and(|id| self.device_sim.find(id).is_none()) {
                    self.device_sim.device = None;
                }
            }
            if !out.close {
                self.device_sim.manage = Some(m);
            }
        }
        for a in actions {
            self.run_action(&a);
        }
    }

    /// The Device Simulator's actions (studio_actions.rs; --action).
    pub(crate) fn device_sim_action(&mut self, a: &str) -> bool {
        if a == "device_controller" {
            // The bar's Virtual controller button: opens or closes the
            // Controller Emulator panel; open, the controller is on.
            let on = self.device_sim.toggle_controller();
            if !self.harness.run.no_ui {
                self.ui.panel_requests.push((crate::app::Tab::ControllerEmulator, Some(on)));
            }
            return true;
        }
        if let Some(l) = a.strip_prefix("device_pad_layout:") {
            self.device_sim.pad_layout = Some(if l == "xbox" { controller::PadLayout::Xbox } else { controller::PadLayout::GenericGamepad });
            return true;
        }
        let sim = &mut self.device_sim;
        match a {
            "device_simulator" => sim.shown = !sim.shown,
            "device_rotate" => sim.rotate(),
            "device_calibrate" => sim.calibrating = Some(sim.dpi),
            "device_manage" => sim.manage = Some(manage::Manage::new(sim.form())),
            _ => {
                if let Some(id) = a.strip_prefix("device:") {
                    sim.shown = true;
                    sim.select((id != "none").then_some(id));
                } else if let Some(f) = a.strip_prefix("device_form:") {
                    let Some(b) = toolbar::FORMS.into_iter().find(|b| toolbar::form_key(*b) == f) else { return true };
                    sim.switch_form(b);
                } else if let Some(m) = a.strip_prefix("device_scaling:") {
                    sim.scroll = egui::Vec2::ZERO;
                    sim.scaling = match m {
                        "actual" => Scaling::ActualResolution,
                        "physical" => Scaling::PhysicalSize,
                        _ => Scaling::FitToWindow,
                    };
                } else if let Some(p) = a.strip_prefix("device_popup:") {
                    let want = match p {
                        "devices" => Popup::Devices,
                        "scaling" => Popup::Scaling,
                        "touch" => Popup::Touch,
                        "lan" => Popup::Lan,
                        _ => Popup::Quality,
                    };
                    sim.popup = if sim.popup == Some(want) { None } else { Some(want) };
                    return true;
                } else if a == "device_back" {
                    // Android's back button opens Roblox's in-game menu
                    // (GuiService.MenuOpened, no key event; measured).
                    if self.game.is_playing() {
                        self.game.android_back();
                    }
                    return true;
                } else {
                    return false;
                }
            }
        }
        sim.save();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    #[test]
    fn controller_toggle_follows_the_panel() {
        use controller::PadLayout;
        let mut s = DeviceSim { shown: true, ..Default::default() };
        s.select(Some("iphone_14"));
        assert!(s.toggle_controller());
        assert_eq!(s.pad_layout(), PadLayout::GenericGamepad);
        // a console shows the Xbox layout unless one was picked
        s.select(Some("xbox"));
        assert_eq!(s.pad_layout(), PadLayout::Xbox);
        s.pad_layout = Some(PadLayout::GenericGamepad);
        assert_eq!(s.pad_layout(), PadLayout::GenericGamepad);
        s.select(Some("ps5"));
        assert_eq!(s.pad_layout(), PadLayout::Xbox);
        assert!(s.controller, "switching devices keeps it on");
        assert!(!s.toggle_controller());
    }

    #[test]
    fn device_sim_keeps_orientation_across_phones() {
        let panel = Rect::from_min_size(pos2(0.0, 0.0), vec2(1914.0, 718.0));
        let mut s = DeviceSim { shown: true, ..Default::default() };
        s.select(Some("iphone_14"));
        s.rotate();
        s.select(Some("iphone_16"));
        assert_eq!(s.orientation, Orientation::Portrait);
        let v = s.view(panel, 1.0).unwrap();
        assert!(v.logical[0] < v.logical[1]);
        // a console doesn't turn
        s.select(Some("xbox"));
        let v = s.view(panel, 1.0).unwrap();
        assert!(v.logical[0] > v.logical[1]);
        s.select(Some("iphone_14"));
        assert!(s.view(panel, 1.0).unwrap().input().portrait);
        // Current device and the simulator off show the window itself
        s.select(None);
        assert!(s.view(panel, 1.0).is_none());
        s.select(Some("iphone_14"));
        s.shown = false;
        assert!(s.view(panel, 1.0).is_none());
        assert_eq!(s.last_per_form[0].as_deref(), Some("iphone_14"));
        assert_eq!(s.last_per_form[2].as_deref(), Some("xbox"));
    }

    #[test]
    fn form_switch_remembers_the_last_device() {
        let mut s = DeviceSim { shown: true, ..Default::default() };
        s.select(Some("samsung_galaxy_a16"));
        s.switch_form(FormButton::Console);
        assert_eq!(s.device.as_deref(), Some("xbox"));
        s.switch_form(FormButton::Mobile);
        assert_eq!(s.device.as_deref(), Some("samsung_galaxy_a16"));
        s.switch_form(FormButton::Desktop);
        assert_eq!(s.device, None);
        assert_eq!(s.form(), FormButton::Desktop);
    }

    #[test]
    fn fit_includes_the_frame() {
        // Studio's iPhone 14 in a 718 px panel: scaled 1.53, 843 x 389 (measured)
        let panel = Rect::from_min_size(pos2(0.0, 0.0), vec2(1906.0, 718.0));
        let mut s = DeviceSim { shown: true, ..Default::default() };
        s.select(Some("iphone_14"));
        let v = s.view(panel, 1.0).unwrap();
        assert!((v.k - 1.5308).abs() < 0.01, "{}", v.k);
        assert_eq!((v.logical, v.safe_size()), ([843.0, 389.0], [749.0, 389.0]));
        s.rotate();
        let v = s.view(panel, 1.0).unwrap();
        assert_eq!((v.logical, v.safe_size()), ([389.0, 843.0], [389.0, 762.0]));
    }

    #[test]
    fn unknown_devices_are_ignored() {
        let mut s = DeviceSim::default();
        s.select(Some("no_such_phone"));
        assert_eq!(s.device, None);
    }
}

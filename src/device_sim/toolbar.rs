//! The Device Simulator's bar over the view (Studio 0.748): the four form
//! buttons with the current form's turned into the device pill on the left;
//! rotate, Android back, touch controls, the virtual controller, display
//! scaling and the LAN and Q## pills on the right.

use std::collections::HashMap;

use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, Sense};
use rustudio_core::devices::{Device, FormButton, Platform};
use rustudio_core::mech;
use rustudio_engine::theme;

use super::{DeviceSim, Popup};

/// A button on the bar's right side, in Studio's order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BarButton {
    Rotate,
    Back,
    Touch,
    Controller,
    Scaling,
    Lan,
    Quality,
    /// The Q pill with no device: "Auto".
    Auto,
}

/// What the bar's left side holds, in order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeftSlot {
    Form(FormButton),
    Pill,
}

pub const FORMS: [FormButton; 4] = [FormButton::Mobile, FormButton::Desktop, FormButton::Console, FormButton::Headset];

/// The right side for a device (None: Current device), as Studio shows it.
pub fn right_buttons(device: Option<&Device>) -> Vec<BarButton> {
    use BarButton::*;
    let Some(d) = device else { return vec![Controller, Lan, Auto] };
    let mut v = Vec::new();
    if d.rotates() {
        v.push(Rotate);
        if d.platform == Platform::Android {
            v.push(Back);
        }
        v.push(Touch);
    }
    v.extend([Controller, Scaling, Lan, Quality]);
    v
}

/// The left side: the form buttons in their fixed order, the current one
/// replaced in place by the device pill.
pub fn left_slots(current: FormButton) -> Vec<LeftSlot> {
    FORMS.iter().map(|&b| if b == current { LeftSlot::Pill } else { LeftSlot::Form(b) }).collect()
}

pub fn form_icon(b: FormButton) -> &'static str {
    match b {
        FormButton::Mobile => "device-mobile",
        FormButton::Desktop => "device-laptop",
        FormButton::Console => "device-gamepad-2",
        FormButton::Headset => "device-vision-pro",
    }
}

pub fn form_key(b: FormButton) -> &'static str {
    match b {
        FormButton::Mobile => "mobile",
        FormButton::Desktop => "desktop",
        FormButton::Console => "console",
        FormButton::Headset => "headset",
    }
}

fn form_tip(b: FormButton) -> &'static str {
    match b {
        FormButton::Mobile => "Switch to Phone",
        FormButton::Desktop => "Switch to Desktop",
        FormButton::Console => "Switch to Console",
        FormButton::Headset => "Switch to Headset",
    }
}

fn text_font() -> FontId {
    FontId::proportional(mech::DEVICE_BAR_TEXT)
}

/// An icon button; greyed when not `enabled`, lit when `on`.
fn icon_button(ui: &mut egui::Ui, r: Rect, id: &str, icon: &str, enabled: bool, on: bool, tip: &str) -> egui::Response {
    let resp = ui.interact(r, ui.id().with(("devsim", id)), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if on {
        p.rect_filled(r.shrink(1.0), 4.0, theme::ACTIVE_TAB);
    } else if enabled && resp.hovered() {
        p.rect_filled(r.shrink(1.0), 4.0, theme::HOVER_BG);
    }
    let col = if !enabled { theme::TEXT.gamma_multiply(0.35) } else if on { theme::ACCENT } else { theme::TEXT };
    let s = mech::DEVICE_BAR_ICON;
    rustudio_engine::ti::paint(p, Rect::from_center_size(r.center(), vec2(s, s)), icon, col);
    resp.on_hover_text(tip)
}

/// A rounded pill: [icon] text [chevron]; light while its menu is open.
fn pill(ui: &mut egui::Ui, at: egui::Pos2, id: &str, icon: &str, text: &str, open: bool, dim: bool) -> egui::Response {
    let font = text_font();
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, Color32::WHITE);
    let (pad, icon_w, gap) = (8.0, mech::DEVICE_BAR_ICON, 5.0);
    let w = pad + icon_w + gap + galley.size().x + gap + 10.0 + pad;
    let h = mech::DEVICE_PILL_HEIGHT;
    let r = Rect::from_min_size(pos2(at.x, at.y - h / 2.0), vec2(w, h));
    let resp = ui.interact(r, ui.id().with(("devsim", id)), Sense::click());
    let p = ui.painter();
    let (bg, fg) = if open {
        (theme::DEVICE_PILL_OPEN_BG, Color32::from_rgb(25, 26, 31))
    } else if resp.hovered() {
        (theme::ACTIVE_TAB, theme::TEXT)
    } else {
        (theme::DEVICE_PILL_BG, theme::TEXT)
    };
    let fg = if dim && !open { theme::TEXT_DIM } else { fg };
    p.rect_filled(r, CornerRadius::same((h / 2.0) as u8), bg);
    let mut x = r.min.x + pad;
    rustudio_engine::ti::paint(p, Rect::from_min_size(pos2(x, r.center().y - icon_w / 2.0), vec2(icon_w, icon_w)), icon, fg);
    x += icon_w + gap;
    p.text(pos2(x, r.center().y), Align2::LEFT_CENTER, text, text_font(), fg);
    x += galley.size().x + gap;
    rustudio_engine::ti::paint(p, Rect::from_min_size(pos2(x, r.center().y - 5.0), vec2(10.0, 10.0)), "chevron-down", fg);
    resp
}

/// Draws the bar and returns the actions it asks for (studio_actions.rs).
pub fn toolbar(ui: &mut egui::Ui, bar: Rect, sim: &DeviceSim, playing: bool, rects: &mut HashMap<&'static str, Rect>) -> Vec<String> {
    let mut out = Vec::new();
    ui.painter().rect_filled(bar, 0.0, theme::DEVICE_BAR_BG);
    let y = bar.center().y;
    let step = mech::DEVICE_BAR_STEP;
    let device = sim.current();
    // ---- left
    let mut x = bar.min.x + 4.0;
    for slot in left_slots(sim.form()) {
        match slot {
            LeftSlot::Pill => {
                let name = device.map_or("Current device", |d| d.name.as_str());
                let resp = pill(ui, pos2(x, y), "pill", form_icon(sim.form()), name, sim.popup == Some(Popup::Devices), false);
                rects.insert("devsim.pill", resp.rect);
                if resp.clicked() {
                    out.push("device_popup:devices".into());
                }
                x = resp.rect.max.x + 4.0;
            }
            LeftSlot::Form(b) => {
                let r = Rect::from_center_size(pos2(x + step / 2.0, y), vec2(step, mech::DEVICE_PILL_HEIGHT));
                let key = match b {
                    FormButton::Mobile => "devsim.form.mobile",
                    FormButton::Desktop => "devsim.form.desktop",
                    FormButton::Console => "devsim.form.console",
                    FormButton::Headset => "devsim.form.headset",
                };
                if icon_button(ui, r, key, form_icon(b), true, false, form_tip(b)).clicked() {
                    out.push(format!("device_form:{}", form_key(b)));
                }
                rects.insert(key, r);
                x += step;
            }
        }
    }
    // ---- right, laid out from the right edge
    let buttons = right_buttons(device);
    let mut x = bar.max.x - 4.0;
    let q_text = device.map_or("Auto".to_string(), |d| format!("Q{}", d.quality));
    for b in buttons.iter().rev() {
        match b {
            BarButton::Lan | BarButton::Quality | BarButton::Auto => {
                let (id, icon, text, which, key) = match b {
                    BarButton::Lan => ("lan", "arrows-horizontal", "LAN".to_string(), Popup::Lan, "devsim.lan"),
                    _ => ("quality", "brand-speedtest", q_text.clone(), Popup::Quality, "devsim.quality"),
                };
                // (measured first, then placed: it grows from the right)
                let font = text_font();
                let tw = ui.painter().layout_no_wrap(text.clone(), font, Color32::WHITE).size().x;
                let w = 8.0 + mech::DEVICE_BAR_ICON + 5.0 + tw + 5.0 + 10.0 + 8.0;
                let resp = pill(ui, pos2(x - w, y), id, icon, &text, sim.popup == Some(which), true);
                rects.insert(key, resp.rect);
                if resp.clicked() {
                    out.push(format!("device_popup:{id}"));
                }
                x = resp.rect.min.x - 4.0;
            }
            _ => {
                let r = Rect::from_center_size(pos2(x - step / 2.0, y), vec2(step, mech::DEVICE_PILL_HEIGHT));
                let (key, icon, tip, enabled, on, action) = match b {
                    BarButton::Rotate => ("devsim.rotate", "rotate-dot", "Rotate", true, false, "device_rotate"),
                    // (Back is dim while editing: it opens the in-game menu)
                    BarButton::Back => ("devsim.back", "circle-chevron-left", "Mock Android back button", playing, false, "device_back"),
                    BarButton::Touch => ("devsim.touch", "hand-click", "Touch controls", true, sim.popup == Some(Popup::Touch), "device_popup:touch"),
                    BarButton::Controller => ("devsim.controller", "rs-virtual-controller", "Virtual controller", true, sim.controller, "device_controller"),
                    _ => ("devsim.scaling", "adjustments-horizontal", "Display scaling", true, sim.popup == Some(Popup::Scaling), "device_popup:scaling"),
                };
                if icon_button(ui, r, key, icon, enabled, on, tip).clicked() {
                    out.push(action.into());
                }
                rects.insert(key, r);
                x -= step;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustudio_core::devices::preset;

    #[test]
    fn bar_buttons_follow_the_device() {
        use BarButton::*;
        let r = |id| right_buttons(Some(preset(id).unwrap()));
        assert_eq!(r("iphone_14"), [Rotate, Touch, Controller, Scaling, Lan, Quality]);
        assert_eq!(r("samsung_galaxy_a16"), [Rotate, Back, Touch, Controller, Scaling, Lan, Quality]);
        assert_eq!(r("ipad_10th_generation"), [Rotate, Touch, Controller, Scaling, Lan, Quality]);
        assert_eq!(r("xbox"), [Controller, Scaling, Lan, Quality]);
        assert_eq!(r("hd_1080"), [Controller, Scaling, Lan, Quality]);
        assert_eq!(right_buttons(None), [Controller, Lan, Auto]);
        use LeftSlot::*;
        assert_eq!(left_slots(FormButton::Console), [Form(FormButton::Mobile), Form(FormButton::Desktop), Pill, Form(FormButton::Headset)]);
    }
}

//! The Device Simulator's virtual controller (Studio's Controller Emulator,
//! "Generic Gamepad" map): while it is on, these keys are a gamepad, not
//! keys. Left stick W/A/S/D (F: L3); d-pad 1 up, 2 left, 3 down, 4 right;
//! 7 Y, 8 X, 9 A, 0 B; right stick I/J/K/L (H: R3); E L2, R L1, P R2, O R1;
//! Q Select, U Start. Events as measured in Studio with real keys: sticks
//! send "Change" while held, each axis -1 / 0 / 1 (not normalised), and
//! "End" at (0, 0, 0) on release, never "Begin"; buttons "Begin" at
//! (0, 0, 1) and "End" at (0, 0, 0); the triggers (L2, R2) "Change" then
//! "Begin" at (0, 0, 1), and "Change" then "End" at (0, 0, 0). An unmapped
//! key reaches nothing while the controller is on (measured with G), except
//! Escape, which still opens the in-game menu (estimate: kept so the menu
//! stays reachable); a focused TextBox keeps every key (estimate).

use std::collections::HashSet;

use glam::Vec3;

use crate::gamepad::Input;

const BUTTONS: &[(&str, &str)] = &[
    ("F", "ButtonL3"),
    ("H", "ButtonR3"),
    ("One", "DPadUp"),
    ("Two", "DPadLeft"),
    ("Three", "DPadDown"),
    ("Four", "DPadRight"),
    ("Seven", "ButtonY"),
    ("Eight", "ButtonX"),
    ("Nine", "ButtonA"),
    ("Zero", "ButtonB"),
    ("R", "ButtonL1"),
    ("O", "ButtonR1"),
    ("Q", "ButtonSelect"),
    ("U", "ButtonStart"),
];
const TRIGGERS: &[(&str, &str)] = &[("E", "ButtonL2"), ("P", "ButtonR2")];
/// (stick, up, left, down, right)
const STICKS: &[(&str, &str, &str, &str, &str)] = &[("Thumbstick1", "W", "A", "S", "D"), ("Thumbstick2", "I", "J", "K", "L")];

/// Whether a key (Roblox KeyCode name) is one of the controller's.
pub fn is_mapped(key: &str) -> bool {
    BUTTONS.iter().chain(TRIGGERS).any(|(k, _)| *k == key)
        || STICKS.iter().any(|(_, u, l, d, r)| [u, l, d, r].contains(&&key))
}

#[derive(Clone, Debug, Default)]
pub struct VirtualPad {
    pub on: bool,
    /// The gamepad slot it drives (Gamepad1: gamepad.rs `reserve`).
    pub slot: Option<usize>,
    /// A mapped key was used since it was turned on: PreferredInput becomes
    /// Gamepad then (measured: not when the panel opens).
    pub used: bool,
    /// The game heard the pad last frame (window focused, no menu, the
    /// client's view): while it doesn't, held keys let go and stay let go.
    pub heard: bool,
    held: HashSet<String>,
    sticks: [[i8; 2]; 2],
}

impl VirtualPad {
    pub fn set_on(&mut self, on: bool) {
        if self.on != on {
            *self = Self { on, slot: self.slot, heard: self.heard, ..Default::default() };
        }
    }

    /// Takes the controller's keys out of this frame's keys (held, began,
    /// ended: names and gameProcessedEvent) and returns the gamepad input
    /// they make. Unmapped keys go too, while it's on; with a TextBox
    /// focused (`typing`) nothing is taken, and what was held lets go.
    pub fn take(&mut self, keys: &mut HashSet<String>, began: &mut Vec<(String, bool)>, ended: &mut Vec<(String, bool)>, typing: bool) -> Vec<Input> {
        if !self.on {
            return Vec::new();
        }
        if typing || !self.heard {
            return self.release_all();
        }
        let now: HashSet<String> = keys.iter().filter(|k| is_mapped(k)).cloned().collect();
        let tapped: Vec<String> = began
            .iter()
            .filter(|(k, _)| is_mapped(k) && !now.contains(k) && ended.iter().any(|(e, _)| e == k))
            .map(|(k, _)| k.clone())
            .collect();
        let out = self.step(now, &tapped);
        // (the keyboard hears none of it, Escape aside)
        keys.retain(|k| k == "Escape");
        began.retain(|(k, _)| k == "Escape");
        ended.retain(|(k, _)| k == "Escape");
        out
    }

    /// Lets go of every held key (the window lost focus, the game stopped
    /// hearing the pad, a TextBox took the keys), as a real pad does.
    pub fn release_all(&mut self) -> Vec<Input> {
        let used = self.used;
        let out = self.step(HashSet::new(), &[]);
        self.used = used;
        out
    }

    /// The input from the held keys `now` and this frame's taps.
    fn step(&mut self, now: HashSet<String>, tapped: &[String]) -> Vec<Input> {
        let mut out = Vec::new();
        let slot = self.slot.unwrap_or(0);
        let ev = |key: &'static str, state: &'static str, position: Vec3| Input { slot, key, state, position, delta: glam::Vec2::ZERO };
        let down = |k: &str| now.contains(k);
        let was = |k: &str, held: &HashSet<String>| held.contains(k);
        for &(k, name) in BUTTONS {
            let (d, w) = (down(k), was(k, &self.held));
            if d && !w {
                out.push(ev(name, "Begin", Vec3::Z));
            } else if !d && w {
                out.push(ev(name, "End", Vec3::ZERO));
            } else if !d && tapped.iter().any(|t| t == k) {
                out.push(ev(name, "Begin", Vec3::Z));
                out.push(ev(name, "End", Vec3::ZERO));
            }
        }
        for &(k, name) in TRIGGERS {
            let (d, w) = (down(k), was(k, &self.held));
            let press = |out: &mut Vec<Input>| {
                out.push(ev(name, "Change", Vec3::Z));
                out.push(ev(name, "Begin", Vec3::Z));
            };
            let release = |out: &mut Vec<Input>| {
                out.push(ev(name, "Change", Vec3::ZERO));
                out.push(ev(name, "End", Vec3::ZERO));
            };
            if d && !w {
                press(&mut out);
            } else if !d && w {
                release(&mut out);
            } else if !d && tapped.iter().any(|t| t == k) {
                press(&mut out);
                release(&mut out);
            }
        }
        for (i, &(name, u, l, dn, r)) in STICKS.iter().enumerate() {
            let axis = |plus: &str, minus: &str| i8::from(down(plus)) - i8::from(down(minus));
            let v = [axis(r, l), axis(u, dn)];
            if v != self.sticks[i] {
                let p = Vec3::new(f32::from(v[0]), f32::from(v[1]), 0.0);
                out.push(ev(name, if v == [0, 0] { "End" } else { "Change" }, p));
                self.sticks[i] = v;
            }
        }
        if !now.is_empty() || !tapped.is_empty() {
            self.used = true;
        }
        self.held = now;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(p: &mut VirtualPad, held: &[&str], began: &[&str], ended: &[&str]) -> (Vec<(String, String, Vec3)>, usize) {
        let mut keys: HashSet<String> = held.iter().map(|s| s.to_string()).collect();
        let mut b: Vec<(String, bool)> = began.iter().map(|s| (s.to_string(), false)).collect();
        let mut e: Vec<(String, bool)> = ended.iter().map(|s| (s.to_string(), false)).collect();
        let out = p.take(&mut keys, &mut b, &mut e, false);
        (out.into_iter().map(|i| (i.key.to_string(), i.state.to_string(), i.position)).collect(), keys.len() + b.len() + e.len())
    }

    fn on() -> VirtualPad {
        let mut p = VirtualPad { heard: true, ..Default::default() };
        p.set_on(true);
        p
    }

    #[test]
    fn w_moves_thumbstick1_up() {
        let mut p = on();
        let (ev, left) = frame(&mut p, &["W"], &["W"], &[]);
        assert_eq!(ev, [("Thumbstick1".into(), "Change".into(), Vec3::new(0.0, 1.0, 0.0))]);
        assert_eq!(left, 0);
        let (ev, _) = frame(&mut p, &[], &[], &["W"]);
        assert_eq!(ev, [("Thumbstick1".into(), "End".into(), Vec3::ZERO)]);
        assert!(p.used);
    }

    #[test]
    fn w_and_d_are_not_normalised() {
        let mut p = on();
        let (ev, _) = frame(&mut p, &["W", "D"], &["W", "D"], &[]);
        assert_eq!(ev[0].2, Vec3::new(1.0, 1.0, 0.0));
    }

    #[test]
    fn nine_is_button_a_with_z() {
        let mut p = on();
        assert_eq!(frame(&mut p, &["Nine"], &["Nine"], &[]).0, [("ButtonA".into(), "Begin".into(), Vec3::Z)]);
        assert_eq!(frame(&mut p, &[], &[], &["Nine"]).0, [("ButtonA".into(), "End".into(), Vec3::ZERO)]);
    }

    #[test]
    fn e_is_l2_change_then_begin() {
        let mut p = on();
        let (ev, _) = frame(&mut p, &["E"], &["E"], &[]);
        assert_eq!(ev, [("ButtonL2".into(), "Change".into(), Vec3::Z), ("ButtonL2".into(), "Begin".into(), Vec3::Z)]);
        let (ev, _) = frame(&mut p, &[], &[], &["E"]);
        assert_eq!(ev, [("ButtonL2".into(), "Change".into(), Vec3::ZERO), ("ButtonL2".into(), "End".into(), Vec3::ZERO)]);
    }

    #[test]
    fn a_tap_within_one_frame_presses_and_lets_go() {
        let mut p = on();
        let (ev, _) = frame(&mut p, &[], &["Nine"], &["Nine"]);
        assert_eq!(ev.iter().map(|e| e.1.as_str()).collect::<Vec<_>>(), ["Begin", "End"]);
    }

    #[test]
    fn unmapped_keys_are_dropped_while_on() {
        let mut p = on();
        let (ev, left) = frame(&mut p, &["G"], &["G"], &[]);
        assert!(ev.is_empty());
        assert_eq!(left, 0);
        assert!(!p.used);
    }

    #[test]
    fn escape_still_reaches_the_game() {
        let mut p = on();
        let (ev, left) = frame(&mut p, &["Escape"], &["Escape"], &[]);
        assert!(ev.is_empty());
        assert_eq!(left, 2);
    }

    #[test]
    fn focused_textbox_keeps_mapped_keys() {
        let mut p = on();
        let mut keys: HashSet<String> = ["W".to_string()].into();
        let mut b = vec![("W".to_string(), true)];
        let mut e = Vec::new();
        assert!(p.take(&mut keys, &mut b, &mut e, true).is_empty());
        assert_eq!((keys.len(), b.len()), (1, 1));
    }

    #[test]
    fn a_textbox_taking_the_keys_lets_go_of_the_stick() {
        let mut p = on();
        frame(&mut p, &["W", "Nine"], &["W", "Nine"], &[]);
        let mut keys: HashSet<String> = ["W".to_string(), "Nine".to_string()].into();
        let out = p.take(&mut keys, &mut Vec::new(), &mut Vec::new(), true);
        let ev: Vec<_> = out.iter().map(|i| (i.key, i.state)).collect();
        assert_eq!(ev, [("ButtonA", "End"), ("Thumbstick1", "End")]);
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn release_all_lets_go_of_everything_held() {
        let mut p = on();
        frame(&mut p, &["W", "E"], &["W", "E"], &[]);
        let ev: Vec<_> = p.release_all().iter().map(|i| (i.key, i.state, i.position)).collect();
        assert_eq!(ev, [("ButtonL2", "Change", Vec3::ZERO), ("ButtonL2", "End", Vec3::ZERO), ("Thumbstick1", "End", Vec3::ZERO)]);
        assert!(p.release_all().is_empty());
        assert!(p.used);
    }

    #[test]
    fn off_takes_nothing() {
        let mut p = VirtualPad::default();
        let (ev, left) = frame(&mut p, &["W"], &["W"], &[]);
        assert!(ev.is_empty());
        assert_eq!(left, 2);
    }
}

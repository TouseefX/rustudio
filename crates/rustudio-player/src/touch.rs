//! Mouse-as-touch for simulated phones and tablets (Studio's Device
//! Simulator): a left press is a finger; LeftAlt + drag adds a second finger
//! mirrored about the screen's centre (pinch and rotate); LeftAlt + LeftShift
//! + drag moves two fingers together (two-finger pan). Positions are GUI
//! coordinates, as InputObject.Position.

use egui::{pos2, Pos2, Rect, Vec2};
use rustudio_core::mech;

/// One frame of the pointer over the device screen.
#[derive(Clone, Copy, Debug)]
pub struct PointerSample {
    pub t: f64,
    /// Where the pointer is, in GUI coordinates (None: off the screen).
    pub pos: Option<Pos2>,
    pub down: bool,
    pub alt: bool,
    pub shift: bool,
    /// The whole device screen's centre and extent, in GUI coordinates.
    pub centre: Pos2,
    pub screen: Rect,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GestureState {
    Begin,
    Change,
    End,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwipeDirection {
    Right,
    Left,
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TouchEvent {
    Began { id: u32, pos: Pos2 },
    Moved { id: u32, pos: Pos2, delta: Vec2 },
    Ended { id: u32, pos: Pos2 },
    Tap { positions: Vec<Pos2> },
    LongPress { positions: Vec<Pos2>, state: GestureState },
    Pan { positions: Vec<Pos2>, translation: Vec2, velocity: Vec2, state: GestureState },
    Swipe { direction: SwipeDirection, touches: u32 },
    Pinch { positions: Vec<Pos2>, scale: f32, velocity: f32, state: GestureState },
    Rotate { positions: Vec<Pos2>, rotation: f32, velocity: f32, state: GestureState },
}

#[derive(Clone, Debug)]
enum Mode {
    Idle,
    One { id: u32, start: Pos2, at: Pos2, t0: f64, last_t: f64, moved: bool, panning: bool, long_press: bool },
    /// LeftAlt: the mirrored finger (first) and the pointer's.
    Pinch { ids: [u32; 2], at: [Pos2; 2], start_dist: f32, start_angle: f32, t0: f64, last_t: f64, last_scale: f32, last_rot: f32, pinching: bool, rotating: bool, long_press: bool },
    /// LeftAlt + LeftShift: both fingers move with the pointer.
    TwoPan { ids: [u32; 2], at: [Pos2; 2], start: Pos2, pointer: Pos2, last_t: f64, panning: bool },
}

#[derive(Debug)]
pub struct TouchEmu {
    mode: Mode,
    next_id: u32,
}

impl Default for TouchEmu {
    fn default() -> Self {
        Self { mode: Mode::Idle, next_id: 1 }
    }
}

fn angle(a: Pos2, b: Pos2) -> f32 {
    let d = a - b;
    d.y.atan2(d.x)
}

fn mirror(p: Pos2, centre: Pos2, screen: Rect) -> Pos2 {
    let m = pos2(2.0 * centre.x - p.x, 2.0 * centre.y - p.y);
    if screen.is_positive() { screen.clamp(m) } else { m }
}

impl TouchEmu {
    /// The fingers down now (for the markers), first finger first.
    pub fn fingers(&self) -> Vec<Pos2> {
        match &self.mode {
            Mode::Idle => Vec::new(),
            Mode::One { at, .. } => vec![*at],
            Mode::Pinch { at, .. } | Mode::TwoPan { at, .. } => at.to_vec(),
        }
    }

    /// The touch id the pointer's finger has (or gets if pressed now: with
    /// LeftAlt the mirrored finger takes the first id).
    pub fn pointer_id(&self, alt: bool) -> u32 {
        match &self.mode {
            Mode::Idle => if alt { self.next_id.wrapping_add(1).max(1) } else { self.next_id },
            Mode::One { id, .. } => *id,
            Mode::Pinch { ids, .. } | Mode::TwoPan { ids, .. } => ids[1],
        }
    }

    pub fn two_fingers(&self) -> bool {
        matches!(self.mode, Mode::Pinch { .. } | Mode::TwoPan { .. })
    }

    fn id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    pub fn step(&mut self, s: PointerSample) -> Vec<TouchEvent> {
        let mut out = Vec::new();
        let mode = std::mem::replace(&mut self.mode, Mode::Idle);
        self.mode = match (mode, s.down, s.pos) {
            // ---- a new press
            (Mode::Idle, true, Some(p)) => {
                if s.alt {
                    let m = mirror(p, s.centre, s.screen);
                    let ids = [self.id(), self.id()];
                    out.push(TouchEvent::Began { id: ids[0], pos: m });
                    out.push(TouchEvent::Began { id: ids[1], pos: p });
                    if s.shift {
                        Mode::TwoPan { ids, at: [m, p], start: p, pointer: p, last_t: s.t, panning: false }
                    } else {
                        Mode::Pinch { ids, at: [m, p], start_dist: (p - m).length().max(1e-3), start_angle: angle(m, p), t0: s.t, last_t: s.t, last_scale: 1.0, last_rot: 0.0, pinching: false, rotating: false, long_press: false }
                    }
                } else {
                    let id = self.id();
                    out.push(TouchEvent::Began { id, pos: p });
                    Mode::One { id, start: p, at: p, t0: s.t, last_t: s.t, moved: false, panning: false, long_press: false }
                }
            }
            (Mode::Idle, _, _) => Mode::Idle,
            // ---- one finger
            (Mode::One { id, start, at, t0, last_t, mut moved, mut panning, mut long_press }, true, pos) => {
                let p = pos.unwrap_or(at);
                let dt = (s.t - last_t).max(1e-4) as f32;
                if p != at {
                    out.push(TouchEvent::Moved { id, pos: p, delta: p - at });
                    if long_press {
                        out.push(TouchEvent::LongPress { positions: vec![p], state: GestureState::Change });
                    }
                    if !moved && (p - start).length() >= mech::TOUCH_TAP_SLOP {
                        moved = true;
                    }
                    if moved && !long_press {
                        let state = if panning { GestureState::Change } else { GestureState::Begin };
                        panning = true;
                        out.push(TouchEvent::Pan { positions: vec![p], translation: p - start, velocity: (p - at) / dt, state });
                    }
                }
                if !moved && !long_press && s.t - t0 >= mech::TOUCH_LONG_PRESS_S - 1e-9 {
                    long_press = true;
                    out.push(TouchEvent::LongPress { positions: vec![p], state: GestureState::Begin });
                }
                Mode::One { id, start, at: p, t0, last_t: if p != at { s.t } else { last_t }, moved, panning, long_press }
            }
            (Mode::One { id, start, at, t0, moved, panning, long_press, .. }, false, pos) => {
                let p = pos.unwrap_or(at);
                out.push(TouchEvent::Ended { id, pos: p });
                let translation = p - start;
                if panning {
                    out.push(TouchEvent::Pan { positions: vec![p], translation, velocity: Vec2::ZERO, state: GestureState::End });
                    let secs = (s.t - t0).max(1e-3) as f32;
                    if translation.length() / secs >= mech::TOUCH_SWIPE_MIN_SPEED {
                        let direction = if translation.x.abs() >= translation.y.abs() {
                            if translation.x >= 0.0 { SwipeDirection::Right } else { SwipeDirection::Left }
                        } else if translation.y >= 0.0 {
                            SwipeDirection::Down
                        } else {
                            SwipeDirection::Up
                        };
                        out.push(TouchEvent::Swipe { direction, touches: 1 });
                    }
                }
                if long_press {
                    out.push(TouchEvent::LongPress { positions: vec![p], state: GestureState::End });
                } else if !moved {
                    out.push(TouchEvent::Tap { positions: vec![p] });
                }
                Mode::Idle
            }
            // ---- LeftAlt: pinch and rotate
            // (a still hold of 0.5 s is a long press too, listing only the
            // first touch, with Change on every move; measured)
            (Mode::Pinch { ids, at, start_dist, start_angle, t0, last_t, last_scale, last_rot, mut pinching, mut rotating, mut long_press }, true, pos) => {
                let p = pos.unwrap_or(at[1]);
                let begin_long_press = |long_press: &mut bool, first: Pos2, out: &mut Vec<TouchEvent>| {
                    if !*long_press && s.t - t0 >= mech::TOUCH_LONG_PRESS_S - 1e-9 {
                        *long_press = true;
                        out.push(TouchEvent::LongPress { positions: vec![first], state: GestureState::Begin });
                    }
                };
                if p == at[1] {
                    begin_long_press(&mut long_press, at[0], &mut out);
                    Mode::Pinch { ids, at, start_dist, start_angle, t0, last_t, last_scale, last_rot, pinching, rotating, long_press }
                } else {
                    let m = mirror(p, s.centre, s.screen);
                    let dt = (s.t - last_t).max(1e-4) as f32;
                    out.push(TouchEvent::Moved { id: ids[0], pos: m, delta: m - at[0] });
                    out.push(TouchEvent::Moved { id: ids[1], pos: p, delta: p - at[1] });
                    let positions = vec![m, p];
                    let scale = (p - m).length() / start_dist;
                    let state = if pinching { GestureState::Change } else { GestureState::Begin };
                    pinching = true;
                    out.push(TouchEvent::Pinch { positions: positions.clone(), scale, velocity: (scale - last_scale) / dt, state });
                    // (radians, not wrapped: Studio's emulator jumps past pi)
                    let rotation = angle(m, p) - start_angle;
                    if rotating || rotation.abs() > mech::TOUCH_ROTATE_START_RAD {
                        let state = if rotating { GestureState::Change } else { GestureState::Begin };
                        rotating = true;
                        out.push(TouchEvent::Rotate { positions, rotation, velocity: (rotation - last_rot) / dt, state });
                    }
                    if long_press {
                        out.push(TouchEvent::LongPress { positions: vec![m], state: GestureState::Change });
                    }
                    begin_long_press(&mut long_press, m, &mut out);
                    Mode::Pinch { ids, at: [m, p], start_dist, start_angle, t0, last_t: s.t, last_scale: scale, last_rot: rotation, pinching, rotating, long_press }
                }
            }
            (Mode::Pinch { ids, at, last_scale, last_rot, pinching, rotating, long_press, .. }, false, _) => {
                out.push(TouchEvent::Ended { id: ids[0], pos: at[0] });
                out.push(TouchEvent::Ended { id: ids[1], pos: at[1] });
                if pinching {
                    out.push(TouchEvent::Pinch { positions: at.to_vec(), scale: last_scale, velocity: 0.0, state: GestureState::End });
                }
                if rotating {
                    out.push(TouchEvent::Rotate { positions: at.to_vec(), rotation: last_rot, velocity: 0.0, state: GestureState::End });
                }
                if long_press {
                    out.push(TouchEvent::LongPress { positions: vec![at[0]], state: GestureState::End });
                }
                Mode::Idle
            }
            // ---- LeftAlt + LeftShift: two fingers pan together
            (Mode::TwoPan { ids, at, start, pointer, last_t, mut panning }, true, pos) => {
                let p = pos.unwrap_or(pointer);
                if p == pointer {
                    Mode::TwoPan { ids, at, start, pointer, last_t, panning }
                } else {
                    let d = p - pointer;
                    let dt = (s.t - last_t).max(1e-4) as f32;
                    let now = [at[0] + d, at[1] + d];
                    out.push(TouchEvent::Moved { id: ids[0], pos: now[0], delta: d });
                    out.push(TouchEvent::Moved { id: ids[1], pos: now[1], delta: d });
                    let state = if panning { GestureState::Change } else { GestureState::Begin };
                    panning = true;
                    out.push(TouchEvent::Pan { positions: now.to_vec(), translation: p - start, velocity: d / dt, state });
                    Mode::TwoPan { ids, at: now, start, pointer: p, last_t: s.t, panning }
                }
            }
            (Mode::TwoPan { ids, at, start, pointer, panning, .. }, false, _) => {
                out.push(TouchEvent::Ended { id: ids[0], pos: at[0] });
                out.push(TouchEvent::Ended { id: ids[1], pos: at[1] });
                if panning {
                    // (measured: a last Change with both, then End with one)
                    out.push(TouchEvent::Pan { positions: at.to_vec(), translation: pointer - start, velocity: Vec2::ZERO, state: GestureState::Change });
                    out.push(TouchEvent::Pan { positions: vec![at[1]], translation: pointer - start, velocity: Vec2::ZERO, state: GestureState::End });
                }
                Mode::Idle
            }
        };
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    // iPhone 14 landscape, GUI coordinates (measured centre)
    fn sample(t: f64, p: Option<(f32, f32)>, down: bool, alt: bool, shift: bool) -> PointerSample {
        PointerSample {
            t,
            pos: p.map(|(x, y)| pos2(x, y)),
            down,
            alt,
            shift,
            centre: pos2(374.5, 136.5),
            screen: Rect::from_min_max(pos2(-47.0, -58.0), pos2(796.0, 331.0)),
        }
    }

    fn kinds(ev: &[TouchEvent]) -> Vec<&'static str> {
        ev.iter()
            .map(|e| match e {
                TouchEvent::Began { .. } => "Began",
                TouchEvent::Moved { .. } => "Moved",
                TouchEvent::Ended { .. } => "Ended",
                TouchEvent::Tap { .. } => "Tap",
                TouchEvent::LongPress { .. } => "LongPress",
                TouchEvent::Pan { .. } => "Pan",
                TouchEvent::Swipe { .. } => "Swipe",
                TouchEvent::Pinch { .. } => "Pinch",
                TouchEvent::Rotate { .. } => "Rotate",
            })
            .collect()
    }

    #[test]
    fn tap_is_began_ended_tap() {
        let mut t = TouchEmu::default();
        assert_eq!(kinds(&t.step(sample(0.0, Some((100.0, 100.0)), true, false, false))), ["Began"]);
        assert_eq!(kinds(&t.step(sample(0.1, Some((100.0, 100.0)), false, false, false))), ["Ended", "Tap"]);
    }

    #[test]
    fn hover_sends_nothing() {
        let mut t = TouchEmu::default();
        assert!(t.step(sample(0.0, Some((100.0, 100.0)), false, false, false)).is_empty());
        assert!(t.step(sample(0.1, Some((150.0, 100.0)), false, false, false)).is_empty());
    }

    #[test]
    fn drag_pans_then_swipes_right() {
        let mut t = TouchEmu::default();
        t.step(sample(0.0, Some((100.0, 100.0)), true, false, false));
        let e = t.step(sample(0.05, Some((160.0, 102.0)), true, false, false));
        assert_eq!(kinds(&e), ["Moved", "Pan"]);
        assert!(matches!(e[1], TouchEvent::Pan { state: GestureState::Begin, .. }));
        let e = t.step(sample(0.1, Some((220.0, 104.0)), true, false, false));
        assert!(matches!(e[1], TouchEvent::Pan { state: GestureState::Change, .. }));
        let e = t.step(sample(0.12, Some((220.0, 104.0)), false, false, false));
        assert_eq!(kinds(&e), ["Ended", "Pan", "Swipe"]);
        assert!(matches!(e[2], TouchEvent::Swipe { direction: SwipeDirection::Right, touches: 1 }));
    }

    #[test]
    fn hold_fires_long_press_after_half_a_second() {
        let mut t = TouchEmu::default();
        t.step(sample(0.0, Some((100.0, 100.0)), true, false, false));
        assert!(t.step(sample(0.49, Some((100.0, 100.0)), true, false, false)).is_empty());
        let e = t.step(sample(0.5, Some((100.0, 100.0)), true, false, false));
        assert_eq!(e, [TouchEvent::LongPress { positions: vec![pos2(100.0, 100.0)], state: GestureState::Begin }]);
        let e = t.step(sample(0.6, Some((101.0, 100.0)), true, false, false));
        assert_eq!(kinds(&e), ["Moved", "LongPress"]);
        assert_eq!(kinds(&t.step(sample(0.7, Some((101.0, 100.0)), false, false, false))), ["Ended", "LongPress"]);
    }

    #[test]
    fn alt_drag_mirrors_about_the_screen_centre() {
        // measured: pointer (470, 79) -> second finger (279, 194), first
        let mut t = TouchEmu::default();
        let e = t.step(sample(0.0, Some((470.0, 79.0)), true, true, false));
        assert_eq!(e, [TouchEvent::Began { id: 1, pos: pos2(279.0, 194.0) }, TouchEvent::Began { id: 2, pos: pos2(470.0, 79.0) }]);
        let e = t.step(sample(0.1, Some((601.0, 177.0)), true, true, false));
        assert_eq!(e[0], TouchEvent::Moved { id: 1, pos: pos2(148.0, 96.0), delta: vec2(-131.0, -98.0) });
        let scale = e.iter().find_map(|e| match e { TouchEvent::Pinch { scale, state: GestureState::Begin, .. } => Some(*scale), _ => None }).unwrap();
        assert!((scale - 2.065).abs() < 0.01, "{scale}");
    }

    #[test]
    fn alt_drag_rotation_is_unwrapped_radians() {
        let mut t = TouchEmu::default();
        t.step(sample(0.0, Some((470.0, 79.0)), true, true, false));
        let rot = |e: &[TouchEvent]| e.iter().find_map(|e| match e { TouchEvent::Rotate { rotation, state, .. } => Some((*rotation, *state)), _ => None });
        let (r, st) = rot(&t.step(sample(0.1, Some((497.0, 79.0)), true, true, false))).unwrap();
        assert!((r - 0.103).abs() < 0.005 && st == GestureState::Begin, "{r}");
        let (r, _) = rot(&t.step(sample(0.2, Some((601.0, 164.0)), true, true, false))).unwrap();
        assert!((r + 5.62).abs() < 0.01, "{r}");
    }

    #[test]
    fn alt_drag_sends_no_pan_or_tap() {
        let mut t = TouchEmu::default();
        let mut all = t.step(sample(0.0, Some((470.0, 79.0)), true, true, false));
        all.extend(t.step(sample(0.1, Some((520.0, 90.0)), true, true, false)));
        all.extend(t.step(sample(0.9, Some((520.0, 90.0)), true, true, false)));
        all.extend(t.step(sample(1.0, Some((520.0, 90.0)), false, true, false)));
        assert!(kinds(&all).iter().all(|k| !matches!(*k, "Pan" | "Tap" | "Swipe")), "{:?}", kinds(&all));
    }

    #[test]
    fn alt_drag_long_press_lists_the_first_touch() {
        // measured: Begin after 0.5 s with only the first (mirrored) touch,
        // Change on every move
        let mut t = TouchEmu::default();
        t.step(sample(0.0, Some((470.0, 79.0)), true, true, false));
        assert!(!kinds(&t.step(sample(0.3, Some((470.0, 79.0)), true, true, false))).contains(&"LongPress"));
        let e = t.step(sample(0.5, Some((470.0, 79.0)), true, true, false));
        assert_eq!(e, [TouchEvent::LongPress { positions: vec![pos2(279.0, 194.0)], state: GestureState::Begin }]);
        let e = t.step(sample(0.6, Some((480.0, 79.0)), true, true, false));
        let lp: Vec<_> = e.iter().filter_map(|e| match e { TouchEvent::LongPress { positions, state } => Some((positions.len(), *state)), _ => None }).collect();
        assert_eq!(lp, [(1, GestureState::Change)]);
        assert_eq!(kinds(&t.step(sample(0.7, Some((480.0, 79.0)), false, true, false))).last(), Some(&"LongPress"));
    }

    #[test]
    fn alt_shift_drag_pans_two_fingers() {
        let mut t = TouchEmu::default();
        let e = t.step(sample(0.0, Some((470.0, 79.0)), true, true, true));
        assert_eq!(e[0], TouchEvent::Began { id: 1, pos: pos2(279.0, 194.0) });
        let e = t.step(sample(0.1, Some((480.0, 99.0)), true, true, true));
        assert_eq!(kinds(&e), ["Moved", "Moved", "Pan"]);
        assert_eq!(e[0], TouchEvent::Moved { id: 1, pos: pos2(289.0, 214.0), delta: vec2(10.0, 20.0) });
        let e = t.step(sample(0.2, Some((480.0, 99.0)), false, true, true));
        assert_eq!(kinds(&e), ["Ended", "Ended", "Pan", "Pan"]);
        assert!(matches!(&e[2], TouchEvent::Pan { positions, state: GestureState::Change, .. } if positions.len() == 2));
        assert!(matches!(&e[3], TouchEvent::Pan { positions, state: GestureState::End, .. } if positions.len() == 1));
    }

    #[test]
    fn portrait_mirrors_about_the_full_screen_centre() {
        // measured, iPhone 14 upright: pointer (61, 462) -> (328.3, 171) about (194.5, 316.5)
        let mut t = TouchEmu::default();
        let mut s = sample(0.0, Some((61.0, 462.0)), true, true, false);
        s.centre = pos2(194.5, 316.5);
        s.screen = Rect::from_min_max(pos2(0.0, -105.0), pos2(389.0, 738.0));
        let e = t.step(s);
        let TouchEvent::Began { pos, .. } = e[0] else { panic!() };
        assert!((pos - pos2(328.0, 171.0)).length() < 0.5, "{pos:?}");
    }
}

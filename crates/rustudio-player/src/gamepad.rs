//! Map host controllers to stable Roblox gamepad slots during Play.

use gilrs::{Axis, Button, Gamepad, GamepadId, Gilrs};
use glam::{Vec2, Vec3};

const BUTTONS: [(Button, &str); 16] = [
    (Button::South, "ButtonA"),
    (Button::East, "ButtonB"),
    (Button::West, "ButtonX"),
    (Button::North, "ButtonY"),
    (Button::LeftTrigger, "ButtonL1"),
    (Button::LeftTrigger2, "ButtonL2"),
    (Button::RightTrigger, "ButtonR1"),
    (Button::RightTrigger2, "ButtonR2"),
    (Button::LeftThumb, "ButtonL3"),
    (Button::RightThumb, "ButtonR3"),
    (Button::Start, "ButtonStart"),
    (Button::Select, "ButtonSelect"),
    (Button::DPadUp, "DPadUp"),
    (Button::DPadDown, "DPadDown"),
    (Button::DPadLeft, "DPadLeft"),
    (Button::DPadRight, "DPadRight"),
];
const STICKS: [(&str, Axis, Axis); 2] = [
    ("Thumbstick1", Axis::LeftStickX, Axis::LeftStickY),
    ("Thumbstick2", Axis::RightStickX, Axis::RightStickY),
];

#[derive(Default)]
pub struct Gamepads {
    host: Option<Gilrs>,
    slots: [Option<(GamepadId, State)>; 8],
    /// The slot a simulated device's own pad holds (Device Simulator):
    /// never handed to a host pad. Its GamepadConnected / Disconnected are
    /// fired by `fire_device_changes`, in Studio's order.
    reserved: Option<usize>,
    /// A host pad moved out of Gamepad1 for the simulated pad: (from, to),
    /// reported by the next poll as a disconnect and a connect.
    moved: Option<(usize, usize)>,
}

#[derive(Clone, Copy, Default)]
struct State {
    buttons: u32,
    sticks: [Vec2; 2],
}

pub struct Input {
    pub slot: usize,
    pub key: &'static str,
    pub state: &'static str,
    /// InputObject.Position: a stick's (x, y, 0); a button's Z is 1 while
    /// it is down on the virtual controller (measured), 0 from host pads.
    pub position: Vec3,
    pub delta: Vec2,
}

#[derive(Default)]
pub struct Frame {
    pub connected: u8,
    pub added: u8,
    pub removed: u8,
    /// A host pad left this slot for the simulated pad: its
    /// GamepadDisconnected comes before the simulated pad's GamepadConnected.
    pub vacated: u8,
    pub inputs: Vec<Input>,
}

impl Gamepads {
    pub fn new() -> Self {
        Self {
            host: Gilrs::new().ok(),
            ..Self::default()
        }
    }

    /// Holds (or frees) Gamepad1 for a simulated pad (a device's own, or
    /// the virtual controller): Studio's emulated pad is Gamepad1, and
    /// Roblox's PlayerModule moves the character from Gamepad1 only. A host
    /// pad already there moves to the first free slot (estimate: Studio
    /// not measured with a real pad plugged in). Returns the held slot.
    pub fn reserve(&mut self, on: bool) -> Option<usize> {
        if !on {
            self.reserved = None;
        } else if self.reserved.is_none() {
            if let Some(to) = vacate_first(&mut self.slots) {
                self.moved = Some((0, to));
            }
            self.reserved = self.slots[0].is_none().then_some(0);
        }
        self.reserved
    }

    pub fn poll(&mut self, active: bool) -> Frame {
        let mut frame = Frame::default();
        if let Some((from, to)) = self.moved.take() {
            frame.vacated |= 1 << from;
            frame.added |= 1 << to;
        }
        let reserved = self.reserved;
        if let Some(host) = &mut self.host {
            // Resample after each event so a press and release in one frame both
            // reach scripts. The first sample also handles focus and startup.
            Self::sample(host, &mut self.slots, reserved, active, &mut frame);
            while host.next_event().is_some() {
                Self::sample(host, &mut self.slots, reserved, active, &mut frame);
            }
        }
        frame.connected = self.connected();
        frame
    }

    fn connected(&self) -> u8 {
        self.slots.iter().enumerate().fold(0, |mask, (slot, entry)| {
            mask | (u8::from(entry.is_some() || self.reserved == Some(slot)) << slot)
        })
    }

    fn sample(
        host: &Gilrs,
        slots: &mut [Option<(GamepadId, State)>; 8],
        reserved: Option<usize>,
        active: bool,
        frame: &mut Frame,
    ) {
        for (slot, entry) in slots.iter_mut().enumerate() {
            if let Some((id, state)) = entry {
                let pad = host.gamepad(*id);
                let next = if active && pad.is_connected() {
                    State::read(pad)
                } else {
                    State::default()
                };
                state.update(next, slot, &mut frame.inputs);
                if !pad.is_connected() {
                    *entry = None;
                    frame.removed |= 1 << slot;
                }
            }
        }
        for (id, pad) in host.gamepads() {
            if slots.iter().flatten().any(|(existing, _)| *existing == id) {
                continue;
            }
            let Some(slot) = free_slot(slots, reserved) else {
                break;
            };
            let mut state = State::default();
            if active {
                state.update(State::read(pad), slot, &mut frame.inputs);
            }
            slots[slot] = Some((id, state));
            frame.added |= 1 << slot;
        }
    }
}

/// Moves whatever holds Gamepad1 to the first free slot; returns where it
/// went (None: Gamepad1 was free, or all eight are full).
fn vacate_first<T>(slots: &mut [Option<T>; 8]) -> Option<usize> {
    slots[0].as_ref()?;
    let to = slots.iter().skip(1).position(Option::is_none)? + 1;
    slots[to] = slots[0].take();
    Some(to)
}

/// The first slot a newly plugged-in host pad takes: never the simulated pad's.
fn free_slot<T>(slots: &[Option<T>; 8], reserved: Option<usize>) -> Option<usize> {
    slots.iter().enumerate().position(|(i, s)| s.is_none() && reserved != Some(i))
}

impl State {
    fn read(pad: Gamepad<'_>) -> Self {
        let mut result = Self::default();
        for (index, (button, _)) in BUTTONS.into_iter().enumerate() {
            result.buttons |= u32::from(pad.is_pressed(button)) << index;
        }
        result.sticks = STICKS.map(|(_, x, y)| Vec2::new(pad.value(x), pad.value(y)));
        result
    }

    fn update(&mut self, next: Self, slot: usize, inputs: &mut Vec<Input>) {
        for (index, (_, key)) in BUTTONS.into_iter().enumerate() {
            let bit = 1 << index;
            if (self.buttons ^ next.buttons) & bit != 0 {
                let pressed = next.buttons & bit != 0;
                inputs.push(Input {
                    slot,
                    key,
                    state: if pressed { "Begin" } else { "End" },
                    position: Vec3::ZERO,
                    delta: Vec2::ZERO,
                });
            }
        }
        for (index, (key, _, _)) in STICKS.into_iter().enumerate() {
            let position = next.sticks[index];
            let delta = position - self.sticks[index];
            if delta != Vec2::ZERO {
                inputs.push(Input {
                    slot,
                    key,
                    state: "Change",
                    position: position.extend(0.0),
                    delta,
                });
            }
        }
        *self = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_virtual_slot_is_gamepad1_and_counts_as_connected() {
        let mut pads = Gamepads::default();
        assert_eq!(pads.reserve(true), Some(0));
        let f = pads.poll(true);
        assert_eq!((f.connected, f.added, f.removed), (0b1, 0, 0));
        assert_eq!(pads.reserve(true), Some(0)); // held, not moved
        assert_eq!(pads.reserve(false), None);
        assert_eq!(pads.poll(true).connected, 0);
    }

    #[test]
    fn a_host_pad_in_gamepad1_moves_up_for_the_virtual_pad() {
        let mut slots: [Option<u32>; 8] = [Some(7), Some(8), None, None, None, None, None, None];
        assert_eq!(vacate_first(&mut slots), Some(2));
        assert_eq!(slots[..3], [None, Some(8), Some(7)]);
        assert_eq!(vacate_first(&mut slots), None); // (already free)
    }

    #[test]
    fn a_host_pad_skips_the_virtual_slot() {
        let mut pads = Gamepads::default();
        pads.reserve(true);
        assert_eq!(free_slot(&pads.slots, pads.reserved), Some(1)); // Gamepad2
        pads.reserve(false);
        assert_eq!(free_slot(&pads.slots, pads.reserved), Some(0));
    }
}

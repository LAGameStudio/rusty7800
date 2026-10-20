//! Atari CX22 trakball controller.
//!
//! Phase 4 port of `a7800-5.2/src/devices/bus/a7800_ctrl/trackball.cpp`
//! (BSD-3-Clause). `JOY`: fire button on 0x20 (active-low); `0x5F`
//! active-high unused (reads 0), `0x80` active-low unused (reads 1), so
//! the idle base is `0xA0`. Each axis uses 2-bit trackball encoding:
//! bit 0 = current spin direction, bit 1 = toggles on motion; X occupies
//! pins 1–2, Y occupies pins 3–4, so `joy_r = base | xcode | ycode << 2`.
//!
//! Note: the C++ keeps the last-position/code accumulators in function
//! `static`s (shared across both ports); here they are per-device instance
//! state, which is behaviorally identical for distinct axis motion and
//! keeps the two ports independent.

use crate::tia_io::TiaPort;

/// CX22 trakball state.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Trakball {
    /// Fire button (bit 0x20, active-low).
    pub button: bool,
    /// Last seen X/Y counter values (MAME `TRACKBALLX/Y` port bytes).
    last_x: u8,
    last_y: u8,
    last_xcode: u8,
    last_ycode: u8,
}

impl Trakball {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed the current X/Y counter values (machine accumulates deltas).
    pub fn set_position(&mut self, x: u8, y: u8) {
        if x > self.last_x {
            self.last_xcode = (self.last_xcode ^ 2) | 0x01;
        }
        if x < self.last_x {
            self.last_xcode = (self.last_xcode ^ 2) & 0xFE;
        }
        if y > self.last_y {
            self.last_ycode = (self.last_ycode ^ 2) | 0x01;
        }
        if y < self.last_y {
            self.last_ycode = (self.last_ycode ^ 2) & 0xFE;
        }
        self.last_x = x;
        self.last_y = y;
    }
}

impl TiaPort for Trakball {
    fn joy_r(&self) -> u8 {
        let base = ((!self.button as u8) << 5) | 0x80;
        base | self.last_xcode | (self.last_ycode << 2)
    }

    fn pot_x(&self) -> u8 {
        0xFF
    }
    fn pot_y(&self) -> u8 {
        0xFF
    }
    fn light_x(&self) -> u8 {
        0xFF
    }
    fn light_y(&self) -> u8 {
        0xFF
    }
    fn has_pot_x(&self) -> bool {
        false
    }
    fn has_pot_y(&self) -> bool {
        false
    }
    fn is_paddle(&self) -> bool {
        false
    }
    fn is_lightgun(&self) -> bool {
        false
    }
    fn has_pro_buttons(&self) -> bool {
        false
    }
}

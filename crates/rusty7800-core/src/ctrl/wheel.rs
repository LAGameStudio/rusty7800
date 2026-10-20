//! Atari driving-wheel controller.
//!
//! Phase 4 port of `a7800-5.2/src/devices/bus/a7800_ctrl/wheel.cpp`
//! (BSD-3-Clause). `JOY`: fire button on 0x20 (active-low); `0x50`
//! active-high unused (reads 0), `0x8C` active-low unused (reads 1), so
//! the idle base is `0xAC`. `joy_r` ORs the base with the gray-code
//! lookup `{ 0x00, 0x02, 0x03, 0x01 }` indexed by wheel-counter bits
//! 3–4 (`(wheel & 0x18) >> 3`), encoding rotation direction in pins 1–2.

use crate::tia_io::TiaPort;

/// Gray-code direction lookup, indexed by `(counter & 0x18) >> 3`.
pub const DRIVING_LOOKUP: [u8; 4] = [0x00, 0x02, 0x03, 0x01];

/// Driving-wheel state.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct DrivingWheel {
    /// Fire button (bit 0x20, active-low).
    pub button: bool,
    /// Rotation counter (MAME `WHEEL` port byte; machine accumulates deltas).
    pub wheel: u8,
}

impl DrivingWheel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Accumulate a rotation delta (wraps at 8 bits like the MAME port byte).
    pub fn spin(&mut self, delta: i8) {
        self.wheel = self.wheel.wrapping_add(delta as u8);
    }
}

impl TiaPort for DrivingWheel {
    fn joy_r(&self) -> u8 {
        let base = ((!self.button as u8) << 5) | 0x8C;
        base | DRIVING_LOOKUP[((self.wheel & 0x18) >> 3) as usize]
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

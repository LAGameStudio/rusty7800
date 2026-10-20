//! VCS (2600 single-button) digital joystick.
//!
//! Phase 4 port of `a7800-5.2/src/devices/bus/a7800_ctrl/joystick.cpp`
//! (BSD-3-Clause). Directions active-low (bits 0–3); the fire button is
//! pin 6 (bit 0x20, active-low). Bits `0x50` are active-high unused (read
//! 0), `0x80` active-low unused (reads 1), bit 0x10 unassigned (reads 0).
//! Idle byte is therefore `0xCF`. No pro buttons, pots, or gun.

use crate::tia_io::TiaPort;

/// VCS joystick state (all inputs released at power-on).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct VcsJoystick {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// Fire button (pin 6, active-low: pressed clears bit 0x20).
    pub button: bool,
}

impl VcsJoystick {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TiaPort for VcsJoystick {
    fn joy_r(&self) -> u8 {
        (!self.up as u8)
            | ((!self.down as u8) << 1)
            | ((!self.left as u8) << 2)
            | ((!self.right as u8) << 3)
            | ((!self.button as u8) << 5)
            | 0x80
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

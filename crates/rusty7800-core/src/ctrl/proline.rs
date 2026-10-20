//! Proline (7800 two-button) joystick.
//!
//! Phase 4 port of `a7800-5.2/src/devices/bus/a7800_ctrl/joyproline.cpp`
//! (BSD-3-Clause). `joy_r` returns the `JOY` port byte verbatim; bit layout
//! (from `ctrl.h`): 0x01 up, 0x02 down, 0x04 left, 0x08 right (active-low),
//! 0x10 Proline A button (active-high), 0x40 Proline B button (active-high).
//! Unused bits `0xA0` read high (idle); bit 0x20 is unassigned and reads 0.
//! Idle byte is therefore `0xAF`. `has_pro_buttons` is true (the driver's
//! INPT4/5 one-button rule keys off it).

use crate::tia_io::TiaPort;

/// Proline joystick state (all inputs released at power-on).
#[derive(Clone, Debug, Default)]
pub struct ProlineJoystick {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// Proline A button (pin 5, active-high).
    pub btn_a: bool,
    /// Proline B button (pin 9, active-high).
    pub btn_b: bool,
}

impl ProlineJoystick {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TiaPort for ProlineJoystick {
    fn joy_r(&self) -> u8 {
        (!self.up as u8)
            | ((!self.down as u8) << 1)
            | ((!self.left as u8) << 2)
            | ((!self.right as u8) << 3)
            | ((self.btn_a as u8) << 4)
            | 0x20
            | ((self.btn_b as u8) << 6)
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
        true
    }
}

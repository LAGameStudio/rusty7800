//! Atari analog paddles (two players per port).
//!
//! Phase 4 port of `a7800-5.2/src/devices/bus/a7800_ctrl/paddles.cpp`
//! (BSD-3-Clause). `JOY`: player-1 button on 0x08, player-2 button on
//! 0x04 (both active-low); `0xF3` active-low unused, so idle is `0xFF`.
//! `POTX`/`POTY` are 0–255 analog positions (MAME default `0x80`, mid
//! travel); the driver's INPT discharge ramp (`tia_io`) compares
//! `(cycles - paddle_start) / 97` against them. `has_pot`/`is_paddle`
//! are all true.

use crate::tia_io::TiaPort;

/// Paddle pair state.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Paddles {
    /// Player-1 fire button (bit 0x08, active-low).
    pub btn1: bool,
    /// Player-2 fire button (bit 0x04, active-low).
    pub btn2: bool,
    /// Player-1 paddle position, 0–255.
    pub pot_x: u8,
    /// Player-2 paddle position, 0–255.
    pub pot_y: u8,
}

impl Default for Paddles {
    fn default() -> Self {
        Paddles {
            btn1: false,
            btn2: false,
            pot_x: 0x80,
            pot_y: 0x80,
        }
    }
}

impl Paddles {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TiaPort for Paddles {
    fn joy_r(&self) -> u8 {
        ((!self.btn1 as u8) << 3) | ((!self.btn2 as u8) << 2) | 0xF3
    }

    fn pot_x(&self) -> u8 {
        self.pot_x
    }
    fn pot_y(&self) -> u8 {
        self.pot_y
    }
    fn light_x(&self) -> u8 {
        0xFF
    }
    fn light_y(&self) -> u8 {
        0xFF
    }
    fn has_pot_x(&self) -> bool {
        true
    }
    fn has_pot_y(&self) -> bool {
        true
    }
    fn is_paddle(&self) -> bool {
        true
    }
    fn is_lightgun(&self) -> bool {
        false
    }
    fn has_pro_buttons(&self) -> bool {
        false
    }
}

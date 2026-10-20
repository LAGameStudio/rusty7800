//! Atari lightgun.
//!
//! Phase 4 port of `a7800-5.2/src/devices/bus/a7800_ctrl/lightgun.cpp`
//! (BSD-3-Clause). `JOY`: trigger on bit 0x01 (active-high); `0xAE`
//! active-low unused, `0x50` active-high unused, so idle is `0xAE`.
//! `LIGHTX`/`LIGHTY` are the 0–255 GUI gunsight coordinates (MAME default
//! `0x00`); the driver's beam-race sensor (`tia_io`) scales them onto the
//! raster. `is_lightgun` is true (also gates the 0.5-cycle stall path).

use crate::tia_io::TiaPort;

/// Lightgun state.
#[derive(Clone, Debug, Default)]
pub struct LightGun {
    /// Trigger (bit 0x01, active-high).
    pub trigger: bool,
    /// Gunsight X, 0–255.
    pub light_x: u8,
    /// Gunsight Y, 0–255.
    pub light_y: u8,
}

impl LightGun {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TiaPort for LightGun {
    fn joy_r(&self) -> u8 {
        (self.trigger as u8) | 0xAE
    }

    fn pot_x(&self) -> u8 {
        0xFF
    }
    fn pot_y(&self) -> u8 {
        0xFF
    }
    fn light_x(&self) -> u8 {
        self.light_x
    }
    fn light_y(&self) -> u8 {
        self.light_y
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
        true
    }
    fn has_pro_buttons(&self) -> bool {
        false
    }
}

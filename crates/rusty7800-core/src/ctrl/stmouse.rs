//! Atari ST mouse controller.
//!
//! Phase 4 port of `a7800-5.2/src/devices/bus/a7800_ctrl/stmouse.cpp`
//! (BSD-3-Clause). `JOY`: left button on 0x40, right button on 0x20
//! (both active-low); `0x9F` active-high unused (reads 0), so the idle
//! base is `0x60` — same buttons as the Amiga mouse, different quadrature
//! wiring: X on pins 1+2 (`{0x00,0x02,0x03,0x01}`), Y on pins 3+4
//! (`{0x00,0x08,0x0C,0x04}`).

use crate::tia_io::TiaPort;

/// Quadrature lookup for the X axis (pins 1+2), indexed by `counter & 3`.
pub const AXIS_LOOKUP_X: [u8; 4] = [0x00, 0x02, 0x03, 0x01];
/// Quadrature lookup for the Y axis (pins 3+4), indexed by `counter & 3`.
pub const AXIS_LOOKUP_Y: [u8; 4] = [0x00, 0x08, 0x0C, 0x04];

/// ST mouse state.
#[derive(Clone, Debug, Default)]
pub struct StMouse {
    /// Left button (bit 0x40, active-low).
    pub btn_left: bool,
    /// Right button (bit 0x20, active-low).
    pub btn_right: bool,
    /// X/Y motion counters (MAME `MOUSEX/Y` port bytes; only low 2 bits read).
    pub mouse_x: u8,
    pub mouse_y: u8,
}

impl StMouse {
    pub fn new() -> Self {
        Self::default()
    }

    /// Accumulate a motion delta (wraps at 8 bits like the MAME port byte).
    pub fn move_by(&mut self, dx: i8, dy: i8) {
        self.mouse_x = self.mouse_x.wrapping_add(dx as u8);
        self.mouse_y = self.mouse_y.wrapping_add(dy as u8);
    }
}

impl TiaPort for StMouse {
    fn joy_r(&self) -> u8 {
        let base = ((!self.btn_left as u8) << 6) | ((!self.btn_right as u8) << 5);
        base | AXIS_LOOKUP_X[(self.mouse_x & 3) as usize]
            | AXIS_LOOKUP_Y[(self.mouse_y & 3) as usize]
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

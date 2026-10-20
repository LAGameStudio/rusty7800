//! Amiga mouse controller.
//!
//! Phase 4 port of `a7800-5.2/src/devices/bus/a7800_ctrl/amigamouse.cpp`
//! (BSD-3-Clause). `JOY`: left button on 0x40, right button on 0x20
//! (both active-low); `0x9F` active-high unused (reads 0), so the idle
//! base is `0x60`. Each axis contributes 2-bit quadrature encoding from
//! the low 2 counter bits: X on pins 2+4 (`{0x00,0x02,0x0A,0x08}`),
//! Y on pins 1+3 (`{0x00,0x01,0x05,0x04}`).

use crate::tia_io::TiaPort;

/// Quadrature lookup for the X axis (pins 2+4), indexed by `counter & 3`.
pub const AXIS_LOOKUP_X: [u8; 4] = [0x00, 0x02, 0x0A, 0x08];
/// Quadrature lookup for the Y axis (pins 1+3), indexed by `counter & 3`.
pub const AXIS_LOOKUP_Y: [u8; 4] = [0x00, 0x01, 0x05, 0x04];

/// Amiga mouse state.
#[derive(Clone, Debug, Default)]
pub struct AmigaMouse {
    /// Left button (bit 0x40, active-low).
    pub btn_left: bool,
    /// Right button (bit 0x20, active-low).
    pub btn_right: bool,
    /// X/Y motion counters (MAME `MOUSEX/Y` port bytes; only low 2 bits read).
    pub mouse_x: u8,
    pub mouse_y: u8,
}

impl AmigaMouse {
    pub fn new() -> Self {
        Self::default()
    }

    /// Accumulate a motion delta (wraps at 8 bits like the MAME port byte).
    pub fn move_by(&mut self, dx: i8, dy: i8) {
        self.mouse_x = self.mouse_x.wrapping_add(dx as u8);
        self.mouse_y = self.mouse_y.wrapping_add(dy as u8);
    }
}

impl TiaPort for AmigaMouse {
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

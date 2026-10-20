//! Atari keypad controller.
//!
//! Phase 4 port of `a7800-5.2/src/devices/bus/a7800_ctrl/keypad.cpp`
//! (BSD-3-Clause). The 12 keys form a 4-row × 3-column matrix held in a
//! 12-bit field (`KEYPAD` port). `joy_w` latches the column/row select
//! (`m_column = data & 0x0F`); each of the 4 select bits, when **clear**,
//! selects one row. Reads scan rows 0–3 and return the first selected
//! row's slice: `joy_r` returns `0xFF`/`0x00` from row bit 2, `pot_x_r`
//! from row bit 0 (default `0x00` when no row selected), `pot_y_r` from
//! row bit 1 (default `0x00`). `has_pot` is true on both axes (so the
//! driver's INPT path returns these bytes directly, not the discharge
//! ramp).
//!
//! Row layout (3 bits per row, `keypad >> 3*i`): row 0 = keys 1,2,3;
//! row 1 = keys 4,5,6; row 2 = keys 7,8,9; row 3 = keys *,0,#.

use crate::tia_io::TiaPort;

/// Key indices into the 12-bit matrix.
pub const KEY_1: u8 = 0;
pub const KEY_2: u8 = 1;
pub const KEY_3: u8 = 2;
pub const KEY_4: u8 = 3;
pub const KEY_5: u8 = 4;
pub const KEY_6: u8 = 5;
pub const KEY_7: u8 = 6;
pub const KEY_8: u8 = 7;
pub const KEY_9: u8 = 8;
pub const KEY_STAR: u8 = 9;
pub const KEY_0: u8 = 10;
pub const KEY_HASH: u8 = 11;

/// Keypad state.
#[derive(Clone, Debug, Default)]
pub struct Keypad {
    /// Pressed keys as a 12-bit matrix (bit `i` = key `i` held).
    pub keys: u16,
    /// Row-select latch from `joy_w` (low nibble; clear bit selects a row).
    column: u8,
}

impl Keypad {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn press(&mut self, key: u8) {
        debug_assert!(key < 12);
        self.keys |= 1 << key;
    }

    pub fn release(&mut self, key: u8) {
        debug_assert!(key < 12);
        self.keys &= !(1 << key);
    }

    /// First selected row (lowest index with a clear select bit), if any.
    fn selected_row(&self) -> Option<u32> {
        (0..4).find(|i| (self.column >> i) & 0x01 == 0)
    }
}

impl TiaPort for Keypad {
    fn joy_r(&self) -> u8 {
        match self.selected_row() {
            Some(i) if (self.keys >> (3 * i)) & 0x04 != 0 => 0xFF,
            Some(_) => 0x00,
            None => 0xFF,
        }
    }

    fn pot_x(&self) -> u8 {
        match self.selected_row() {
            Some(i) if (self.keys >> (3 * i)) & 0x01 != 0 => 0x00,
            Some(_) => 0xFF,
            None => 0x00,
        }
    }

    fn pot_y(&self) -> u8 {
        match self.selected_row() {
            Some(i) if (self.keys >> (3 * i)) & 0x02 != 0 => 0x00,
            Some(_) => 0xFF,
            None => 0x00,
        }
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
        false
    }
    fn is_lightgun(&self) -> bool {
        false
    }
    fn has_pro_buttons(&self) -> bool {
        false
    }

    fn joy_w(&mut self, data: u8) {
        // A real keypad takes ~400us to respond to a row change; the C++
        // latches immediately (TODO there too), and so do we.
        self.column = data & 0x0F;
    }
}

//! Console buttons, difficulty switches, and RIOT port wiring.
//!
//! Phase 4 port of the driver side in `a7800-5.2/src/mame/drivers/a7800.cpp`
//! (BSD-3-Clause): `INPUT_PORTS_START(a7800)` console byte (lines ~530),
//! `riot_joystick_r/w` nibble mux (lines ~202), `riot_button_pullup_w`
//! with its RIOT-DDR gate (lines ~232), and the `DRIVER_INIT` one-button
//! defaults (lines ~1424).
//!
//! Console byte (all active-low): bit 0 Reset (default key `T`), bit 1
//! Select (default key `E`), bit 3 Pause (default key `O`); bits 2,4,5
//! unused (read 1); bits 6–7 are the left/right difficulty DIPs (default
//! `A` = right position = bit set).

/// Default host keys for the console buttons (from `INPUT_PORTS_START`).
pub const KEY_RESET: char = 'T';
pub const KEY_SELECT: char = 'E';
pub const KEY_PAUSE: char = 'O';

/// Console buttons + difficulty switches (all released / `A` at power-on).
#[derive(Clone, Debug, Default)]
pub struct ConsoleState {
    /// Reset button held.
    pub reset: bool,
    /// Select button held.
    pub select: bool,
    /// Pause button held.
    pub pause: bool,
    /// Left difficulty in `B` (left) position; `false` = `A` (default).
    pub left_b: bool,
    /// Right difficulty in `B` (left) position; `false` = `A` (default).
    pub right_b: bool,
}

impl ConsoleState {
    pub fn new() -> Self {
        Self::default()
    }

    /// The `console_buttons` port byte consumed by `riot_console_button_r`.
    pub fn byte(&self) -> u8 {
        (!self.reset as u8)
            | ((!self.select as u8) << 1)
            | (1 << 2)
            | ((!self.pause as u8) << 3)
            | (1 << 4)
            | (1 << 5)
            | ((!self.left_b as u8) << 6)
            | ((!self.right_b as u8) << 7)
    }
}

/// RIOT PA read mux (`riot_joystick_r`): port-1 directions in the high
/// nibble, port-2 directions in the low nibble (pins 1–4 only).
pub fn riot_joystick_byte(p1_joy: u8, p2_joy: u8) -> u8 {
    ((p1_joy & 0x0F) << 4) | (p2_joy & 0x0F)
}

/// RIOT PA write split (`riot_joystick_w`): high nibble to port 1's
/// `joy_w` (keypad row select), low nibble to port 2's.
pub fn riot_joystick_split(data: u8) -> (u8, u8) {
    (data >> 4, data & 0x0F)
}

/// RIOT PB write (`riot_button_pullup_w`): pin 6 of each controller port
/// is held high by the RIOT chip when reading two-button controllers.
/// Each port's one-button latch only updates when the corresponding DDR
/// bit at RIOT `$283` is an output (bit 0x04 for port 1, 0x10 for port 2
/// — the C++ re-reads `$283` from CPU space per write; the machine passes
/// that byte in as `ddr_283`).
pub fn pullup_write(ddr_283: u8, data: u8, p1_one_button: &mut bool, p2_one_button: &mut bool) {
    if ddr_283 & 0x04 != 0 {
        *p1_one_button = data & 0x04 != 0;
    }
    if ddr_283 & 0x10 != 0 {
        *p2_one_button = data & 0x10 != 0;
    }
}

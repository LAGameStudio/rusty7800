//! Rusty7800 `tia_io` — driver-side TIA read/write logic.
//!
//! Phase 3 port of `a7800_state::tia_r` / `tia_w` (`a7800.cpp`, lines
//! ~240–460): INPT0–5 paddle/pot/digital mux, lightgun beam-race with the
//! 0.5-cycle stall gate, collision-register stub, INPTCTRL/ctrl_lock,
//! VBLANK paddle-discharge timing, and audio-register write-through.
//!
//! Controller queries go through [`TiaPort`], which Phase 4's control-port
//! devices implement (method names mirror the C++ port interface). Beam
//! position and CPU cycle count are machine inputs per access.

use crate::specs::PADDLE_CYCLE_DIVISOR;
use crate::tia::TiaAudio;

/// Queries the driver makes of a control port (mirrors the C++ port API).
/// Phase 4's `ctrl` devices implement this; `ControlPort` adds the
/// empty-slot (`exists() == false` → `0xFF`) semantics from `ctrl.h`.
pub trait TiaPort {
    fn joy_r(&self) -> u8;
    /// Row/column select write (only the keypad implements it; the port
    /// slot ignores writes when empty, per `ctrl.h`).
    fn joy_w(&mut self, _data: u8) {}
    fn pot_x(&self) -> u8;
    fn pot_y(&self) -> u8;
    fn light_x(&self) -> u8;
    fn light_y(&self) -> u8;
    fn has_pot_x(&self) -> bool;
    fn has_pot_y(&self) -> bool;
    fn is_paddle(&self) -> bool;
    fn is_lightgun(&self) -> bool;
    fn has_pro_buttons(&self) -> bool;
}

/// Raster/beam state for lightgun hit testing.
#[derive(Clone, Copy, Debug)]
pub struct BeamState {
    /// MARIA pixel clock position (driver uses `hpos() / 2`).
    pub hpos: u16,
    /// Current raster line.
    pub vpos: u16,
    /// Machine line count (263 NTSC / 313 PAL).
    pub lines: u16,
    pub is_pal: bool,
}

/// Driver-side TIA I/O state (fields live in `a7800_state` in C++).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TiaIo {
    /// INPTCTRL lock (set by bit 0 of a TIA write while unlocked).
    pub ctrl_lock: bool,
    /// Latched INPTCTRL value; bit 1 selects MARIA vs TIA/RIOT mapping and
    /// bits 0–1 == 3 routes audio writes to the TIA chip.
    pub ctrl_reg: u8,
    /// Set when INPTCTRL bit 0 is written (MAME `m_maria_flag`).
    pub maria_flag: bool,
    /// CPU cycle count at paddle discharge (`paddle_start`).
    pub paddle_start: u64,
    paddle_grounded: bool,
    /// 0.5-cycle stall alternator (`tia_delay`).
    tia_delay: u8,
    /// Two-button pull-up state per port (`m_p1/p2_one_button`).
    pub p1_one_button: bool,
    pub p2_one_button: bool,
}

impl Default for TiaIo {
    fn default() -> Self {
        Self::new()
    }
}

impl TiaIo {
    pub fn new() -> Self {
        TiaIo {
            ctrl_lock: false,
            ctrl_reg: 0,
            maria_flag: false,
            paddle_start: 0,
            paddle_grounded: false,
            tia_delay: 0,
            // DRIVER_INITs set both ports to one-button mode.
            p1_one_button: true,
            p2_one_button: true,
        }
    }

    pub fn reset(&mut self) {
        self.ctrl_lock = false;
        self.ctrl_reg = 0;
        self.maria_flag = false;
        self.paddle_start = 0;
        self.paddle_grounded = false;
        self.tia_delay = 0;
    }

    /// Lightgun 0.5-cycle stall gate, shared by reads and writes: every 2nd
    /// TIA access while a lightgun is plugged costs one extra CPU cycle.
    /// Returns the stall (0 or 1).
    fn stall_gate(&mut self, p1_gun: bool, p2_gun: bool) -> u64 {
        if p1_gun || p2_gun {
            self.tia_delay += 1;
            if self.tia_delay == 2 {
                self.tia_delay = 0;
                return 1;
            }
        }
        0
    }

    /// Paddle discharge level for one axis: 0x80 once
    /// `(cpu_cycles - paddle_start) / 97` exceeds `level`.
    fn paddle_level(&self, cpu_cycles: u64, level: u8) -> u8 {
        let elapsed =
            cpu_cycles.saturating_sub(self.paddle_start) / PADDLE_CYCLE_DIVISOR;
        if elapsed > level as u64 {
            0x80
        } else {
            0x00
        }
    }

    /// Lightgun beam-race sensor for one port (INPT4/INPT5 gun path).
    fn gun_sensor<P: TiaPort>(&self, port: &P, beam: BeamState) -> u8 {
        let cpu_x = beam.hpos as i32 / 2;
        let cpu_y = (beam.vpos % beam.lines) as i32;
        let gun_x = (((port.light_x() as u32 * 160) / 255 + 95) % 227) as i32;
        let gun_y = if beam.is_pal {
            ((port.light_y() as u32 * 260) / 255 + 24) as i32
        } else {
            ((port.light_y() as u32 * 228) / 255 + 16) as i32
        };
        let dx = gun_x - cpu_x;
        let dy = gun_y - cpu_y;
        // Within 8 pixels (squared compare, no sqrt) -> sensor lit (0x00).
        if dx * dx + dy * dy < 64 {
            0x00
        } else {
            0x80
        }
    }

    /// TIA read. Returns `(value, stall_cycles)`.
    pub fn read<P1: TiaPort, P2: TiaPort>(
        &mut self,
        offset: u16,
        cpu_cycles: u64,
        beam: BeamState,
        p1: &P1,
        p2: &P2,
    ) -> (u8, u64) {
        let stall = self.stall_gate(p1.is_lightgun(), p2.is_lightgun());
        let value = match offset & 0x0F {
            // Collision registers: 7800 ignores TIA graphics; return 0.
            0x00..=0x07 => 0x00,
            // INPT0: port 1 pot X.
            0x08 => {
                if p1.is_paddle() {
                    self.paddle_level(cpu_cycles, p1.pot_x())
                } else if p1.has_pot_x() {
                    p1.pot_x()
                } else {
                    (p1.joy_r() & 0x10) << 3
                }
            }
            // INPT1: port 1 pot Y.
            0x09 => {
                if p1.is_paddle() {
                    self.paddle_level(cpu_cycles, p1.pot_y())
                } else if p1.has_pot_y() {
                    p1.pot_y()
                } else {
                    if p1.joy_r() & 0x40 != 0 {
                        0x80
                    } else {
                        0x00
                    }
                }
            }
            // INPT2: port 2 pot X (XOR quirk on the paddle scale).
            0x0A => {
                if p2.is_paddle() {
                    self.paddle_level(cpu_cycles, p2.pot_x() ^ 0xFF)
                } else if p2.has_pot_x() {
                    p2.pot_x()
                } else {
                    if p2.joy_r() & 0x10 != 0 {
                        0x80
                    } else {
                        0x00
                    }
                }
            }
            // INPT3: port 2 pot Y.
            0x0B => {
                if p2.is_paddle() {
                    self.paddle_level(cpu_cycles, p2.pot_y())
                } else if p2.has_pot_y() {
                    p2.pot_y()
                } else {
                    if p2.joy_r() & 0x40 != 0 {
                        0x80
                    } else {
                        0x00
                    }
                }
            }
            // INPT4: port 1 button / lightgun sensor.
            0x0C => {
                if p1.has_pro_buttons() && self.p1_one_button && (p1.joy_r() & 0x50) != 0 {
                    // A and B activate the VCS button line in 1-button mode.
                    0x00
                } else if p1.is_lightgun() {
                    self.gun_sensor(p1, beam)
                } else if p1.joy_r() & 0x20 != 0 {
                    0x80
                } else {
                    0x00
                }
            }
            // INPT5: port 2 button / lightgun sensor.
            0x0D => {
                if p2.has_pro_buttons() && self.p2_one_button && (p2.joy_r() & 0x50) != 0 {
                    0x00
                } else if p2.is_lightgun() {
                    self.gun_sensor(p2, beam)
                } else if p2.joy_r() & 0x20 != 0 {
                    0x80
                } else {
                    0x00
                }
            }
            _ => 0xFF,
        };
        (value, stall)
    }

    /// TIA write. Returns stall cycles (lightgun gate). `cpu_cycles` feeds
    /// paddle-discharge timing; `audio` receives register write-through.
    pub fn write(
        &mut self,
        offset: u16,
        data: u8,
        cpu_cycles: u64,
        p1_gun: bool,
        p2_gun: bool,
        audio: &mut TiaAudio,
    ) -> u64 {
        let stall = self.stall_gate(p1_gun, p2_gun);
        // INPTCTRL covers TIA registers 0x00–0x1F until locked. (Offset
        // carries mirror bits, so mirrored writes skip this block.)
        if offset < 0x20 {
            // C++ shape: `if (lock && off == 1) flag; else if (!lock) flag;`
            // i.e. flag unless locked-and-elsewhere. Simplified equivalently.
            if data & 0x01 != 0 && (!self.ctrl_lock || offset == 0x01) {
                self.maria_flag = true;
            }
            if !self.ctrl_lock {
                self.ctrl_lock = data & 0x01 != 0;
                self.ctrl_reg = data;
            }
        }
        if offset == 0x01 {
            // VBLANK: bit 7 grounds the paddles; release stamps paddle_start.
            if data & 0x80 != 0 {
                self.paddle_grounded = true;
            } else if self.paddle_grounded {
                self.paddle_grounded = false;
                self.paddle_start = cpu_cycles;
            }
        } else if (self.ctrl_reg & 3) == 3 {
            audio.write((offset & 0xFF) as u8, data);
        }
        stall
    }
}

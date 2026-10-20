//! Rusty7800 `riot` — MOS 6532 RIOT.
//!
//! Phase 1 port of `a7800-5.2/src/devices/machine/6532riot.*` (BSD-3-Clause,
//! Aaron Giles), re-expressed cycle-stepped instead of event-timer driven.
//! The documented timer rules from the file header are preserved verbatim:
//! flag 0→1 keeps counting at 1-cycle rate; timer read/write clears the
//! flag; flag set + contents 0 stops the down-count (free-run reload keeps
//! `get_timer` wrapping 0→255 in FINISHING, as in MAME's `timer_end`).
//! This file also encodes Mike Saarna's 2014 RIOT timer fix (per the
//! a7800.cpp changelog); `tests/phase1_riot.rs` pins the behavior.
//!
//! Machine wiring: the driver connects RIOT PA to the control ports and PB
//! to console buttons/difficulty (a7800.cpp `riot_joystick_*`,
//! `riot_console_button_*`). The machine sets `pa_in`/`pb_in` and reads
//! `pa_out`/`pb_out` + `irq_asserted`.

pub const TIMER_FLAG: u8 = 0x80;
pub const PA7_FLAG: u8 = 0x40;

/// Timer divisor shifts selected by offset bits 0–1 (reg_w timershift table).
const TIMER_SHIFTS: [u32; 4] = [0, 3, 6, 10];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimerState {
    Idle,
    Counting,
    Finishing,
}

#[derive(Clone, Debug)]
pub struct Riot {
    pa_in: u8,
    pub pa_out: u8,
    pa_ddr: u8,
    pb_in: u8,
    pub pb_out: u8,
    pb_ddr: u8,
    irq_state: u8,
    irq_enable: u8,
    /// Asserted IRQ line level (machine polls this).
    pub irq_asserted: bool,
    /// PA7 edge-detect direction: 0x80 = high-to-low... see update rule.
    pa7dir: u8,
    pa7prev: u8,
    timer_shift: u32,
    timer_state: TimerState,
    /// Ticks (RIOT clocks = CPU cycles) remaining.
    timer_remaining: u32,
}

impl Default for Riot {
    fn default() -> Self {
        Self::new()
    }
}

impl Riot {
    pub fn new() -> Self {
        let mut r = Riot {
            pa_in: 0,
            pa_out: 0,
            pa_ddr: 0,
            pb_in: 0,
            pb_out: 0,
            pb_ddr: 0,
            irq_state: 0,
            irq_enable: 0,
            irq_asserted: false,
            pa7dir: 0,
            pa7prev: 0,
            timer_shift: 10,
            timer_state: TimerState::Idle,
            timer_remaining: 0,
        };
        r.reset();
        r
    }

    /// device_reset: I/O zeroed, IRQ state cleared, timer COUNTING with
    /// shift 10 and 256<<10 ticks.
    pub fn reset(&mut self) {
        self.pa_in = 0;
        self.pa_out = 0;
        self.pa_ddr = 0;
        self.pb_in = 0;
        self.pb_out = 0;
        self.pb_ddr = 0;
        self.irq_enable = 0;
        self.irq_state = 0;
        self.update_irq();
        self.pa7dir = 0;
        self.pa7prev = 0;
        self.timer_shift = 10;
        self.timer_state = TimerState::Counting;
        self.timer_remaining = 256 << 10;
    }

    /// Advance the timer by `cycles` RIOT clocks.
    pub fn tick(&mut self, mut cycles: u64) {
        while cycles > 0 {
            match self.timer_state {
                TimerState::Idle => return,
                TimerState::Counting | TimerState::Finishing => {
                    let step = cycles.min(self.timer_remaining.max(1) as u64);
                    if self.timer_remaining == 0 {
                        // Flag set + contents 0: reload free-run slice.
                        self.timer_remaining = 256;
                        continue;
                    }
                    self.timer_remaining -= step as u32;
                    cycles -= step;
                    if self.timer_remaining == 0 {
                        self.on_timer_end();
                    }
                }
            }
        }
    }

    /// MAME timer_end().
    fn on_timer_end(&mut self) {
        match self.timer_state {
            TimerState::Counting => {
                self.timer_state = TimerState::Finishing;
                self.timer_remaining = 256;
                self.irq_state |= TIMER_FLAG;
                self.update_irq();
            }
            TimerState::Finishing => {
                self.timer_remaining = 256;
            }
            TimerState::Idle => {}
        }
    }

    fn update_irq(&mut self) {
        self.irq_asserted = (self.irq_state & self.irq_enable) != 0;
    }

    fn apply_ddr(out: u8, ddr: u8, inp: u8) -> u8 {
        (out & ddr) | (inp & !ddr)
    }

    fn update_pa7(&mut self) {
        let data = Self::apply_ddr(self.pa_out, self.pa_ddr, self.pa_in) & 0x80;
        if (self.pa7prev ^ data) != 0 && (self.pa7dir ^ data) == 0 {
            self.irq_state |= PA7_FLAG;
            self.update_irq();
        }
        self.pa7prev = data;
    }

    /// Current timer value (MAME get_timer()).
    pub fn timer_value(&self) -> u8 {
        match self.timer_state {
            TimerState::Idle => 0,
            TimerState::Counting => (self.timer_remaining >> self.timer_shift) as u8,
            TimerState::Finishing => self.timer_remaining as u8,
        }
    }

    /// Machine drives port A input lines (joystick state).
    pub fn set_pa_in(&mut self, data: u8) {
        self.pa_in = data;
        self.update_pa7();
    }

    /// Machine drives port B input lines (console buttons/difficulty).
    pub fn set_pb_in(&mut self, data: u8) {
        self.pb_in = data;
    }

    /// Register read; `offset` = address & 0x1F.
    pub fn reg_read(&mut self, offset: u8) -> u8 {
        let off = offset & 0x1F;
        if (off & 0x05) == 0x05 {
            // Interrupt flags; implicitly clears PA7 flag.
            let v = self.irq_state;
            self.irq_state &= !PA7_FLAG;
            self.update_irq();
            v
        } else if (off & 0x05) == 0x04 {
            // Timer.
            let v = self.timer_value();
            if (off & 8) != 0 {
                self.irq_enable |= TIMER_FLAG;
            } else {
                self.irq_enable &= !TIMER_FLAG;
            }
            if self.timer_state != TimerState::Finishing || v != 0xff {
                self.irq_state &= !TIMER_FLAG;
            }
            self.update_irq();
            v
        } else if (off & 1) != 0 {
            // DDR read; A1 selects port.
            if (off & 2) != 0 {
                self.pb_ddr
            } else {
                self.pa_ddr
            }
        } else if (off & 2) != 0 {
            Self::apply_ddr(self.pb_out, self.pb_ddr, self.pb_in)
        } else {
            Self::apply_ddr(self.pa_out, self.pa_ddr, self.pa_in)
        }
    }

    /// Register write; `offset` = address & 0x1F.
    pub fn reg_write(&mut self, offset: u8, data: u8) {
        let off = offset & 0x1F;
        if (off & 0x14) == 0x14 {
            // Timer write.
            self.timer_shift = TIMER_SHIFTS[(off & 3) as usize];
            if (off & 8) != 0 {
                self.irq_enable |= TIMER_FLAG;
            } else {
                self.irq_enable &= !TIMER_FLAG;
            }
            if self.timer_state != TimerState::Finishing || self.timer_value() != 0xff {
                self.irq_state &= !TIMER_FLAG;
            }
            self.update_irq();
            self.timer_state = TimerState::Counting;
            // target = now + 1 + (data << shift).
            self.timer_remaining = 1 + ((data as u32) << self.timer_shift);
        } else if (off & 0x14) == 0x04 {
            // Edge-detect control.
            if (off & 2) != 0 {
                self.irq_enable |= PA7_FLAG;
            } else {
                self.irq_enable &= !PA7_FLAG;
            }
            self.pa7dir = (off & 1) << 7;
            self.update_irq();
        } else if (off & 1) != 0 {
            // DDR write.
            if (off & 2) != 0 {
                self.pb_ddr = data;
            } else {
                self.pa_ddr = data;
                self.update_pa7();
            }
        } else {
            // Output write.
            if (off & 2) != 0 {
                self.pb_out = data;
            } else {
                self.pa_out = data;
                self.update_pa7();
            }
        }
    }

    /// Full-register-space read/write at CPU address (mirrors handled by bus).
    pub fn read(&mut self, addr: u16) -> u8 {
        self.reg_read((addr & 0x1F) as u8)
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        self.reg_write((addr & 0x1F) as u8, data);
    }
}

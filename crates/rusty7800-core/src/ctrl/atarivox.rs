//! AtariVox speech + SaveKey EEPROM controller devices.
//!
//! Port of the AtariVox programming model from the 8BitDev 7800 wiki
//! ("AtariVox for the 7800", adapted from Alex Herbert's 2600 guide) and
//! the Microchip 24LC256 datasheet behavior it cites. The DE-9 pins are
//! re-purposed as serial lines and bit-banged by the game through the
//! RIOT SWCHA data/direction registers:
//!
//! * pin 1 (nibble bit 0) — SpeakJet DATA, 19 200 baud 8N1 **inverted**
//!   async serial, console → device, idle high;
//! * pin 2 (nibble bit 1) — SpeakJet READY, device → console, low = busy;
//! * pin 3 (nibble bit 2) — EEPROM SDA, open-drain bidirectional;
//! * pin 4 (nibble bit 3) — EEPROM SCL, console → device.
//!
//! Stella emulates the same hardware (`AtariVox`/`SaveKey` controller
//! types: full EEPROM, speech bytes decoded and forwarded); MAME's
//! `a7800_ctrl` slot bus has no such device, so the slot names here are
//! new (`specs::CONTROL_PORT_DEVICES` notes the extension).
//!
//! What is and isn't modeled: the 24LC256 EEPROM is modeled
//! bit-accurately (START/STOP, control, address, page-wrapped writes,
//! ACK/NACK, multi-byte reads), 32 KiB, with `nvram_image` /
//! `load_nvram` persistence like the cart battery. Power-on page 0
//! carries the System Settings signature (`"ATARIVOX"` + `0x80` = NTSC
//! 60 Hz) real units ship with, so detection code that reads the syspage
//! works out of the box.
//!
//! The SpeakJet protocol is modeled (baud auto-calibrated per frame off
//! the start bit, READY busy window, 64-byte input buffer semantics via
//! the busy line), but the received phrase bytes are **logged, not
//! synthesized**: there is no open-source SpeakJet vocal-tract model to
//! port (Stella forwards to real hardware; Gopher2600 shells to
//! Festival). `take_speech` / socket `state.port_speech` expose the log
//! for harnesses. The `SaveKey` variant is the same EEPROM with the
//! speech side unfitted (DATA ignored, READY pulled high).
//!
//! Console-side drive convention: the machine forwards the RIOT PA latch
//! nibble (`levels`) with the DDR nibble (`mask`) on every SWCHA
//! data/DDR write. Bits clear in `mask` float to pull-up high, and
//! direction flips surface as edges, so I2C SDA release-while-SCL-high
//! correctly reads as STOP.

use std::collections::VecDeque;

/// 24LC256 size (32K × 8) and 64-byte page-write granularity.
pub const EEPROM_SIZE: usize = 32 * 1024;
pub const EEPROM_PAGE: usize = 64;
/// Speech log capacity (oldest bytes drop past this).
pub const SPEECH_LOG_CAP: usize = 8192;
/// SpeakJet READY busy window per received byte (~1.1 ms @ 1.79 MHz).
/// Real drain rate depends on the spoken phrase; this only needs to be
/// long enough that poll loops see busy, short enough not to stall games.
pub const SPEECH_BUSY_CYCLES: u64 = 2000;
/// Nominal 19 200 baud cell at the 7800 CPU clock, for reference only —
/// the decoder calibrates off each frame's start bit instead.
pub const UART_NOMINAL_CELL: u64 = 93;

/// I2C control bytes (7-bit device address `0x50`, R/W in bit 0).
const CTRL_WRITE: u8 = 0xA0;
const CTRL_READ: u8 = 0xA1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum EPhase {
    /// Between operations: next byte must be a control byte.
    Idle,
    AddrHi,
    AddrLo,
    Write,
    Read,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Eeprom24LC256 {
    mem: Vec<u8>,
    /// Console SCL level (driven latch, or pull-up high when released).
    scl: bool,
    /// Console SDA level when driven; pull-up high when released.
    sda: bool,
    sda_drv: bool,
    /// Device open-drain pull on SDA (`true` = pulling low).
    sda_lo: bool,
    phase: EPhase,
    /// Receive shift register + rising-edge count in the current byte.
    rx: u8,
    clk: u8,
    /// Transmit byte + bits still to present (Read phase).
    tx: u8,
    tx_left: u8,
    /// 15-bit memory pointer.
    addr: u16,
}

impl Eeprom24LC256 {
    fn new() -> Self {
        let mut mem = vec![0xFFu8; EEPROM_SIZE];
        // Factory System Settings page: 8-byte signature + TV mode byte
        // (bit 7 = 1 NTSC, bit 6 = 0 60 Hz), per the wiki syspage.inc.
        mem[0..8].copy_from_slice(b"ATARIVOX");
        mem[8] = 0x80;
        Eeprom24LC256 {
            mem,
            scl: true,
            sda: true,
            sda_drv: false,
            sda_lo: false,
            phase: EPhase::Idle,
            rx: 0,
            clk: 0,
            tx: 0,
            tx_left: 0,
            addr: 0,
        }
    }

    /// Current bus SDA level (console drive AND device open-drain).
    fn bus_sda(&self) -> bool {
        (if self.sda_drv { self.sda } else { true }) && !self.sda_lo
    }

    /// Console SCL drive changed (released = pull-up high).
    fn drive_scl(&mut self, level: bool) {
        if level == self.scl {
            return;
        }
        self.scl = level;
        if level {
            self.scl_rise();
        } else {
            self.scl_fall();
        }
    }

    /// Console SDA drive changed. A change while SCL is high is
    /// START (falling) or STOP (rising); otherwise it just moves the bus.
    fn drive_sda(&mut self, driven: bool, level: bool) {
        let now = if driven { level } else { true };
        let was = if self.sda_drv { self.sda } else { true };
        self.sda_drv = driven;
        self.sda = level;
        if self.scl && now != was {
            if now {
                self.on_stop();
            } else {
                self.on_start();
            }
        }
    }

    fn on_start(&mut self) {
        // (Repeated) START: abort the current operation, expect control.
        self.phase = EPhase::Idle;
        self.rx = 0;
        self.clk = 0;
        self.sda_lo = false;
        self.tx_left = 0;
    }

    fn on_stop(&mut self) {
        // Writes applied per byte with page wrap, so STOP only closes.
        self.phase = EPhase::Idle;
        self.rx = 0;
        self.clk = 0;
        self.sda_lo = false;
        self.tx_left = 0;
    }

    fn scl_rise(&mut self) {
        let sda = self.bus_sda();
        if self.phase == EPhase::Read {
            // Master samples our bit on clocks 1–8; clock 9 is its ACK.
            if self.clk == 8 {
                self.clk = 9;
                if sda {
                    // NACK: end of read, release the bus.
                    self.phase = EPhase::Idle;
                    self.sda_lo = false;
                } else {
                    // ACK: advance and arm the next byte for its falling edge.
                    self.addr = (self.addr + 1) & 0x7FFF;
                    self.tx = self.mem[self.addr as usize];
                    self.tx_left = 8;
                }
            } else {
                self.clk += 1;
            }
            return;
        }
        // Reception: sample on clocks 1–8; clock 9 is our ACK slot.
        if self.clk < 8 {
            self.rx = (self.rx << 1) | (sda as u8);
            self.clk += 1;
        } else {
            self.clk = 9;
        }
    }

    fn scl_fall(&mut self) {
        if self.phase == EPhase::Read {
            if self.clk == 8 {
                // ACK clock approaches: release so the master can drive.
                self.sda_lo = false;
            } else if self.tx_left > 0 {
                // Present the next MSB while SCL is low.
                self.sda_lo = (self.tx & 0x80) == 0;
                self.tx <<= 1;
                self.tx_left -= 1;
                if self.clk == 9 {
                    // Post-ACK falling edge: fresh byte, restart count.
                    self.clk = 0;
                }
            }
            return;
        }
        if self.clk == 8 {
            // Byte complete: drive ACK (low) or NACK (release) on clock 9.
            self.sda_lo = self.ack_for(self.rx);
        } else if self.clk == 9 {
            self.sda_lo = false;
            self.clk = 0;
            self.commit_byte();
        }
    }

    /// Whether byte `b` (just received in `phase`) is acknowledged.
    fn ack_for(&self, b: u8) -> bool {
        match self.phase {
            EPhase::Idle => b == CTRL_WRITE || b == CTRL_READ,
            EPhase::AddrHi | EPhase::AddrLo | EPhase::Write => true,
            EPhase::Read => true, // unreachable (read uses scl_fall early path)
        }
    }

    /// Consume the received byte after its ACK clock.
    fn commit_byte(&mut self) {
        let b = self.rx;
        self.rx = 0;
        match self.phase {
            EPhase::Idle => {
                if b == CTRL_WRITE {
                    self.phase = EPhase::AddrHi;
                } else if b == CTRL_READ {
                    self.phase = EPhase::Read;
                    self.tx = self.mem[self.addr as usize];
                    self.tx_left = 8;
                    // Present the MSB immediately (SCL is low now).
                    self.sda_lo = (self.tx & 0x80) == 0;
                    self.tx <<= 1;
                    self.tx_left -= 1;
                }
                // Unrecognized control: NACK was already signaled by
                // release; stay idle.
            }
            EPhase::AddrHi => {
                self.addr = ((b as u16) << 8) & 0x7FFF;
                self.phase = EPhase::AddrLo;
            }
            EPhase::AddrLo => {
                self.addr = (self.addr & 0x7F00) | b as u16;
                self.addr &= 0x7FFF;
                self.phase = EPhase::Write;
            }
            EPhase::Write => {
                // Page-wrapped write, like the 24LC256 address counter.
                let page = self.addr & !(EEPROM_PAGE as u16 - 1);
                self.mem[self.addr as usize] = b;
                self.addr = page | ((self.addr + 1) & (EEPROM_PAGE as u16 - 1));
            }
            EPhase::Read => {}
        }
    }
}

/// One async frame in progress: `t0` = start-bit falling edge, `cell` =
/// calibrated width (0 until the first rising edge), `runs` = line levels
/// after the start cell with cell counts, `pushed` = their total. The
/// current (unclosed) run derives from the line level + `edge_t`, so
/// tails running into idle (0xFF) or a stop cell with no trailing edge
/// still complete via [`SpeakJet::poll`], which the port calls on every
/// RIOT read with fresh time.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Frame {
    t0: u64,
    cell: u64,
    runs: Vec<(bool, u64)>,
    pushed: u64,
}

impl Frame {
    fn push(&mut self, level: bool, n: u64) {
        if let Some(last) = self.runs.last_mut() {
            if last.0 == level {
                last.1 += n;
                self.pushed += n;
                return;
            }
        }
        self.runs.push((level, n));
        self.pushed += n;
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct SpeakJet {
    speech: bool,
    busy_until: u64,
    log: VecDeque<u8>,
    frame: Option<Frame>,
    /// Running baud calibration (seeds at nominal, tracks clean frames).
    cell_hint: u64,
    /// Current DATA level + last edge time (run-math anchor; polls must
    /// not move it).
    level: bool,
    edge_t: u64,
}

impl SpeakJet {
    fn new(speech: bool) -> Self {
        SpeakJet {
            speech,
            busy_until: 0,
            log: VecDeque::new(),
            frame: None,
            cell_hint: UART_NOMINAL_CELL,
            level: true,
            edge_t: 0,
        }
    }

    fn ready(&self, now: u64) -> bool {
        !self.speech || now >= self.busy_until
    }

    /// Console DATA level changed to `level` at cycle `now`.
    fn data_edge(&mut self, level: bool, now: u64) {
        if !self.speech || level == self.level {
            return;
        }
        let prev = self.level;
        self.level = level;
        // Inter-frame idle (>12 cells) aborts any stale frame; the edge
        // is then treated as fresh.
        let stale = match self.frame.as_ref() {
            Some(f) => f.cell > 0 && now.saturating_sub(self.edge_t) > 12 * f.cell,
            None => false,
        };
        if stale {
            self.frame = None;
        }
        let dt = now.saturating_sub(self.edge_t);
        self.edge_t = now;
        if self.frame.is_none() {
            // Idle: only a falling edge (start bit) begins a frame.
            if !level {
                self.frame = Some(Frame {
                    t0: now,
                    cell: 0,
                    runs: Vec::new(),
                    pushed: 0,
                });
            }
            return;
        }
        if self.frame.as_ref().unwrap().cell == 0 {
            // First rising edge ends the start bit: calibrate. Its width
            // covers the start cell plus any leading zero data bits —
            // `m` counts them off the running hint (nominal 93), so
            // bytes like 0x00 decode instead of miscalibrating.
            if !level {
                self.frame = None; // start never ended; abort
                return;
            }
            let m = ((dt + self.cell_hint / 2) / self.cell_hint.max(1)).clamp(1, 9);
            let cell = dt / m.max(1);
            if !(16..=20_000).contains(&cell) {
                self.frame = None; // noise, not a start bit
                return;
            }
            let f = self.frame.as_mut().unwrap();
            f.cell = cell;
            self.cell_hint = cell;
            if m > 1 {
                f.push(false, m - 1);
            }
            return;
        }
        // Run of the just-ended level, rounded to cells.
        let cell = self.frame.as_ref().unwrap().cell;
        let n = (dt + cell / 2) / cell;
        if n == 0 {
            return; // glitch: absorb without recording
        }
        self.frame.as_mut().unwrap().push(prev, n);
        if self.frame.as_ref().unwrap().pushed > 12 {
            self.frame = None; // runaway: noise, not a frame
            return;
        }
        self.maybe_finalize(now, level, 0);
    }

    /// Complete a frame whose 10 cells (start + 8 data + stop) have
    /// elapsed. `cur_level`/`cur_extra` describe the still-open tail run
    /// (empty at an edge, measured from the clock at a poll).
    fn maybe_finalize(&mut self, now: u64, cur_level: bool, cur_extra: u64) {
        let (t0, cell, runs) = match self.frame.as_ref() {
            Some(f) if f.cell > 0 => (f.t0, f.cell, f.runs.clone()),
            _ => return,
        };
        let pushed: u64 = runs.iter().map(|&(_, c)| c).sum();
        if 1 + pushed + cur_extra < 10 {
            return;
        }
        // First 10 cells: [start, d0..d7 LSB-first, stop].
        let mut cells = vec![false];
        for &(lv, c) in &runs {
            for _ in 0..c {
                if cells.len() >= 10 {
                    break;
                }
                cells.push(lv);
            }
        }
        while cells.len() < 10 {
            cells.push(cur_level);
        }
        let mut byte = 0u8;
        for (i, &c) in cells[1..9].iter().enumerate() {
            byte |= (c as u8) << i;
        }
        let end = t0 + 10 * cell;
        let _ = now;
        self.frame = None;
        if cells[9] {
            self.busy_until = end + SPEECH_BUSY_CYCLES;
            if self.log.len() >= SPEECH_LOG_CAP {
                self.log.pop_front();
            }
            self.log.push_back(byte);
        }
        // Bad stop bit: byte dropped, like hardware.
    }

    /// `pin_levels` calls this with fresh time so frames whose tail runs
    /// into idle still complete (single bytes, 0xFF, phrase ends).
    fn poll(&mut self, now: u64) {
        let (cell, edge_t, level) = match self.frame.as_ref() {
            Some(f) if f.cell > 0 => (f.cell, self.edge_t, self.level),
            _ => return,
        };
        let cur = now.saturating_sub(edge_t) / cell.max(1);
        self.maybe_finalize(now, level, cur);
    }
}

/// AtariVox (speech + EEPROM) or SaveKey (EEPROM only) controller.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AtariVox {
    /// `false` = SaveKey variant (speech side unfitted).
    speech_fitted: bool,
    eeprom: Eeprom24LC256,
    speak: SpeakJet,
    /// Last console drive state (`con_drv` = DDR mask, `con_lv` = latch).
    /// Bits clear in `con_drv` float to pull-up high.
    con_lv: u8,
    con_drv: u8,
    /// Last bus time seen (drives lazy speech-frame completion).
    seen: u64,
}

impl AtariVox {
    /// AtariVox with speech synthesis logging + EEPROM.
    pub fn new() -> Self {
        Self::with_speech(true)
    }

    /// SaveKey-compatible EEPROM with no speech hardware.
    pub fn savekey() -> Self {
        Self::with_speech(false)
    }

    fn with_speech(speech: bool) -> Self {
        AtariVox {
            speech_fitted: speech,
            eeprom: Eeprom24LC256::new(),
            speak: SpeakJet::new(speech),
            con_lv: 0x0F,
            con_drv: 0,
            seen: 0,
        }
    }

    pub fn speech_fitted(&self) -> bool {
        self.speech_fitted
    }

    /// Live EEPROM contents (32 KiB) for NVRAM persistence.
    pub fn nvram_image(&self) -> Vec<u8> {
        self.eeprom.mem.clone()
    }

    /// Restore EEPROM contents (short images zero-pad... actually 0xFF-pad
    /// like erased flash; long images truncate).
    pub fn load_nvram(&mut self, image: &[u8]) {
        let n = image.len().min(EEPROM_SIZE);
        self.eeprom.mem = vec![0xFFu8; EEPROM_SIZE];
        self.eeprom.mem[..n].copy_from_slice(&image[..n]);
    }

    /// Drain the received speech-phrase log (like `take_audio`); settles
    /// any frame whose tail ran into idle first.
    pub fn take_speech(&mut self) -> Vec<u8> {
        self.speak.poll(self.seen);
        std::mem::take(&mut self.speak.log).into()
    }

    /// Peek at the speech log without draining.
    pub fn speech_log(&self) -> Vec<u8> {
        self.speak.log.iter().copied().collect()
    }

    fn scl_level(&self) -> bool {
        if self.con_drv & 0x08 != 0 {
            self.con_lv & 0x08 != 0
        } else {
            true
        }
    }

    fn sda_bus(&self) -> bool {
        let con = if self.con_drv & 0x04 != 0 {
            self.con_lv & 0x04 != 0
        } else {
            true
        };
        con && !self.eeprom.sda_lo
    }
}

impl Default for AtariVox {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::tia_io::TiaPort for AtariVox {
    fn joy_r(&self) -> u8 {
        // No direction/button lines: everything released.
        0xFF
    }

    fn pot_x(&self) -> u8 {
        0xFF
    }

    fn pot_y(&self) -> u8 {
        0xFF
    }

    fn light_x(&self) -> u8 {
        0
    }

    fn light_y(&self) -> u8 {
        0
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

    fn pin_levels(&mut self, now: u64) -> u8 {
        self.seen = now;
        self.speak.poll(now);
        // Bit 0 DATA: console-driven or pull-up idle-high.
        let data = (self.con_drv & 0x01 == 0) || (self.con_lv & 0x01 != 0);
        // Bit 1 READY: low while the SpeakJet is busy (SaveKey: high).
        let ready = self.speak.ready(now);
        // Bit 2 SDA: console drive AND device open-drain.
        let sda = self.sda_bus();
        // Bit 3 SCL: console-driven or pull-up high.
        let scl = self.scl_level();
        (data as u8) | ((ready as u8) << 1) | ((sda as u8) << 2) | ((scl as u8) << 3)
    }

    fn pin_write(&mut self, levels: u8, mask: u8, now: u64) {
        self.seen = now;
        let levels = levels & 0x0F;
        let mask = mask & 0x0F;
        for i in [0u8, 2, 3] {
            let bit = 1 << i;
            let old_lv = if self.con_drv & bit != 0 {
                self.con_lv & bit != 0
            } else {
                true
            };
            let new_lv = if mask & bit != 0 {
                levels & bit != 0
            } else {
                true
            };
            if old_lv == new_lv {
                continue;
            }
            match i {
                0 => self.speak.data_edge(new_lv, now),
                2 => self.eeprom.drive_sda(mask & bit != 0, new_lv),
                _ => self.eeprom.drive_scl(new_lv),
            }
        }
        self.con_lv = (self.con_lv & !mask) | (levels & mask);
        self.con_drv = mask;
    }
}

// ---------------------------------------------------------------------------
// Tests (device-level bit-bang through the pin API)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tia_io::TiaPort;

    /// I2C bit-bang driver: returns SDA level read back after each op.
    struct Wire {
        dev: AtariVox,
        now: u64,
        /// Console latch + DDR (only SDA/SCL/DATA matter here).
        lv: u8,
        drv: u8,
    }

    impl Wire {
        fn new(speech: bool) -> Self {
            Wire {
                dev: if speech {
                    AtariVox::new()
                } else {
                    AtariVox::savekey()
                },
                now: 1_000_000,
                lv: 0x0F,
                drv: 0,
            }
        }

        fn tick(&mut self, cycles: u64) {
            self.now += cycles;
        }

        /// Drive DDR `mask` bits to `levels` (DDR is sticky: once a bit
        /// is an output it stays one until `sda_drive(false)` releases it).
        /// Instantaneous: tests advance time explicitly with `tick`, so
        /// UART cell widths are exact.
        fn drive(&mut self, levels: u8, mask: u8) {
            self.lv = (self.lv & !mask) | (levels & mask);
            self.drv |= mask & 0x0F;
            self.dev.pin_write(self.lv, self.drv, self.now);
        }

        fn set(&mut self, bit: u8, level: bool) {
            let lv = if level { self.lv | bit } else { self.lv & !bit };
            self.drive(lv, bit);
        }

        /// SDA direction flip (DDR write): driven both ways and released.
        fn sda_drive(&mut self, driven: bool) {
            if driven {
                self.drv |= 0x04;
            } else {
                self.drv &= !0x04;
            }
            self.dev.pin_write(self.lv, self.drv, self.now);
        }

        fn scl(&mut self, high: bool) {
            self.set(0x08, high);
        }

        fn sda(&mut self, high: bool) {
            self.set(0x04, high);
        }

        fn sda_read(&mut self) -> bool {
            self.dev.pin_levels(self.now) & 0x04 != 0
        }

        fn start(&mut self) {
            // Bus idle: SDA + SCL released high, then SDA falls.
            self.sda_drive(true);
            self.sda(true);
            self.drive(self.lv, 0x08); // SCL driven...
            self.scl(true);
            self.sda(false); // START
            self.scl(false);
        }

        fn stop(&mut self) {
            self.sda(false);
            self.scl(true);
            self.sda(true); // STOP
            self.sda_drive(false); // release SDA (DDR in)
            self.scl(false);
        }

        fn write_bit(&mut self, bit: bool) {
            self.sda(bit);
            self.scl(true);
            self.scl(false);
        }

        fn write_byte(&mut self, b: u8) -> bool {
            for i in (0..8).rev() {
                self.write_bit((b >> i) & 1 != 0);
            }
            // 9th clock: release SDA, sample ACK (low = ack).
            self.sda_drive(false);
            self.scl(true);
            let ack = !self.sda_read();
            self.scl(false);
            self.sda_drive(true);
            ack
        }

        fn read_byte(&mut self, ack: bool) -> u8 {
            self.sda_drive(false); // release for the device to drive
            let mut b = 0u8;
            for _ in 0..8 {
                self.scl(true);
                b = (b << 1) | (self.sda_read() as u8);
                self.scl(false);
            }
            // ACK clock: pull low to continue, release (NACK) to stop.
            self.sda_drive(true);
            self.sda(!ack);
            self.scl(true);
            self.scl(false);
            self.sda_drive(false);
            b
        }
    }

    fn write_mem(w: &mut Wire, addr: u16, data: &[u8]) -> bool {
        w.start();
        if !w.write_byte(CTRL_WRITE) {
            return false;
        }
        if !w.write_byte((addr >> 8) as u8) {
            return false;
        }
        if !w.write_byte(addr as u8) {
            return false;
        }
        for &b in data {
            if !w.write_byte(b) {
                return false;
            }
        }
        w.stop();
        true
    }

    fn read_mem(w: &mut Wire, addr: u16, n: usize) -> Option<Vec<u8>> {
        // Fake write to set the pointer, then read.
        w.start();
        if !w.write_byte(CTRL_WRITE) {
            return None;
        }
        if !w.write_byte((addr >> 8) as u8) {
            return None;
        }
        if !w.write_byte(addr as u8) {
            return None;
        }
        w.stop();
        w.start();
        if !w.write_byte(CTRL_READ) {
            return None;
        }
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            out.push(w.read_byte(i + 1 < n));
        }
        w.stop();
        Some(out)
    }

    /// Send one SpeakJet byte LSB-first with per-cell jitter.
    fn speak_byte(w: &mut Wire, b: u8, cell: u64, jitter: u64) {
        // DATA idle high, driven output.
        w.drive(0x01, 0x01);
        w.tick(cell * 3);
        // Start bit (low).
        w.set(0x01, false);
        w.tick(cell);
        // Data bits LSB first with alternating jitter.
        let mut j = jitter;
        for i in 0..8 {
            w.set(0x01, (b >> i) & 1 != 0);
            w.tick(cell + j);
            j = if j == 0 { jitter } else { 0 };
        }
        // Stop bit (high), then idle.
        w.set(0x01, true);
        w.tick(cell * 3);
    }

    #[test]
    fn eeprom_powers_up_with_syspage_signature() {
        let d = AtariVox::new();
        assert_eq!(&d.eeprom.mem[0..8], b"ATARIVOX");
        assert_eq!(d.eeprom.mem[8], 0x80);
        assert!(d.eeprom.mem[9..].iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn eeprom_write_then_read_roundtrip() {
        let mut w = Wire::new(true);
        assert!(write_mem(&mut w, 0x0140, b"HELLO-7800"));
        assert_eq!(read_mem(&mut w, 0x0140, 10).unwrap(), b"HELLO-7800");
        // Untouched area still erased.
        assert_eq!(read_mem(&mut w, 0x0200, 4).unwrap(), vec![0xFF; 4]);
    }

    #[test]
    fn eeprom_writes_wrap_within_page() {
        let mut w = Wire::new(true);
        // 0x013E + 4 bytes: last two wrap to the start of the same page.
        assert!(write_mem(&mut w, 0x013E, &[0xAA, 0xBB, 0xCC, 0xDD]));
        assert_eq!(read_mem(&mut w, 0x013E, 2).unwrap(), vec![0xAA, 0xBB]);
        assert_eq!(read_mem(&mut w, 0x0100, 2).unwrap(), vec![0xCC, 0xDD]);
    }

    #[test]
    fn eeprom_naks_unknown_control_byte() {
        let mut w = Wire::new(true);
        w.start();
        assert!(!w.write_byte(0xB0)); // neither $A0 nor $A1: NACK
        w.stop();
    }

    #[test]
    fn eeprom_stop_via_sda_release_counts() {
        // DDR-release STOP: SDA driven low, SCL high, then SDA released.
        let mut w = Wire::new(true);
        w.start();
        assert!(w.write_byte(CTRL_WRITE));
        assert!(w.write_byte(0x03));
        assert!(w.write_byte(0x00));
        assert!(w.write_byte(0x5A));
        // STOP without driving SDA high: release the pin (DDR in).
        w.sda(false);
        w.scl(true);
        w.sda_drive(false); // floats high while SCL high = STOP
        w.scl(false);
        assert_eq!(read_mem(&mut w, 0x0300, 1).unwrap(), vec![0x5A]);
    }

    #[test]
    fn uart_decodes_bytes_with_jitter_and_busies_ready() {
        let mut w = Wire::new(true);
        assert!(w.dev.pin_levels(w.now) & 0x02 != 0); // READY idle high
        speak_byte(&mut w, 0x55, UART_NOMINAL_CELL, 9);
        assert!(w.dev.pin_levels(w.now) & 0x02 == 0); // busy right after
        speak_byte(&mut w, 0xA3, UART_NOMINAL_CELL + 7, 11); // off-nominal cell
                                                             // A game polls READY between bytes; the poll completes a frame
                                                             // whose stop tail ran into idle (take_speech alone only sees
                                                             // the last bus time).
        assert!(w.dev.pin_levels(w.now) & 0x02 == 0);
        assert_eq!(w.dev.take_speech(), vec![0x55, 0xA3]);
        // Busy window expires.
        w.tick(SPEECH_BUSY_CYCLES * 2);
        w.drive(w.lv, 0x00); // any notify refreshes nothing; poll pin
        assert!(w.dev.pin_levels(w.now) & 0x02 != 0);
    }

    #[test]
    fn uart_bad_stop_bit_drops_byte() {
        let mut w = Wire::new(true);
        w.drive(0x01, 0x01);
        w.tick(300);
        w.set(0x01, false); // start
        w.tick(UART_NOMINAL_CELL);
        for i in 0..8 {
            w.set(0x01, (0x41 >> i) & 1 != 0);
            w.tick(UART_NOMINAL_CELL);
        }
        w.set(0x01, false); // bad stop (low)
        w.tick(UART_NOMINAL_CELL * 3);
        w.set(0x01, true);
        w.tick(UART_NOMINAL_CELL * 3);
        assert!(w.dev.take_speech().is_empty());
    }

    #[test]
    fn savekey_ignores_speech_but_keeps_eeprom() {
        let mut w = Wire::new(false);
        speak_byte(&mut w, 0x55, UART_NOMINAL_CELL, 0);
        assert!(w.dev.take_speech().is_empty());
        assert!(w.dev.pin_levels(w.now) & 0x02 != 0); // READY always high
        assert!(write_mem(&mut w, 0x0100, &[0x11, 0x22]));
        assert_eq!(read_mem(&mut w, 0x0100, 2).unwrap(), vec![0x11, 0x22]);
    }

    #[test]
    fn nvram_image_roundtrip() {
        let mut w = Wire::new(true);
        assert!(write_mem(&mut w, 0x1000, &[1, 2, 3, 4]));
        let img = w.dev.nvram_image();
        assert_eq!(img.len(), EEPROM_SIZE);
        let mut w2 = Wire::new(true);
        w2.dev.load_nvram(&img);
        assert_eq!(read_mem(&mut w2, 0x1000, 4).unwrap(), vec![1, 2, 3, 4]);
        // Short images 0xFF-pad.
        w2.dev.load_nvram(&[0xAA]);
        assert_eq!(w2.dev.nvram_image()[0], 0xAA);
        assert_eq!(w2.dev.nvram_image()[1], 0xFF);
    }

    #[test]
    fn nack_ends_read_and_bus_releases() {
        let mut w = Wire::new(true);
        assert!(write_mem(&mut w, 0x0050, &[0xDE, 0xAD]));
        let bytes = read_mem(&mut w, 0x0050, 2).unwrap();
        assert_eq!(bytes, vec![0xDE, 0xAD]);
        // After NACK + STOP the bus is free: a fresh START works.
        assert_eq!(read_mem(&mut w, 0x0050, 1).unwrap(), vec![0xDE]);
    }
}

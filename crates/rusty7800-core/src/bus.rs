//! Rusty7800 `bus` — address map, mirrors, and access stall accounting.
//!
//! Phase 1 skeleton against `a7800.cpp` `ADDRESS_MAP_START(a7800_mem, …)`.
//! TIA/MARIA/RIOT/cart devices attach in later phases; this module owns the
//! decode, the RAM, the mirrors, and the slow-access classification.
//!
//! Stall model: the machine config notes SALLY "switches to 1.19 MHz on TIA
//! or RIOT RAM access" (1.789772 / 1.19 ≈ 1.5×). Costs are counted in
//! half-cycles: fast regions cost 2 (one CPU cycle), TIA-register and
//! RIOT-RAM accesses cost 3. The machine loop accumulates halves and ticks
//! devices with whole cycles.

use crate::sally::CpuBus;

/// Half-cycle cost of a fast (1×) byte access.
pub const COST_FAST_HALF: u8 = 2;
/// Half-cycle cost of a slow (1.5×) byte access (TIA regs, RIOT RAM).
pub const COST_SLOW_HALF: u8 = 3;

/// Decoded region of an access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Tia,
    Maria,
    ZpMirror,
    SpMirror,
    Riot,
    RiotRam,
    Sram1,
    Sram2,
    Cart,
    /// 0x0060–0x00FF etc. gaps that hit nothing in Phase 1 (open bus).
    Open,
}

/// Classify an address without performing the access.
pub fn decode(addr: u16) -> Region {
    match addr {
        0x0000..=0x001F => Region::Tia,
        0x0020..=0x003F => Region::Maria,
        0x0040..=0x00FF => Region::ZpMirror,
        0x0100..=0x013F => Region::Open,
        0x0140..=0x01FF => Region::SpMirror,
        0x0200..=0x027F => Region::Open,
        0x0280..=0x029F => Region::Riot,
        0x02A0..=0x047F => Region::Open,
        0x0480..=0x04FF => Region::RiotRam,
        0x0500..=0x17FF => Region::Open,
        0x1800..=0x1FFF => Region::Sram1,
        0x2000..=0x27FF => Region::Sram2,
        0x2800..=0x3FFF => Region::Open,
        0x4000..=0xFFFF => Region::Cart,
    }
}

/// Half-cycle cost of accessing `addr`.
pub fn access_cost_half(addr: u16) -> u8 {
    match decode(addr) {
        // Slow: TIA registers and RIOT RAM (see module docs).
        Region::Tia | Region::RiotRam => COST_SLOW_HALF,
        _ => COST_FAST_HALF,
    }
}

/// Phase 1 memory: RAM + mirrors. Device-backed regions (TIA/MARIA/RIOT/
/// cart) return open-bus 0xFF / ignore writes until their phases land.
pub struct Memory {
    /// 6116_1 SRAM, 0x1800–0x1FFF.
    pub sram1: [u8; 2048],
    /// 6116_2 SRAM, 0x2000–0x27FF (hosts zp/sp mirrors).
    pub sram2: [u8; 4096],
    /// RIOT RAM, 0x0480–0x04FF.
    pub riot_ram: [u8; 128],
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory {
    pub fn new() -> Self {
        Memory {
            sram1: [0; 2048],
            sram2: [0; 4096],
            riot_ram: [0; 128],
        }
    }

    /// Raw read without cost (devices use this internally).
    ///
    /// Mirrors (per machine_start membanks): 0x0040–0x00FF aliases
    /// sram2[0x40..0x100] (i.e. 0x2040–0x20FF); 0x0140–0x01FF aliases
    /// sram2[0x140..0x200] (i.e. 0x2140–0x21FF).
    pub fn read_raw(&self, addr: u16) -> u8 {
        match decode(addr) {
            Region::ZpMirror | Region::SpMirror => self.sram2[addr as usize],
            Region::RiotRam => self.riot_ram[(addr & 0x7F) as usize],
            Region::Sram1 => self.sram1[(addr - 0x1800) as usize],
            Region::Sram2 => self.sram2[(addr - 0x2000) as usize],
            _ => 0xFF,
        }
    }

    /// Raw write without cost.
    pub fn write_raw(&mut self, addr: u16, val: u8) {
        match decode(addr) {
            Region::ZpMirror | Region::SpMirror => self.sram2[addr as usize] = val,
            Region::RiotRam => self.riot_ram[(addr & 0x7F) as usize] = val,
            Region::Sram1 => self.sram1[(addr - 0x1800) as usize] = val,
            Region::Sram2 => self.sram2[(addr - 0x2000) as usize] = val,
            _ => {}
        }
    }
}

impl CpuBus for Memory {
    fn read(&mut self, addr: u16) -> u8 {
        self.read_raw(addr)
    }

    fn write(&mut self, addr: u16, val: u8) {
        self.write_raw(addr, val);
    }
}

/// Tracks half-cycle accumulation for device ticking.
#[derive(Default, Debug)]
pub struct Clock {
    half: u64,
}

impl Clock {
    pub fn new() -> Self {
        Clock { half: 0 }
    }

    /// Account one access to `addr`.
    pub fn access(&mut self, addr: u16) {
        self.half += access_cost_half(addr) as u64;
    }

    /// Whole CPU cycles elapsed (fast accesses).
    pub fn cycles(&self) -> u64 {
        self.half / 2
    }

    pub fn half_cycles(&self) -> u64 {
        self.half
    }
}

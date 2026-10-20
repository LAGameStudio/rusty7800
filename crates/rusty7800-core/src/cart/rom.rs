//! Standard ROM PCBs: flat, POKEY, SuperGame, Mirror-RAM, Absolute, Activision.
//!
//! Phase 5 port of `a7800-5.2/src/devices/bus/a7800/rom.{h,cpp}`
//! (BSD-3-Clause). Methods take full CPU addresses and subtract the
//! `$4000` window base to obtain the C++ `offset`. POKEY register traffic
//! goes through [`CartBus`](super::CartBus) (the machine owns the audio
//! device); `pokey_bases()` reports the decoded base for the address map.
//!
//! Open question from `rom.cpp:15` ("Are POKEY regs readable somewhere in
//! SG 144K + POKEY homebrew?"): preserved as written — SG+POKEY carts
//! expose POKEY registers on reads of `$4000–$7FFF`, and the banked ROM
//! that would otherwise live there is unreadable in that window. The
//! `sg_pokey_visibility` test pins this outcome.

use super::{bank_mask, base_rom, bidx, Cart, CartBus};

/// Shared ROM image + bank state (`m_rom`/`m_bank`/`m_bank_mask`).
#[derive(Clone, Debug)]
pub struct RomImage {
    pub rom: Vec<u8>,
    pub bank: i32,
    pub mask: i32,
    pub base: u32,
}

impl RomImage {
    pub fn new(rom: Vec<u8>) -> Self {
        let mask = bank_mask(rom.len());
        let base = base_rom(rom.len());
        RomImage { rom, bank: 0, mask, base }
    }

    pub fn reset(&mut self) {
        self.bank = 0;
    }

    /// Flat-map read: bytes below the top-of-memory base are open bus.
    pub fn flat(&self, off: u32) -> u8 {
        let addr = off + 0x4000;
        if addr < self.base {
            0xFF
        } else {
            self.rom[(addr - self.base) as usize]
        }
    }
}

/// Flat cart, no bankswitch (`a78_rom`; 8K–48K).
#[derive(Clone, Debug)]
pub struct Flat {
    pub img: RomImage,
}

impl Flat {
    pub fn new(rom: Vec<u8>) -> Self {
        Flat { img: RomImage::new(rom) }
    }
}

impl Cart for Flat {
    fn read_40xx(&mut self, addr: u16, _maria: bool, _bus: &mut dyn CartBus) -> u8 {
        self.img.flat((addr as u32).wrapping_sub(0x4000))
    }
    fn reset(&mut self) {
        self.img.reset();
    }
}

/// Flat cart + POKEY@`$4000` (`a78_pokey`; POKEY on `$4000–$7FFF` writes
/// and reads, i.e. `offset < 0x4000`).
#[derive(Clone, Debug)]
pub struct FlatPokey {
    pub img: RomImage,
}

impl FlatPokey {
    pub fn new(rom: Vec<u8>) -> Self {
        FlatPokey { img: RomImage::new(rom) }
    }
}

impl Cart for FlatPokey {
    fn read_40xx(&mut self, addr: u16, _maria: bool, bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x4000);
        if off < 0x4000 {
            bus.pokey_read(0x4000, (off & 0x0F) as u8)
        } else {
            self.img.flat(off as u32)
        }
    }
    fn write_40xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        let off = addr.wrapping_sub(0x4000);
        if off < 0x4000 {
            bus.pokey_write(0x4000, (off & 0x0F) as u8, data);
        }
    }
    fn pokey_bases(&self) -> Vec<u16> {
        vec![0x4000]
    }
    fn reset(&mut self) {
        self.img.reset();
    }
}

/// Mirror-RAM cart (`a78_mram`): 16K RAM at `$4000–$7FFF` with the A8 line
/// unused (`offset & 0xFEFF` mirrors even pages onto odd pages).
#[derive(Clone, Debug)]
pub struct MirrorRam {
    pub img: RomImage,
    pub ram: Vec<u8>,
}

impl MirrorRam {
    pub fn new(rom: Vec<u8>, ram: Vec<u8>) -> Self {
        debug_assert_eq!(ram.len(), 0x4000);
        MirrorRam { img: RomImage::new(rom), ram }
    }
}

impl Cart for MirrorRam {
    fn read_40xx(&mut self, addr: u16, _maria: bool, _bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x4000);
        if off < 0x4000 {
            self.ram[(off & 0xFEFF) as usize]
        } else {
            self.img.flat(off as u32)
        }
    }
    fn write_40xx(&mut self, addr: u16, data: u8, _bus: &mut dyn CartBus) {
        let off = addr.wrapping_sub(0x4000);
        if off < 0x4000 {
            self.ram[(off & 0xFEFF) as usize] = data;
        }
    }
    fn reset(&mut self) {
        self.img.reset();
    }
}

/// SuperGame bankswitch (`a78_sg`): 8×16K banks in `$8000–$BFFF`,
/// bank 7 fixed in `$C000–$FFFF`, second-to-last bank in `$4000–$7FFF`
/// (games expecting open bus there are assumed none — see C++ comment).
/// Writes to `$8000–$BFFF` select `data & mask`.
#[derive(Clone, Debug)]
pub struct SuperGame {
    pub img: RomImage,
}

impl SuperGame {
    pub fn new(rom: Vec<u8>) -> Self {
        SuperGame { img: RomImage::new(rom) }
    }
}

impl Cart for SuperGame {
    fn read_40xx(&mut self, addr: u16, _maria: bool, _bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x4000) as usize;
        let (rom, bank, mask) = (&self.img.rom, self.img.bank as i64, self.img.mask as i64);
        if off < 0x4000 {
            bidx(rom, (off & 0x3FFF) as i64 + (mask - 1) * 0x4000)
        } else if off < 0x8000 {
            bidx(rom, (off & 0x3FFF) as i64 + bank * 0x4000)
        } else {
            bidx(rom, (off & 0x3FFF) as i64 + mask * 0x4000)
        }
    }
    fn write_40xx(&mut self, addr: u16, data: u8, _bus: &mut dyn CartBus) {
        let off = addr.wrapping_sub(0x4000);
        if (0x4000..0x8000).contains(&off) {
            self.img.bank = data as i32 & self.img.mask;
        }
    }
    fn reset(&mut self) {
        self.img.reset();
    }
}

/// SuperGame + POKEY (`a78_sg_pokey`): as SG, but `$4000–$7FFF` reads and
/// writes hit the POKEY instead of the second-to-last bank.
#[derive(Clone, Debug)]
pub struct SuperGamePokey {
    pub img: RomImage,
}

impl SuperGamePokey {
    pub fn new(rom: Vec<u8>) -> Self {
        SuperGamePokey { img: RomImage::new(rom) }
    }
}

impl Cart for SuperGamePokey {
    fn read_40xx(&mut self, addr: u16, _maria: bool, bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x4000) as usize;
        let (rom, bank, mask) = (&self.img.rom, self.img.bank as i64, self.img.mask as i64);
        if off < 0x4000 {
            bus.pokey_read(0x4000, (off & 0x0F) as u8)
        } else if off < 0x8000 {
            bidx(rom, (off & 0x3FFF) as i64 + bank * 0x4000)
        } else {
            bidx(rom, (off & 0x3FFF) as i64 + mask * 0x4000)
        }
    }
    fn write_40xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        let off = addr.wrapping_sub(0x4000);
        if off < 0x4000 {
            bus.pokey_write(0x4000, (off & 0x0F) as u8, data);
        } else if off < 0x8000 {
            self.img.bank = data as i32 & self.img.mask;
        }
    }
    fn pokey_bases(&self) -> Vec<u16> {
        vec![0x4000]
    }
    fn reset(&mut self) {
        self.img.reset();
    }
}

/// SuperGame + 16K RAM (`a78_sg_ram`): RAM at `$4000–$7FFF` replaces the
/// second-to-last bank window (read and write).
#[derive(Clone, Debug)]
pub struct SuperGameRam {
    pub img: RomImage,
    pub ram: Vec<u8>,
}

impl SuperGameRam {
    pub fn new(rom: Vec<u8>, ram: Vec<u8>) -> Self {
        debug_assert_eq!(ram.len(), 0x4000);
        SuperGameRam { img: RomImage::new(rom), ram }
    }
}

impl Cart for SuperGameRam {
    fn read_40xx(&mut self, addr: u16, _maria: bool, _bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x4000) as usize;
        let (rom, bank, mask) = (&self.img.rom, self.img.bank as i64, self.img.mask as i64);
        if off < 0x4000 {
            self.ram[off]
        } else if off < 0x8000 {
            bidx(rom, (off & 0x3FFF) as i64 + bank * 0x4000)
        } else {
            bidx(rom, (off & 0x3FFF) as i64 + mask * 0x4000)
        }
    }
    fn write_40xx(&mut self, addr: u16, data: u8, _bus: &mut dyn CartBus) {
        let off = addr.wrapping_sub(0x4000) as usize;
        if off < 0x4000 {
            self.ram[off] = data;
        } else if off < 0x8000 {
            self.img.bank = data as i32 & self.img.mask;
        }
    }
    fn reset(&mut self) {
        self.img.reset();
    }
}

/// SuperGame 9-bank (`a78_sg9`, 144K): first 16K fixed at `$4000–$7FFF`,
/// banks 1–8 selectable in `$8000–$BFFF` (`m_bank = (data & mask) + 1`),
/// last bank fixed at `$C000–$FFFF`.
#[derive(Clone, Debug)]
pub struct SuperGame9 {
    pub img: RomImage,
}

impl SuperGame9 {
    pub fn new(rom: Vec<u8>) -> Self {
        SuperGame9 { img: RomImage::new(rom) }
    }
}

impl Cart for SuperGame9 {
    fn read_40xx(&mut self, addr: u16, _maria: bool, _bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x4000) as usize;
        let (rom, bank, mask) = (&self.img.rom, self.img.bank as i64, self.img.mask as i64);
        if off < 0x4000 {
            bidx(rom, (off & 0x3FFF) as i64)
        } else if off < 0x8000 {
            bidx(rom, (off & 0x3FFF) as i64 + bank * 0x4000)
        } else {
            bidx(rom, (off & 0x3FFF) as i64 + (mask + 1) * 0x4000)
        }
    }
    fn write_40xx(&mut self, addr: u16, data: u8, _bus: &mut dyn CartBus) {
        let off = addr.wrapping_sub(0x4000);
        if (0x4000..0x8000).contains(&off) {
            self.img.bank = (data as i32 & self.img.mask) + 1;
        }
    }
    fn reset(&mut self) {
        // NOTE: C++ `device_reset` zeroes m_bank even though bank 0 is
        // never visible (writes always add 1); kept for parity.
        self.img.reset();
    }
}

/// Absolute bankswitch (`a78_abs`, F-18 Hornet): 64K; lower 32K are two
/// 16K banks in `$4000–$7FFF` selected by a write to `$8000` (bit 0 →
/// bank 0, else bit 1 → bank 1); upper 32K fixed at `$8000–$FFFF`.
#[derive(Clone, Debug)]
pub struct Absolute {
    pub img: RomImage,
}

impl Absolute {
    pub fn new(rom: Vec<u8>) -> Self {
        Absolute { img: RomImage::new(rom) }
    }
}

impl Cart for Absolute {
    fn read_40xx(&mut self, addr: u16, _maria: bool, _bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x4000) as usize;
        let (rom, bank) = (&self.img.rom, self.img.bank as i64);
        if off < 0x4000 {
            bidx(rom, (off & 0x3FFF) as i64 + bank * 0x4000)
        } else {
            bidx(rom, off as i64 - 0x4000 + 0x8000)
        }
    }
    fn write_40xx(&mut self, addr: u16, data: u8, _bus: &mut dyn CartBus) {
        if addr.wrapping_sub(0x4000) == 0x4000 {
            if data & 1 != 0 {
                self.img.bank = 0;
            } else if data & 2 != 0 {
                self.img.bank = 1;
            }
        }
    }
    fn reset(&mut self) {
        self.img.reset();
    }
}

/// Activision bankswitch (`a78_act`, 128K): banks 0–7 selectable at
/// `$A000–$DFFF` by writes to `$FF80–$FF87` (`m_bank = offset & 7`);
/// `$4000–$5FFF` = bank 6 upper half, `$6000–$7FFF` = bank 6 lower half,
/// `$8000–$9FFF` = bank 7 upper half, `$E000–$FFFF` = bank 7 lower half.
#[derive(Clone, Debug)]
pub struct Activision {
    pub img: RomImage,
}

impl Activision {
    pub fn new(rom: Vec<u8>) -> Self {
        Activision { img: RomImage::new(rom) }
    }
}

impl Cart for Activision {
    fn read_40xx(&mut self, addr: u16, _maria: bool, _bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x4000) as usize;
        let (rom, bank) = (&self.img.rom, self.img.bank as i64);
        let a = (off & 0x1FFF) as i64;
        match off & 0xE000 {
            0x0000 => bidx(rom, a + 0x1A000),
            0x2000 => bidx(rom, a + 0x18000),
            0x4000 => bidx(rom, a + 0x1E000),
            0x6000 => bidx(rom, a + bank * 0x4000),
            0x8000 => bidx(rom, a + bank * 0x4000 + 0x2000),
            _ => bidx(rom, a + 0x1C000),
        }
    }
    fn write_40xx(&mut self, addr: u16, _data: u8, _bus: &mut dyn CartBus) {
        let off = addr.wrapping_sub(0x4000);
        if (0xBF80..=0xBF8F).contains(&off) {
            self.img.bank = (off & 7) as i32;
        }
    }
    fn reset(&mut self) {
        self.img.reset();
    }
}

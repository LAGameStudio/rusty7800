//! CPUWIZ homebrew boards: VersaBoard and MegaCart+.
//!
//! Phase 5 port of `a7800-5.2/src/devices/bus/a7800/cpuwiz.{h,cpp}`
//! (BSD-3-Clause). VersaBoard = up to 256K ROM (SuperGame-style banking
//! with `(data & 0x0F) & mask`) + 2×16K RAM at `$4000–$7FFF` selected by
//! `BIT(data, 5)` on bank writes. MegaCart+ = up to 512K ROM
//! (`(data & 0x1F) & mask`), same RAM scheme. The P450_VB flavor adds a
//! POKEY@`$0450` window (composed in `p450.rs`).

use super::{bank_mask, Cart, CartBus};

/// Shared Versa/Mega state (`m_bank` + `m_ram_bank`, both zero at reset).
#[derive(Clone, Debug)]
pub struct VersaImage {
    pub rom: Vec<u8>,
    pub ram: Vec<u8>,
    pub bank: i32,
    pub ram_bank: i32,
    pub mask: i32,
    /// Bank-select data mask: `0x0F` (Versa, 256K) or `0x1F` (Mega, 512K).
    pub bank_bits: u8,
}

impl VersaImage {
    pub fn new(rom: Vec<u8>, ram: Vec<u8>, bank_bits: u8) -> Self {
        debug_assert_eq!(ram.len(), 0x8000);
        VersaImage { mask: bank_mask(rom.len()), rom, ram, bank: 0, ram_bank: 0, bank_bits }
    }

    pub fn reset(&mut self) {
        self.bank = 0;
        self.ram_bank = 0;
    }

    pub fn read(&self, off: usize) -> u8 {
        if off < 0x4000 {
            self.ram[off + (self.ram_bank as usize) * 0x4000]
        } else if off < 0x8000 {
            self.rom[(off & 0x3FFF) + (self.bank as usize) * 0x4000]
        } else {
            self.rom[(off & 0x3FFF) + (self.mask as usize) * 0x4000]
        }
    }

    pub fn write(&mut self, off: usize, data: u8) {
        if off < 0x4000 {
            self.ram[off + (self.ram_bank as usize) * 0x4000] = data;
        } else if off < 0x8000 {
            self.bank = (data & self.bank_bits) as i32 & self.mask;
            self.ram_bank = ((data >> 5) & 1) as i32;
        }
    }
}

/// VersaBoard (`a78_versa`).
#[derive(Clone, Debug)]
pub struct Versa {
    pub img: VersaImage,
}

impl Versa {
    pub fn new(rom: Vec<u8>, ram: Vec<u8>) -> Self {
        Versa { img: VersaImage::new(rom, ram, 0x0F) }
    }
}

impl Cart for Versa {
    fn read_40xx(&mut self, addr: u16, _maria: bool, _bus: &mut dyn CartBus) -> u8 {
        self.img.read(addr.wrapping_sub(0x4000) as usize)
    }
    fn write_40xx(&mut self, addr: u16, data: u8, _bus: &mut dyn CartBus) {
        self.img.write(addr.wrapping_sub(0x4000) as usize, data);
    }
    fn reset(&mut self) {
        self.img.reset();
    }
}

/// MegaCart+ (`a78_megacart`): same windows, 5 bank-select bits.
#[derive(Clone, Debug)]
pub struct Mega {
    pub img: VersaImage,
}

impl Mega {
    pub fn new(rom: Vec<u8>, ram: Vec<u8>) -> Self {
        Mega { img: VersaImage::new(rom, ram, 0x1F) }
    }
}

impl Cart for Mega {
    fn read_40xx(&mut self, addr: u16, _maria: bool, _bus: &mut dyn CartBus) -> u8 {
        self.img.read(addr.wrapping_sub(0x4000) as usize)
    }
    fn write_40xx(&mut self, addr: u16, data: u8, _bus: &mut dyn CartBus) {
        self.img.write(addr.wrapping_sub(0x4000) as usize, data);
    }
    fn reset(&mut self) {
        self.img.reset();
    }
}

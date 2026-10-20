//! Cart variants with a POKEY at `$0450` (VersaBoard/homebrew PCBs).
//!
//! Phase 5 port of the `a78_rom_p450_*` family in
//! `a7800-5.2/src/devices/bus/a7800/rom.h` (BSD-3-Clause). The POKEY
//! answers only for `offset` in `0x50–0x5F` (`$0450–$045F`); the rest of
//! the `04xx` window is open bus. Read/write behavior is factored into
//! [`Pokey450`], composed over the flat / flat+POKEY / SG / SG+RAM / SG9
//! base. (`a78_p450_vb` = VersaBoard + POKEY lives in `cpuwiz.rs` +
//! [`VersaPokey`] below.)

use super::{Cart, CartBus};

/// Mixin: POKEY@`$0450` window over any base cart.
#[derive(Clone, Debug)]
pub struct Pokey450<C> {
    pub base: C,
}

impl<C> Pokey450<C> {
    pub fn new(base: C) -> Self {
        Pokey450 { base }
    }
}

impl<C: Cart> Cart for Pokey450<C> {
    fn read_04xx(&mut self, addr: u16, bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x0400);
        if (0x50..0x60).contains(&off) {
            bus.pokey_read(0x0450, (off & 0x0F) as u8)
        } else {
            0xFF
        }
    }
    fn write_04xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        let off = addr.wrapping_sub(0x0400);
        if (0x50..0x60).contains(&off) {
            bus.pokey_write(0x0450, (off & 0x0F) as u8, data);
        }
    }
    fn read_08xx(&mut self, addr: u16, bus: &mut dyn CartBus) -> u8 {
        self.base.read_08xx(addr, bus)
    }
    fn write_08xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        self.base.write_08xx(addr, data, bus);
    }
    fn read_10xx(&mut self, addr: u16, bus: &mut dyn CartBus) -> u8 {
        self.base.read_10xx(addr, bus)
    }
    fn write_10xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        self.base.write_10xx(addr, data, bus);
    }
    fn read_30xx(&mut self, addr: u16, maria: bool, bus: &mut dyn CartBus) -> u8 {
        self.base.read_30xx(addr, maria, bus)
    }
    fn read_40xx(&mut self, addr: u16, maria: bool, bus: &mut dyn CartBus) -> u8 {
        self.base.read_40xx(addr, maria, bus)
    }
    fn write_40xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        self.base.write_40xx(addr, data, bus);
    }
    fn pokey_bases(&self) -> Vec<u16> {
        let mut bases = vec![0x0450];
        bases.extend(self.base.pokey_bases());
        bases
    }
    fn ym_present(&self) -> bool {
        self.base.ym_present()
    }
    fn reset(&mut self) {
        self.base.reset();
    }
}

/// VersaBoard + POKEY@`$0450` (`a78_p450_vb`).
#[derive(Clone, Debug)]
pub struct VersaPokey {
    pub base: super::cpuwiz::Versa,
}

impl VersaPokey {
    pub fn new(rom: Vec<u8>, ram: Vec<u8>) -> Self {
        VersaPokey { base: super::cpuwiz::Versa::new(rom, ram) }
    }
}

impl Cart for VersaPokey {
    fn read_04xx(&mut self, addr: u16, bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x0400);
        if (0x50..0x60).contains(&off) {
            bus.pokey_read(0x0450, (off & 0x0F) as u8)
        } else {
            0xFF
        }
    }
    fn write_04xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        let off = addr.wrapping_sub(0x0400);
        if (0x50..0x60).contains(&off) {
            bus.pokey_write(0x0450, (off & 0x0F) as u8, data);
        }
    }
    fn read_40xx(&mut self, addr: u16, maria: bool, bus: &mut dyn CartBus) -> u8 {
        self.base.read_40xx(addr, maria, bus)
    }
    fn write_40xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        self.base.write_40xx(addr, data, bus);
    }
    fn pokey_bases(&self) -> Vec<u16> {
        vec![0x0450]
    }
    fn reset(&mut self) {
        self.base.reset();
    }
}

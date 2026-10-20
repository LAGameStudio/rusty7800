//! Cart variants with a POKEY at `$0800` (homebrew PCBs).
//!
//! Phase 5 port of the `a78_rom_p800_*` family in
//! `a7800-5.2/src/devices/bus/a7800/rom.h` plus the bankset P800 flavors
//! in `bankset.h` (BSD-3-Clause). The whole `08xx` window
//! (`$0800–$0FFF`) hits the POKEY with `offset & 0x0F` (no sub-range
//! gate, unlike P450). Factored into [`Pokey800`], composed over the
//! flat / flat+POKEY / SG / SG+RAM / SG9 / bankset bases.

use super::{Cart, CartBus};

/// Mixin: POKEY@`$0800` window over any base cart.
#[derive(Clone, Debug)]
pub struct Pokey800<C> {
    pub base: C,
}

impl<C> Pokey800<C> {
    pub fn new(base: C) -> Self {
        Pokey800 { base }
    }
}

impl<C: Cart> Cart for Pokey800<C> {
    fn read_04xx(&mut self, addr: u16, bus: &mut dyn CartBus) -> u8 {
        self.base.read_04xx(addr, bus)
    }
    fn write_04xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        self.base.write_04xx(addr, data, bus);
    }
    fn read_08xx(&mut self, addr: u16, bus: &mut dyn CartBus) -> u8 {
        bus.pokey_read(0x0800, (addr.wrapping_sub(0x0800) & 0x0F) as u8)
    }
    fn write_08xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        bus.pokey_write(0x0800, (addr.wrapping_sub(0x0800) & 0x0F) as u8, data);
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
        let mut bases = vec![0x0800];
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

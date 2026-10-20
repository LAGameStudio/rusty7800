//! High Score passthrough cart (`a78_hsc`; 4K ROM + 2K NVRAM).
//!
//! Phase 5 port of `a7800-5.2/src/devices/bus/a7800/hiscore.{h,cpp}`
//! (BSD-3-Clause). NVRAM at `$1000–$17FF` and ROM at `$3000–$3FFF`
//! (installed by `machine_start`); `04xx`/`40xx` fall through to the
//! top-slot cart (`inner`).

use super::{load_nvram, Cart, CartBus, SavedCartState};

/// High Score cart with an optional top-slot passthrough cart.
pub struct Hiscore {
    /// 4K HSC ROM (`highscre.bin`, CRC `9be408d3`).
    pub rom: Vec<u8>,
    /// 2K battery-backed NVRAM (`0xFF`-filled at load).
    pub nvram: Vec<u8>,
    pub inner: Option<Box<dyn Cart>>,
}

impl Hiscore {
    pub fn new(rom: Vec<u8>, nvram_image: Option<&[u8]>) -> Self {
        debug_assert_eq!(rom.len(), 0x1000);
        Hiscore { rom, nvram: load_nvram(nvram_image, 0x800), inner: None }
    }
}

impl Cart for Hiscore {
    fn save_state(&self) -> SavedCartState {
        let mut s = SavedCartState::versioned();
        s.nvram = self.nvram.clone();
        s.inner = self.inner.as_ref().map(|c| Box::new(c.save_state()));
        s
    }
    fn load_state(&mut self, s: &SavedCartState) {
        s.apply_nvram(&mut self.nvram);
        if let (Some(inner), Some(saved)) = (self.inner.as_mut(), s.inner.as_ref()) {
            inner.load_state(saved);
        }
    }
    fn nvram_image(&self) -> Option<Vec<u8>> {
        Some(self.nvram.clone())
    }
    fn load_nvram(&mut self, image: &[u8]) {
        let n = image.len().min(self.nvram.len());
        self.nvram[..n].copy_from_slice(&image[..n]);
    }
    fn read_10xx(&mut self, addr: u16, _bus: &mut dyn CartBus) -> u8 {
        self.nvram[addr.wrapping_sub(0x1000) as usize]
    }

    fn write_10xx(&mut self, addr: u16, data: u8, _bus: &mut dyn CartBus) {
        self.nvram[addr.wrapping_sub(0x1000) as usize] = data;
    }

    fn read_30xx(&mut self, addr: u16, _maria: bool, _bus: &mut dyn CartBus) -> u8 {
        self.rom[addr.wrapping_sub(0x3000) as usize]
    }

    fn read_04xx(&mut self, addr: u16, bus: &mut dyn CartBus) -> u8 {
        if let Some(inner) = self.inner.as_mut() {
            inner.read_04xx(addr, bus)
        } else {
            0xFF
        }
    }

    fn write_04xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        if let Some(inner) = self.inner.as_mut() {
            inner.write_04xx(addr, data, bus);
        }
    }

    fn read_40xx(&mut self, addr: u16, maria: bool, bus: &mut dyn CartBus) -> u8 {
        if let Some(inner) = self.inner.as_mut() {
            inner.read_40xx(addr, maria, bus)
        } else {
            0xFF
        }
    }

    fn write_40xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        if let Some(inner) = self.inner.as_mut() {
            inner.write_40xx(addr, data, bus);
        }
    }

    fn reset(&mut self) {
        if let Some(inner) = self.inner.as_mut() {
            inner.reset();
        }
    }
}

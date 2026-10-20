//! XM expansion module (passthrough + 128K RAM + POKEY + YM2151 + HSC).
//!
//! Phase 5 port of `a7800-5.2/src/devices/bus/a7800/xm.{h,cpp}`
//! (BSD-3-Clause). Control registers `cntrl1`–`cntrl5` (all zero at
//! reset) page two 8K RAM windows (`$4000–$5FFF` bank 0, `$6000–$7FFF`
//! bank 1) independently for Sally (`cntrl2` pages) and Maria (`cntrl3`
//! pages); `cntrl5` bit 0 switches to flat 48K RAM mode and bit 1
//! write-protects it. The onboard HSC ROM (`$3000–$3FFF`, gated on
//! `cntrl1` bit 3) and NVRAM (`$1000–$17FF`, always mapped — FIXME
//! preserved from the C++) ride along; anything unclaimed in `40xx`
//! falls through to the top-slot cart (`inner`).
//!
//! POKEY@`$0450` and YM2151@`$0460–$0461` decode only when enabled by
//! `cntrl1` bits 4/7. POKEY/YM traffic goes through
//! [`CartBus`](super::CartBus); `pokey_bases()`/`ym_present()` reflect
//! live control state.

use super::{load_nvram, Cart, CartBus, SavedCartState};

/// XM control-register file (`m_cntrl1`–`m_cntrl5`).
#[derive(Clone, Debug, Default)]
pub struct XmControls {
    /// Bit 0 rof-lo, 1 rof-hi, 2 bios/top-slot, 3 HSC on, 4 POKEY on,
    /// 5 bank0 on, 6 bank1 on, 7 YM2151 on.
    pub cntrl1: u8,
    /// Sally RAM 8K page multiplexer (low/high nibble = page 0/1).
    pub cntrl2: u8,
    /// Maria RAM 8K page multiplexer.
    pub cntrl3: u8,
    /// PIA/flash/top-slot-lock (stored; unused by the handlers).
    pub cntrl4: u8,
    /// Bit 0 48K flat RAM, 1 RAM write-protect, 2 BIOS, 3 POKEY lock,
    /// 4 HSC lock, 5 PAL HSC.
    pub cntrl5: u8,
}

/// XM expansion module. `inner` is the top-slot passthrough cart.
pub struct Xm {
    pub ctrl: XmControls,
    /// 128K paged RAM (`call_load` allocates `0x20000`).
    pub ram: Vec<u8>,
    /// Onboard HSC ROM image (4K) + NVRAM (2K, `0xFF`-filled at load).
    pub rom: Vec<u8>,
    pub nvram: Vec<u8>,
    pub inner: Option<Box<dyn Cart>>,
}

impl Xm {
    pub fn new(ram: Vec<u8>, rom: Vec<u8>, nvram_image: Option<&[u8]>) -> Self {
        debug_assert_eq!(ram.len(), 0x20000);
        Xm {
            ctrl: XmControls::default(),
            ram,
            rom,
            nvram: load_nvram(nvram_image, 0x800),
            inner: None,
        }
    }

    fn bank_addr(&self, off: usize, page_nibble: u8, rof_bit: u8) -> usize {
        // `((off & 0x1FFF) + (page * 0x2000)) | ((cntrl1 & rof) << shift)`:
        // rof bit 0 → `| (b0 << 8)` (bit 8 = +0x100), bit 1 → `| (b1 << 7)`
        // (bit 8 of the 0x2000-masked... kept verbatim via u8 shifts:
        // `(cntrl1 & 1) << 8` can set bit 8; `(cntrl1 & 2) << 7` = bit 8).
        let rof = if rof_bit == 0 {
            ((self.ctrl.cntrl1 & 1) as usize) << 8
        } else {
            ((self.ctrl.cntrl1 & 2) as usize) << 7
        };
        ((off & 0x1FFF) + (page_nibble as usize) * 0x2000) | rof
    }
}

impl Cart for Xm {
    fn save_state(&self) -> SavedCartState {
        let mut s = SavedCartState::versioned();
        s.ctrl = vec![
            self.ctrl.cntrl1,
            self.ctrl.cntrl2,
            self.ctrl.cntrl3,
            self.ctrl.cntrl4,
            self.ctrl.cntrl5,
        ];
        s.ram = self.ram.clone();
        s.nvram = self.nvram.clone();
        s.inner = self.inner.as_ref().map(|c| Box::new(c.save_state()));
        s
    }
    fn load_state(&mut self, s: &SavedCartState) {
        if s.ctrl.len() == 5 {
            self.ctrl.cntrl1 = s.ctrl[0];
            self.ctrl.cntrl2 = s.ctrl[1];
            self.ctrl.cntrl3 = s.ctrl[2];
            self.ctrl.cntrl4 = s.ctrl[3];
            self.ctrl.cntrl5 = s.ctrl[4];
        }
        s.apply_ram(&mut self.ram);
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
    fn debug_regs(&self) -> Vec<(&'static str, i64)> {
        vec![
            ("cntrl1", self.ctrl.cntrl1 as i64),
            ("cntrl2", self.ctrl.cntrl2 as i64),
            ("cntrl3", self.ctrl.cntrl3 as i64),
            ("cntrl4", self.ctrl.cntrl4 as i64),
            ("cntrl5", self.ctrl.cntrl5 as i64),
        ]
    }
    fn read_40xx(&mut self, addr: u16, maria: bool, bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x4000) as usize;
        if self.ctrl.cntrl5 & 1 == 0 {
            if self.ctrl.cntrl1 & 0x20 != 0 && off < 0x2000 {
                let page = if maria { self.ctrl.cntrl3 & 15 } else { self.ctrl.cntrl2 & 15 };
                let a = self.bank_addr(off, page, 0);
                self.ram[a]
            } else if self.ctrl.cntrl1 & 0x40 != 0 && (0x2000..0x4000).contains(&off) {
                let pages = if maria { self.ctrl.cntrl3 } else { self.ctrl.cntrl2 };
                let a = self.bank_addr(off, (pages >> 4) & 15, 1);
                self.ram[a]
            } else if let Some(inner) = self.inner.as_mut() {
                inner.read_40xx(addr, maria, bus)
            } else {
                0xFF
            }
        } else {
            self.ram[off]
        }
    }

    fn write_40xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        // Writes are dropped entirely while WE# is disabled (cntrl5 bit 1).
        if self.ctrl.cntrl5 & 2 != 0 {
            return;
        }
        let off = addr.wrapping_sub(0x4000) as usize;
        if self.ctrl.cntrl5 & 1 == 0 {
            if self.ctrl.cntrl1 & 0x20 != 0 && off < 0x2000 {
                // NOTE: C++ drops the rof bit on Sally writes (no `| rof`
                // term, unlike reads); kept verbatim.
                self.ram[(off & 0x1FFF) + ((self.ctrl.cntrl2 & 15) as usize) * 0x2000] = data;
            } else if self.ctrl.cntrl1 & 0x40 != 0 && (0x2000..0x4000).contains(&off) {
                self.ram[(off & 0x1FFF) + (((self.ctrl.cntrl2 >> 4) & 15) as usize) * 0x2000] = data;
            } else if let Some(inner) = self.inner.as_mut() {
                inner.write_40xx(addr, data, bus);
            }
        } else {
            self.ram[off] = data;
        }
    }

    fn read_10xx(&mut self, addr: u16, _bus: &mut dyn CartBus) -> u8 {
        self.nvram[addr.wrapping_sub(0x1000) as usize]
    }

    fn write_10xx(&mut self, addr: u16, data: u8, _bus: &mut dyn CartBus) {
        self.nvram[addr.wrapping_sub(0x1000) as usize] = data;
    }

    fn read_30xx(&mut self, addr: u16, _maria: bool, _bus: &mut dyn CartBus) -> u8 {
        if self.ctrl.cntrl1 & 0x08 != 0 {
            self.rom[addr.wrapping_sub(0x3000) as usize]
        } else {
            0xFF
        }
    }

    fn read_04xx(&mut self, addr: u16, bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x0400);
        if self.ctrl.cntrl1 & 0x10 != 0 && (0x50..0x60).contains(&off) {
            bus.pokey_read(0x0450, (off & 0x0F) as u8)
        } else if self.ctrl.cntrl1 & 0x80 != 0 && (0x60..=0x61).contains(&off) {
            bus.ym_read((off & 1) as u8)
        } else {
            0xFF
        }
    }

    fn write_04xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        let off = addr.wrapping_sub(0x0400);
        if self.ctrl.cntrl1 & 0x10 != 0 && (0x50..0x60).contains(&off) {
            bus.pokey_write(0x0450, (off & 0x0F) as u8, data);
        } else if self.ctrl.cntrl1 & 0x80 != 0 && (0x60..=0x61).contains(&off) {
            bus.ym_write((off & 1) as u8, data);
        } else if off == 0x70 {
            self.ctrl.cntrl1 = data;
        } else if off == 0x78 {
            self.ctrl.cntrl2 = data;
        } else if off == 0x7C {
            self.ctrl.cntrl3 = data;
        } else if off == 0x71 {
            self.ctrl.cntrl4 = data;
        } else if off == 0x72 {
            self.ctrl.cntrl5 = data;
        }
        // else do nothing
    }

    fn pokey_bases(&self) -> Vec<u16> {
        if self.ctrl.cntrl1 & 0x10 != 0 {
            vec![0x0450]
        } else {
            Vec::new()
        }
    }

    fn ym_present(&self) -> bool {
        self.ctrl.cntrl1 & 0x80 != 0
    }

    fn reset(&mut self) {
        self.ctrl = XmControls::default();
    }
}

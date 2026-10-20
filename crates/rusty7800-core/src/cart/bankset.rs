//! Bankset homebrew board (Fred Quimby / batari).
//!
//! Phase 5 port of `a7800-5.2/src/devices/bus/a7800/bankset.{h,cpp}`
//! (BSD-3-Clause). A bankset is 2×128K: Sally sees one SuperGame-style
//! image and Maria sees another; the `maria` read parameter selects the
//! view (replacing the C++ `m_dmaactive` global — TODO there to excise
//! it; here it is already a parameter). Banked-RAM variants give Sally
//! one 16K RAM bank at `$4000` and Maria another; Sally writes Maria's
//! bank via `$C000–$FFFF` (`write_40xx` with `offset >= 0x8000`).
//!
//! POKEY@`$0800` / POKEY@`$4000` combinations expose their windows; the
//! 52K variants add the `30xx` (`$3000–$3FFF`) ROM window installed by
//! `machine_start`.

use super::{bank_mask, Cart, CartBus};

/// Shared bankset image state.
#[derive(Clone, Debug)]
pub struct BanksetImage {
    pub rom: Vec<u8>,
    pub ram: Vec<u8>,
    pub bank: i32,
    pub mask: i32,
}

impl BanksetImage {
    pub fn new(rom: Vec<u8>, ram: Vec<u8>) -> Self {
        BanksetImage { mask: bank_mask(rom.len()), rom, ram, bank: 0 }
    }

    pub fn reset(&mut self) {
        self.bank = 0;
    }

    /// Second 128K half offset for the Maria view (`((mask/2)+1)*0x4000`).
    pub fn maria_half(&self) -> usize {
        (((self.mask / 2) + 1) as usize) * 0x4000
    }

    /// Bank register write (`$8000–$BFFF`): up to 2×256K banksets.
    pub fn select_bank(&mut self, data: u8) {
        self.bank = (data & 0x0F) as i32 & (self.mask / 2);
    }
}

/// Flat bankset ROM (`a78_bankset`, `a78_bankset_p800`,
/// `a78_bankset_p4000`): each CPU sees a half-ROM window ending at
/// `$C000` (`addrstart = 0xC000 - rom_size/2`); below it is open bus.
/// The P800/P4000 flavors add POKEY windows (handled by composition in
/// `p800.rs` / the `write_40xx` overrides below).
#[derive(Clone, Debug)]
pub struct BanksetRom {
    pub img: BanksetImage,
}

impl BanksetRom {
    pub fn new(rom: Vec<u8>) -> Self {
        BanksetRom { img: BanksetImage::new(rom, Vec::new()) }
    }

    fn window(&self) -> u32 {
        0xC000 - self.img.rom.len() as u32 / 2
    }
}

impl Cart for BanksetRom {
    fn read_40xx(&mut self, addr: u16, maria: bool, _bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x4000) as u32;
        let start = self.window();
        if off < start {
            0xFF
        } else if maria {
            let i = (off - start) as usize + self.img.rom.len() / 2;
            self.img.rom[i]
        } else {
            self.img.rom[(off - start) as usize]
        }
    }
    fn reset(&mut self) {
        self.img.reset();
    }
}

/// Flat bankset + POKEY@`$4000` writes (`a78_bankset_p4000`): reads as
/// `BanksetRom`; writes with `offset` in `0..0x10` hit the POKEY.
#[derive(Clone, Debug)]
pub struct BanksetRomPok4000 {
    pub inner: BanksetRom,
}

impl BanksetRomPok4000 {
    pub fn new(rom: Vec<u8>) -> Self {
        BanksetRomPok4000 { inner: BanksetRom::new(rom) }
    }
}

impl Cart for BanksetRomPok4000 {
    fn read_40xx(&mut self, addr: u16, maria: bool, bus: &mut dyn CartBus) -> u8 {
        self.inner.read_40xx(addr, maria, bus)
    }
    fn write_40xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        // C++ tests `offset >= 0 && offset < 0x10` on an unsigned offset,
        // i.e. effectively `offset < 0x10`; kept verbatim.
        let off = addr.wrapping_sub(0x4000);
        if off < 0x10 {
            bus.pokey_write(0x4000, (off & 0x0F) as u8, data);
        }
    }
    fn pokey_bases(&self) -> Vec<u16> {
        vec![0x4000]
    }
    fn reset(&mut self) {
        self.inner.reset();
    }
}

/// Bankset SuperGame, no RAM (`a78_bankset_sg`, `a78_bankset_sg_p800`):
/// second-to-last bank of the viewer half at `$4000–$7FFF` (the P800
/// flavor returns `0xFF` there — "can we do better?"), selectable bank
/// at `$8000–$BFFF`, last bank of the half at `$C000–$FFFF`.
#[derive(Clone, Debug)]
pub struct BanksetSg {
    pub img: BanksetImage,
    /// P800 flavor: `$4000–$7FFF` reads open bus instead of ROM.
    pub low_open: bool,
}

impl BanksetSg {
    pub fn new(rom: Vec<u8>, low_open: bool) -> Self {
        BanksetSg { img: BanksetImage::new(rom, Vec::new()), low_open }
    }
}

impl Cart for BanksetSg {
    fn read_40xx(&mut self, addr: u16, maria: bool, _bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x4000) as usize;
        let half = if maria { self.img.maria_half() } else { 0 };
        let last = ((self.img.mask / 2) as usize) * 0x4000 + half;
        if off < 0x4000 {
            if self.low_open {
                0xFF
            } else {
                // Second-to-last bank of the viewer half. C++ spells it
                // `offset + (mask/2)*0x4000 - 0x4000` (= `offset + last -
                // 0x4000`); the subtraction is exact (last >= 0x4000
                // whenever this window is reachable... for tiny ROMs the
                // usize math would underflow, matching nothing sensible
                // upstream either — banksets are >= 64K in practice).
                self.img.rom[off + last - 0x4000]
            }
        } else if off < 0x8000 {
            self.img.rom[(off & 0x3FFF) + (self.img.bank as usize) * 0x4000 + half]
        } else {
            self.img.rom[(off & 0x3FFF) + last]
        }
    }
    fn write_40xx(&mut self, addr: u16, data: u8, _bus: &mut dyn CartBus) {
        let off = addr.wrapping_sub(0x4000);
        if (0x4000..0x8000).contains(&off) {
            self.img.select_bank(data);
        }
    }
    fn reset(&mut self) {
        self.img.reset();
    }
}

/// Bankset SuperGame + banked RAM (`a78_bankset_sg_bankram`,
/// `a78_bankset_sg_bankram_p800`): Sally's 16K RAM at `$4000–$7FFF`,
/// Maria's at the same addresses during DMA; Sally reaches Maria's via
/// `$C000–$FFFF` writes.
#[derive(Clone, Debug)]
pub struct BanksetSgBankram {
    pub img: BanksetImage,
}

impl BanksetSgBankram {
    pub fn new(rom: Vec<u8>, ram: Vec<u8>) -> Self {
        debug_assert_eq!(ram.len(), 0x8000);
        BanksetSgBankram { img: BanksetImage::new(rom, ram) }
    }
}

impl Cart for BanksetSgBankram {
    fn read_40xx(&mut self, addr: u16, maria: bool, _bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x4000) as usize;
        let half = if maria { self.img.maria_half() } else { 0 };
        if off < 0x4000 {
            // Maria can only read, so the DMA flag picks Sally's 16K RAM
            // bank (`m_ram[offset]`) vs Maria's (`m_ram[offset + 0x4000]`).
            // (The ROM-half stride does NOT apply to RAM: fixed 0x4000.)
            let ram_half = if maria { 0x4000 } else { 0 };
            self.img.ram[off + ram_half]
        } else if off < 0x8000 {
            self.img.rom[(off & 0x3FFF) + (self.img.bank as usize) * 0x4000 + half]
        } else {
            let last = ((self.img.mask / 2) as usize) * 0x4000 + half;
            self.img.rom[(off & 0x3FFF) + last]
        }
    }
    fn write_40xx(&mut self, addr: u16, data: u8, _bus: &mut dyn CartBus) {
        let off = addr.wrapping_sub(0x4000) as usize;
        if off < 0x4000 {
            self.img.ram[off] = data;
        } else if off < 0x8000 {
            self.img.select_bank(data);
        } else {
            self.img.ram[off - 0x8000 + 0x4000] = data;
        }
    }
    fn reset(&mut self) {
        self.img.reset();
    }
}

/// Flat bankset + banked RAM (`a78_bankset_bankram`,
/// `a78_bankset_bankram_p800`): flat half-ROM window plus the same
/// Sally/Maria RAM split as `BanksetSgBankram`.
#[derive(Clone, Debug)]
pub struct BanksetBankram {
    pub img: BanksetImage,
}

impl BanksetBankram {
    pub fn new(rom: Vec<u8>, ram: Vec<u8>) -> Self {
        debug_assert_eq!(ram.len(), 0x8000);
        BanksetBankram { img: BanksetImage::new(rom, ram) }
    }

    fn window(&self) -> u32 {
        0xC000 - self.img.rom.len() as u32 / 2
    }
}

impl Cart for BanksetBankram {
    fn read_40xx(&mut self, addr: u16, maria: bool, _bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x4000) as usize;
        if off < 0x4000 {
            let ram_half = if maria { 0x4000 } else { 0 };
            self.img.ram[off + ram_half]
        } else {
            let start = self.window() as usize;
            if off < start {
                0xFF
            } else if maria {
                self.img.rom[off - start + self.img.rom.len() / 2]
            } else {
                self.img.rom[off - start]
            }
        }
    }
    fn write_40xx(&mut self, addr: u16, data: u8, _bus: &mut dyn CartBus) {
        let off = addr.wrapping_sub(0x4000) as usize;
        if off < 0x4000 {
            self.img.ram[off] = data;
        } else if off >= 0x8000 {
            self.img.ram[off - 0x8000 + 0x4000] = data;
        }
    }
    fn reset(&mut self) {
        self.img.reset();
    }
}

/// 52K bankset (`a78_bankset_52k`, `a78_bankset_52k_p4000`): the `40xx`
/// window reads `rom[offset + 0x1000]` (Maria: `+ 0xD000` more) and the
/// `30xx` window reads `rom[offset]` (Maria: `+ 0xD000`). The P4000
/// flavor adds POKEY@`$4000` writes for `offset < 0x4000`.
#[derive(Clone, Debug)]
pub struct Bankset52k {
    pub rom: Vec<u8>,
    pub pokey4000: bool,
}

impl Bankset52k {
    pub fn new(rom: Vec<u8>, pokey4000: bool) -> Self {
        Bankset52k { rom, pokey4000 }
    }
}

impl Cart for Bankset52k {
    fn read_40xx(&mut self, addr: u16, maria: bool, _bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x4000) as usize;
        if maria {
            self.rom[off + 0x1000 + 0xD000]
        } else {
            self.rom[off + 0x1000]
        }
    }
    fn read_30xx(&mut self, addr: u16, maria: bool, _bus: &mut dyn CartBus) -> u8 {
        let off = addr.wrapping_sub(0x3000) as usize;
        if maria {
            self.rom[off + 0xD000]
        } else {
            self.rom[off]
        }
    }
    fn write_40xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        if self.pokey4000 {
            let off = addr.wrapping_sub(0x4000);
            if off < 0x4000 {
                bus.pokey_write(0x4000, (off & 0x0F) as u8, data);
            }
        }
    }
    fn pokey_bases(&self) -> Vec<u16> {
        if self.pokey4000 {
            vec![0x4000]
        } else {
            Vec::new()
        }
    }
    fn reset(&mut self) {}
}

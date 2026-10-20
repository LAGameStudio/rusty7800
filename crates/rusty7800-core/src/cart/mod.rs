//! Rusty7800 `cart` — cartridge / slot bus.
//!
//! Phase 5 port of `a7800-5.2/src/devices/bus/a7800/*` (BSD-3-Clause):
//! `a78_slot.*` (slot windows, header dispatch, overlay installer),
//! `rom.*` (flat/SG/SG9/MRAM/ABS/ACT + P450/P800 POKEY variants),
//! `bankset.*` (Sally/Maria split views), `xm.*` (control registers),
//! `hiscore.*` (NVRAM + passthrough), `cpuwiz.*` (Versa/Mega boards).
//!
//! Address windows (from the file header in `a78_slot.cpp`): `04xx`
//! = `$0400–$047F`, `08xx` = `$0800–$0FFF`, `10xx` = `$1000–$17FF`,
//! `30xx` = `$3000–$3FFF`, `40xx` = `$4000–$FFFF`. All methods take full
//! CPU addresses; each implementation subtracts its window base to obtain
//! the MAME-style `offset` used in the C++ formulas.
//!
//! The C++ `m_dmaactive` global (Sally vs Maria view) is an explicit
//! `maria: bool` parameter on the banked `40xx`/`30xx` reads. POKEY/YM
//! sound chips are owned by the machine (audio phases); carts report
//! their chips via [`Cart::pokey_bases`] / [`Cart::ym_present`] and route
//! register traffic through [`CartBus`].

/// Register traffic sink for cart POKEY/YM chips (machine implements this
/// with the real audio devices; tests use a recorder).
pub trait CartBus {
    fn pokey_read(&mut self, base: u16, reg: u8) -> u8;
    fn pokey_write(&mut self, base: u16, reg: u8, data: u8);
    fn ym_read(&mut self, reg: u8) -> u8;
    fn ym_write(&mut self, reg: u8, data: u8);
}

/// Null bus: POKEY/YM reads return `0xFF`, writes are ignored.
#[derive(Clone, Debug, Default)]
pub struct NullBus;

impl CartBus for NullBus {
    fn pokey_read(&mut self, _base: u16, _reg: u8) -> u8 {
        0xFF
    }
    fn pokey_write(&mut self, _base: u16, _reg: u8, _data: u8) {}
    fn ym_read(&mut self, _reg: u8) -> u8 {
        0xFF
    }
    fn ym_write(&mut self, _reg: u8, _data: u8) {}
}

/// Cartridge interface (mirrors `device_a78_cart_interface` + the five
/// slot windows). Bank state lives in the implementor; `reset()` returns
/// to power-on state (all `m_bank`/`m_ram_bank`/XM controls zeroed).
pub trait Cart {
    fn read_04xx(&mut self, addr: u16, bus: &mut dyn CartBus) -> u8 {
        let _ = (addr, bus);
        0xFF
    }
    fn write_04xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        let _ = (addr, data, bus);
    }
    fn read_08xx(&mut self, addr: u16, bus: &mut dyn CartBus) -> u8 {
        let _ = (addr, bus);
        0xFF
    }
    fn write_08xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        let _ = (addr, data, bus);
    }
    fn read_10xx(&mut self, addr: u16, bus: &mut dyn CartBus) -> u8 {
        let _ = (addr, bus);
        0xFF
    }
    fn write_10xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        let _ = (addr, data, bus);
    }
    fn read_30xx(&mut self, addr: u16, maria: bool, bus: &mut dyn CartBus) -> u8 {
        let _ = (addr, maria, bus);
        0xFF
    }
    fn write_30xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        let _ = (addr, data, bus);
    }
    fn read_40xx(&mut self, addr: u16, maria: bool, bus: &mut dyn CartBus) -> u8;
    fn write_40xx(&mut self, addr: u16, data: u8, bus: &mut dyn CartBus) {
        let _ = (addr, data, bus);
    }
    /// POKEY base addresses this cart decodes (e.g. `[0x4000]`,
    /// `[0x0450]`, `[0x0800]`). XM gates its POKEY on `cntrl1` bit 4 at
    /// runtime, so its answer reflects live control state.
    fn pokey_bases(&self) -> Vec<u16> {
        Vec::new()
    }
    /// Whether the XM YM2151 window (`$0460–$0461`) is currently decoded.
    fn ym_present(&self) -> bool {
        false
    }
    fn reset(&mut self);
    /// Mutable state for save-states (Phase 8). ROM bytes are immutable
    /// and travel with the loaded image; only bank registers, RAM,
    /// NVRAM, and control registers are captured here. The machine checks
    /// `slot_key` equality before `load_state`, so a blob always meets
    /// its own cart type; length-mismatched fields are left untouched.
    fn save_state(&self) -> SavedCartState {
        SavedCartState::versioned()
    }
    fn load_state(&mut self, s: &SavedCartState) {
        let _ = s;
    }
    /// Live battery contents for NVRAM persistence (`state.nvram_image`);
    /// `None` = no battery on this cart.
    fn nvram_image(&self) -> Option<Vec<u8>> {
        None
    }
    /// Restore battery contents (truncates/pads like `load_nvram`).
    fn load_nvram(&mut self, image: &[u8]) {
        let _ = image;
    }
    /// Bank/control registers for `state.cart` introspection.
    fn debug_regs(&self) -> Vec<(&'static str, i64)> {
        Vec::new()
    }
}

/// Save-state version tag (bump when the layout changes).
pub const CART_STATE_VERSION: u32 = 1;

/// Mutable cart state blob. `regs[0]` = primary bank (`m_bank`),
/// `regs[1]` = secondary bank (`m_ram_bank`); `ctrl` carries small
/// control-register files (XM `cntrl1`–`cntrl5`); `inner` recurses for
/// passthrough wrappers (HSC/XM top-slot, POKEY mixins).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SavedCartState {
    pub version: u32,
    pub regs: [i32; 2],
    pub ram: Vec<u8>,
    pub nvram: Vec<u8>,
    pub ctrl: Vec<u8>,
    pub inner: Option<Box<SavedCartState>>,
}

impl SavedCartState {
    pub fn versioned() -> Self {
        SavedCartState { version: CART_STATE_VERSION, ..Default::default() }
    }

    pub fn with_bank(bank: i32) -> Self {
        let mut s = Self::versioned();
        s.regs[0] = bank;
        s
    }

    pub fn with_bank_ram(bank: i32, ram_bank: i32, ram: &[u8]) -> Self {
        let mut s = Self::versioned();
        s.regs = [bank, ram_bank];
        s.ram = ram.to_vec();
        s
    }

    /// Restore helpers: registers apply unconditionally; bulk fields only
    /// when lengths match the live image (guards corrupt snapshots).
    pub fn apply_bank(&self, bank: &mut i32) {
        *bank = self.regs[0];
    }

    pub fn apply_ram(&self, ram: &mut [u8]) {
        if self.ram.len() == ram.len() {
            ram.copy_from_slice(&self.ram);
        }
    }

    pub fn apply_nvram(&self, nvram: &mut [u8]) {
        if self.nvram.len() == nvram.len() {
            nvram.copy_from_slice(&self.nvram);
        }
    }
}

/// Raw PCB ids from `a78_slot.h` (kept numeric: header dispatch does
/// `m_type += A78_POKEY0450` arithmetic, including the 0x60 Versa quirk).
pub mod pcb {
    pub const TYPE0: i32 = 0;
    pub const TYPE1: i32 = 1;
    pub const TYPE2: i32 = 2;
    pub const TYPE3: i32 = 3;
    pub const TYPE6: i32 = 4;
    pub const TYPEA: i32 = 5;
    pub const TYPE8: i32 = 6;
    pub const ABSOLUTE: i32 = 7;
    pub const ACTIVISION: i32 = 8;
    pub const HSC: i32 = 9;
    pub const XB_BOARD: i32 = 10;
    pub const XM_BOARD: i32 = 11;
    pub const BANKSET: i32 = 12;
    pub const BANKSET_POK800: i32 = 13;
    pub const BANKSET_POK4000: i32 = 14;
    pub const BANKSET_SG: i32 = 15;
    pub const BANKSET_SG_POK800: i32 = 16;
    pub const BANKSET_SG_BANKRAM: i32 = 17;
    pub const BANKSET_SG_BANKRAM_POK800: i32 = 18;
    pub const BANKSET_BANKRAM: i32 = 19;
    pub const BANKSET_BANKRAM_POK800: i32 = 20;
    pub const BANKSET_52K: i32 = 21;
    pub const BANKSET_52K_POK4000: i32 = 22;
    pub const MEGACART: i32 = 23;
    pub const VERSABOARD: i32 = 0x20;
    pub const TYPE0_POK450: i32 = 0x40;
    pub const TYPE1_POK450: i32 = 0x41;
    pub const TYPE2_POK450: i32 = 0x42;
    pub const TYPE6_POK450: i32 = 0x44;
    pub const TYPEA_POK450: i32 = 0x45;
    pub const VERSA_POK450: i32 = 0x50;
    pub const TYPE0_POK800: i32 = 0x80;
    pub const TYPE1_POK800: i32 = 0x81;
    pub const TYPE2_POK800: i32 = 0x82;
    pub const TYPE6_POK800: i32 = 0x84;
    pub const TYPEA_POK800: i32 = 0x85;
    pub const POKEY0450: i32 = 0x40;
}

/// Slot option key for a raw PCB id (mirrors `a78_get_slot`: unknown ids
/// fall back to `"a78_rom"` — this is what a Versa image with the POKEY
/// bit set resolves to, since `0x20 + 0x40 = 0x60` hits no entry).
pub fn slot_key_for_type(ty: i32) -> &'static str {
    match ty {
        pcb::TYPE0 => "a78_rom",
        pcb::TYPE1 => "a78_pokey",
        pcb::TYPE2 => "a78_sg",
        pcb::TYPE3 => "a78_sg_pokey",
        pcb::TYPE6 => "a78_sg_ram",
        pcb::TYPEA => "a78_sg9",
        pcb::TYPE8 => "a78_mram",
        pcb::ABSOLUTE => "a78_abs",
        pcb::ACTIVISION => "a78_act",
        pcb::HSC => "a78_hsc",
        pcb::XM_BOARD => "a78_xm",
        pcb::BANKSET => "a78_bankset",
        pcb::BANKSET_POK4000 => "a78_bankset_p4000",
        pcb::BANKSET_BANKRAM => "a78_bankset_bankram",
        pcb::BANKSET_POK800 => "a78_bankset_p800",
        pcb::BANKSET_BANKRAM_POK800 => "a78_bankset_bankram_p800",
        pcb::BANKSET_SG => "a78_bankset_sg",
        pcb::BANKSET_SG_POK800 => "a78_bankset_sg_p800",
        pcb::BANKSET_SG_BANKRAM => "a78_bankset_sg_bankram",
        pcb::BANKSET_SG_BANKRAM_POK800 => "a78_bankset_sg_bankram_p800",
        pcb::BANKSET_52K => "a78_bankset_52k",
        pcb::BANKSET_52K_POK4000 => "a78_bankset_52k_p4000",
        pcb::MEGACART => "a78_megacart",
        pcb::VERSABOARD => "a78_versa",
        pcb::TYPE0_POK450 => "a78_p450_t0",
        pcb::TYPE1_POK450 => "a78_p450_t1",
        pcb::TYPE2_POK450 => "a78_p450_t2",
        pcb::TYPE6_POK450 => "a78_p450_t6",
        pcb::TYPEA_POK450 => "a78_p450_ta",
        pcb::VERSA_POK450 => "a78_p450_vb",
        pcb::TYPE0_POK800 => "a78_p800_t0",
        pcb::TYPE1_POK800 => "a78_p800_t1",
        pcb::TYPE2_POK800 => "a78_p800_t2",
        pcb::TYPE6_POK800 => "a78_p800_t6",
        pcb::TYPEA_POK800 => "a78_p800_ta",
        _ => "a78_rom",
    }
}

/// `rom_alloc` helpers: bank mask (with the SG9 non-power-of-2
/// compensation) and the top-of-memory base for flat images.
pub fn bank_mask(rom_size: usize) -> i32 {
    let banks = rom_size as i32 / 0x4000;
    if banks & 1 != 0 {
        banks - 2
    } else {
        banks - 1
    }
}

/// Start of image data in CPU space for flat carts (`0x10000 - size`;
/// wraps like the C++ unsigned arithmetic for images over 64K — only
/// flat carts (<= 48K) actually consult it).
pub fn base_rom(rom_size: usize) -> u32 {
    0x10000u32.wrapping_sub(rom_size as u32)
}

/// Dynamic address-map overlay installed by `machine_start` per cart type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overlay {
    /// First CPU address covered.
    pub start: u16,
    /// Last CPU address covered.
    pub end: u16,
    /// Which slot window serves it: `0x04`, `0x08`, `0x10`, or `0x30`.
    pub window: u8,
    /// Whether CPU writes reach the cart here.
    pub writable: bool,
}

/// Overlay set for a raw PCB id (mirrors the `machine_start` switch,
/// including the `A78_BANKSET_52K_POK4000` fallthrough that installs both
/// the `30xx` read handler and the `40xx` (`0x4000–0x7FFF`) write path).
pub fn overlays_for_type(ty: i32) -> Vec<Overlay> {
    match ty {
        pcb::HSC => vec![
            Overlay { start: 0x1000, end: 0x17FF, window: 0x10, writable: true },
            Overlay { start: 0x3000, end: 0x3FFF, window: 0x30, writable: true },
        ],
        pcb::BANKSET_52K => {
            vec![Overlay { start: 0x3000, end: 0x3FFF, window: 0x30, writable: false }]
        }
        pcb::BANKSET_52K_POK4000 => vec![
            Overlay { start: 0x3000, end: 0x3FFF, window: 0x30, writable: false },
            Overlay { start: 0x4000, end: 0x7FFF, window: 0x40, writable: true },
        ],
        pcb::BANKSET_POK4000 => {
            vec![Overlay { start: 0x4000, end: 0x7FFF, window: 0x40, writable: true }]
        }
        pcb::TYPE0_POK450 | pcb::TYPE1_POK450 | pcb::TYPE2_POK450 | pcb::TYPE6_POK450
        | pcb::TYPEA_POK450 | pcb::VERSA_POK450 => {
            vec![Overlay { start: 0x0400, end: 0x04FF, window: 0x04, writable: true }]
        }
        pcb::TYPE0_POK800 | pcb::TYPE1_POK800 | pcb::TYPE2_POK800 | pcb::TYPE6_POK800
        | pcb::TYPEA_POK800 | pcb::BANKSET_POK800 | pcb::BANKSET_SG_POK800
        | pcb::BANKSET_SG_BANKRAM_POK800 | pcb::BANKSET_BANKRAM_POK800 => {
            vec![Overlay { start: 0x0800, end: 0x0FFF, window: 0x08, writable: true }]
        }
        pcb::XM_BOARD => vec![
            Overlay { start: 0x0400, end: 0x047F, window: 0x04, writable: true },
            Overlay { start: 0x1000, end: 0x17FF, window: 0x10, writable: true },
            Overlay { start: 0x3000, end: 0x3FFF, window: 0x30, writable: true },
        ],
        _ => Vec::new(),
    }
}

/// NVRAM battery semantics: a missing image powers on to `0xFF`
/// (`battery_load(..., 0xff)`); overlong images truncate, short ones pad.
pub fn load_nvram(existing: Option<&[u8]>, size: usize) -> Vec<u8> {
    let mut nv = vec![0xFF; size];
    if let Some(img) = existing {
        let n = img.len().min(size);
        nv[..n].copy_from_slice(&img[..n]);
    }
    nv
}

/// NVRAM persistence image (`battery_save` always writes `0x800` bytes for
/// HSC/XM; here: the live contents verbatim).
pub fn save_nvram(nvram: &[u8]) -> Vec<u8> {
    nvram.to_vec()
}

pub mod header;
pub mod rom;
pub mod bankset;
pub mod xm;
pub mod hiscore;
pub mod cpuwiz;
pub mod p450;
pub mod p800;

/// A loaded cartridge image: the built cart plus header metadata.
pub struct LoadedCart {
    pub cart: Box<dyn Cart>,
    /// Raw PCB id (`header::dispatch_type` result, pre-slot-key). `-1` for
    /// headerless images (assumed flat).
    pub pcb_id: i32,
    /// Slot key the cart was built as.
    pub slot_key: &'static str,
    /// Parsed header (`None` for headerless images).
    pub header: Option<header::A78Header>,
    /// Quirk repairs applied to the header mapper.
    pub quirks: Vec<header::Quirk>,
    /// Non-fatal loader notes (SaveKey, XM-required, length clamp...).
    pub notes: Vec<&'static str>,
}

/// Build a cart for a slot key with correctly-sized work RAM.
/// `rom` is the image data (without the 128-byte header); `nvram_image`
/// is optional battery history. Unknown keys fall back to flat.
pub fn build_cart(slot_key: &str, rom: Vec<u8>, nvram_image: Option<&[u8]>) -> Box<dyn Cart> {
    use bankset::{Bankset52k, BanksetBankram, BanksetRom, BanksetRomPok4000, BanksetSg, BanksetSgBankram};
    use cpuwiz::{Mega, Versa};
    use hiscore::Hiscore;
    use p450::{Pokey450, VersaPokey};
    use p800::Pokey800;
    use rom::{Absolute, Activision, Flat, FlatPokey, MirrorRam, SuperGame, SuperGame9, SuperGamePokey, SuperGameRam};
    use xm::Xm;
    let ram = |n: usize| vec![0u8; n];
    match slot_key {
        "a78_pokey" => Box::new(FlatPokey::new(rom)),
        "a78_sg" => Box::new(SuperGame::new(rom)),
        "a78_sg_pokey" => Box::new(SuperGamePokey::new(rom)),
        "a78_sg_ram" => Box::new(SuperGameRam::new(rom, ram(0x4000))),
        "a78_sg9" => Box::new(SuperGame9::new(rom)),
        "a78_mram" => Box::new(MirrorRam::new(rom, ram(0x4000))),
        "a78_abs" => Box::new(Absolute::new(rom)),
        "a78_act" => Box::new(Activision::new(rom)),
        "a78_hsc" => Box::new(Hiscore::new(pad_to(rom, 0x1000), nvram_image)),
        "a78_xm" => Box::new(Xm::new(ram(0x20000), vec![0u8; 0x1000], nvram_image)),
        "a78_bankset" => Box::new(BanksetRom::new(rom)),
        "a78_bankset_p4000" => Box::new(BanksetRomPok4000::new(rom)),
        "a78_bankset_bankram" => Box::new(BanksetBankram::new(rom, ram(0x8000))),
        "a78_bankset_p800" => Box::new(Pokey800::new(BanksetRom::new(rom))),
        "a78_bankset_bankram_p800" => {
            Box::new(Pokey800::new(BanksetBankram::new(rom, ram(0x8000))))
        }
        "a78_bankset_sg" => Box::new(BanksetSg::new(rom, false)),
        "a78_bankset_sg_p800" => Box::new(Pokey800::new(BanksetSg::new(rom, true))),
        "a78_bankset_sg_bankram" => Box::new(BanksetSgBankram::new(rom, ram(0x8000))),
        "a78_bankset_sg_bankram_p800" => {
            Box::new(Pokey800::new(BanksetSgBankram::new(rom, ram(0x8000))))
        }
        "a78_bankset_52k" => Box::new(Bankset52k::new(rom, false)),
        "a78_bankset_52k_p4000" => Box::new(Bankset52k::new(rom, true)),
        "a78_megacart" => Box::new(Mega::new(rom, ram(0x8000))),
        "a78_versa" => Box::new(Versa::new(rom, ram(0x8000))),
        "a78_p450_t0" => Box::new(Pokey450::new(Flat::new(rom))),
        "a78_p450_t1" => Box::new(Pokey450::new(FlatPokey::new(rom))),
        "a78_p450_t2" => Box::new(Pokey450::new(SuperGame::new(rom))),
        "a78_p450_t6" => Box::new(Pokey450::new(SuperGameRam::new(rom, ram(0x4000)))),
        "a78_p450_ta" => Box::new(Pokey450::new(SuperGame9::new(rom))),
        "a78_p450_vb" => Box::new(VersaPokey::new(rom, ram(0x8000))),
        "a78_p800_t0" => Box::new(Pokey800::new(Flat::new(rom))),
        "a78_p800_t1" => Box::new(Pokey800::new(FlatPokey::new(rom))),
        "a78_p800_t2" => Box::new(Pokey800::new(SuperGame::new(rom))),
        "a78_p800_t6" => Box::new(Pokey800::new(SuperGameRam::new(rom, ram(0x4000)))),
        "a78_p800_ta" => Box::new(Pokey800::new(SuperGame9::new(rom))),
        _ => Box::new(Flat::new(rom)),
    }
}

/// Banked-ROM indexing (`m_rom[...]` sites). The C++ uses signed `int`
/// math throughout the bank formulas, so small/degenerate images index
/// out of bounds there (undefined behavior — garbage or crashes). Here
/// the same signed formula is evaluated in `i64` (which cannot overflow
/// at these magnitudes) and wrapped within the image: every in-bounds
/// access returns exactly what the C++ reads; out-of-bounds accesses
/// mirror deterministically instead of panicking. Empty images read
/// `0xFF`. Explicit open-bus windows keep their `0xFF` semantics and do
/// NOT use this helper.
pub fn bidx(rom: &[u8], idx: i64) -> u8 {
    if rom.is_empty() {
        0xFF
    } else {
        rom[idx.rem_euclid(rom.len() as i64) as usize]
    }
}

/// Pad or truncate image data to an exact size (pads with `0xFF`, the
/// erased-EPROM state, matching `ROMREGION_ERASEFF`).
pub fn pad_to(mut data: Vec<u8>, size: usize) -> Vec<u8> {
    data.resize(size, 0xFF);
    data
}

/// Load a raw file dump. Headered `.a78` files go through magic check,
/// dispatch, quirk repair, length clamp, and `call_load` RAM/NVRAM sizing.
/// Headerless `.bin` files load whole as a flat cart. MAME rejects those,
/// but the plan requires headerless detection, so accepting them as flat
/// is a documented adaptation.
pub fn load_image(file: &[u8]) -> Result<LoadedCart, header::HeaderError> {
    if file.len() >= header::HEADER_LEN && file[1..10] == header::MAGIC[..] {
        let head = header::parse_header(file)?;
        let (mapper, quirks) = header::validate_header(head.mapper);
        let rom_len = header::effective_rom_len(head.len, file.len());
        let rom = file[header::HEADER_LEN..header::HEADER_LEN + rom_len].to_vec();
        let pcb_id = header::dispatch_type(mapper, rom_len, file.len());
        let slot_key = slot_key_for_type(pcb_id);
        let mut notes = Vec::new();
        if head.save == 2 {
            notes.push("SaveKey NVRAM requested (unsupported upstream)");
        }
        if head.needs_xm {
            notes.push("XM expansion required");
        }
        if rom_len != head.len as usize {
            notes.push("header length clamped to file");
        }
        let cart = build_cart(slot_key, rom, None);
        Ok(LoadedCart { cart, pcb_id, slot_key, header: Some(head), quirks, notes })
    } else {
        Ok(LoadedCart {
            cart: build_cart("a78_rom", file.to_vec(), None),
            pcb_id: -1,
            slot_key: "a78_rom",
            header: None,
            quirks: Vec::new(),
            notes: vec!["headerless image, assumed flat"],
        })
    }
}

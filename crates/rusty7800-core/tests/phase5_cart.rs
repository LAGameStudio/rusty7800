//! Phase 5 acceptance: header quirks, mapper dispatch, bank switching,
//! bankset Sally/Maria views, POKEY matrix, XM controls, HSC NVRAM.

use rusty7800_core::cart::bankset::{Bankset52k, BanksetBankram, BanksetRom, BanksetRomPok4000, BanksetSg, BanksetSgBankram};
use rusty7800_core::cart::cpuwiz::{Mega, Versa};
use rusty7800_core::cart::header::{
    self, dispatch_type, effective_rom_len, nvram_size_for_type, ram_size_for_type, validate_header,
    HeaderError, Quirk, HEADER_LEN,
};
use rusty7800_core::cart::hiscore::Hiscore;
use rusty7800_core::cart::p450::{Pokey450, VersaPokey};
use rusty7800_core::cart::p800::Pokey800;
use rusty7800_core::cart::pcb;
use rusty7800_core::cart::rom::{Absolute, Activision, Flat, FlatPokey, MirrorRam, SuperGame, SuperGame9, SuperGamePokey, SuperGameRam};
use rusty7800_core::cart::xm::Xm;
use rusty7800_core::cart::{bank_mask, load_nvram, overlays_for_type, save_nvram, slot_key_for_type, Cart, CartBus, NullBus};

/// ROM image with each 16K bank filled with its index.
fn marker_banks(nbanks: usize) -> Vec<u8> {
    let mut rom = vec![0u8; nbanks * 0x4000];
    for (i, chunk) in rom.chunks_mut(0x4000).enumerate() {
        chunk.fill(i as u8);
    }
    rom
}

#[derive(Default)]
struct RecordBus {
    log: Vec<(u16, u8, u8)>,
    ym_log: Vec<(u8, u8)>,
    read_val: u8,
}

impl CartBus for RecordBus {
    fn pokey_read(&mut self, base: u16, reg: u8) -> u8 {
        self.log.push((base, reg, 0xFF));
        self.read_val
    }
    fn pokey_write(&mut self, base: u16, reg: u8, data: u8) {
        self.log.push((base, reg, data));
    }
    fn ym_read(&mut self, reg: u8) -> u8 {
        self.ym_log.push((reg, 0xFF));
        self.read_val
    }
    fn ym_write(&mut self, reg: u8, data: u8) {
        self.ym_log.push((reg, data));
    }
}

fn synth_header(mapper: u16, len: u32) -> [u8; HEADER_LEN] {
    let mut head = [0u8; HEADER_LEN];
    head[1..10].copy_from_slice(b"ATARI7800");
    head[49..53].copy_from_slice(&len.to_be_bytes());
    head[53..55].copy_from_slice(&mapper.to_be_bytes());
    head[0x11..0x17].copy_from_slice(b"TEST  ");
    head
}

// ---- header parse / verify ---------------------------------------------

#[test]
fn header_parse_and_errors() {
    let h = header::parse_header(&synth_header(0x0002, 0x20000)).unwrap();
    assert_eq!((h.len, h.mapper), (0x20000, 0x0002));
    assert!(!h.is_pal && h.save == 0 && !h.needs_xm);
    assert_eq!(&h.title[..6], b"TEST  ");
    assert_eq!(header::parse_header(&[0u8; 10]), Err(HeaderError::TooShort));
    let mut bad = synth_header(0, 0);
    bad[1] = b'X';
    assert_eq!(header::parse_header(&bad), Err(HeaderError::BadMagic));
    assert_eq!(effective_rom_len(0x20000, 0x20000 + 128), 0x20000);
    assert_eq!(effective_rom_len(0x99999, 0x1000 + 128), 0x1000);
}

// ---- validate_header quirks --------------------------------------------

#[test]
fn quirk_arms_repair_one_bit_each() {
    // (input, cleared bit, expected output)
    for (inp, clear, out) in [
        (0x05u16, 0x01u16, 0x04u16),
        (0x09, 0x01, 0x08),
        (0x11, 0x01, 0x10),
        (0x21, 0x01, 0x20),
        (0x0C, 0x04, 0x08),
        (0x14, 0x04, 0x10),
        (0x24, 0x04, 0x20),
        (0x18, 0x08, 0x10),
        (0x28, 0x08, 0x20),
        (0x30, 0x10, 0x20),
    ] {
        let (fixed, quirks) = validate_header(inp);
        assert_eq!(fixed, out, "input {inp:#06X}");
        assert_eq!(quirks.len(), 1);
        assert!(matches!(quirks[0], Quirk::MaskRepair { cleared, .. } if cleared == clear));
    }
    // Untouched mappers (incl. the disabled C++ branches staying off).
    for inp in [0x0000u16, 0x0001, 0x0002, 0x0003, 0x0004, 0x0020, 0x0080, 0x0100, 0x0200, 0x2000] {
        let (fixed, quirks) = validate_header(inp);
        assert_eq!((fixed, quirks.len()), (inp, 0), "input {inp:#06X}");
    }
    // Special-bit masking (0x0102/0x0202 hit no mask arm, so the single
    // quirk is the special mask; 0x0105 hits the 0x05 arm first, then the
    // special mask, yielding two quirks and the same final value).
    assert_eq!(validate_header(0x0102).0, 0x0100);
    assert_eq!(validate_header(0x0202).0, 0x0200);
    assert!(matches!(
        validate_header(0x0102).1[0],
        Quirk::SpecialMasked { kept: 0x0100 }
    ));
    assert_eq!(validate_header(0x0105).0, 0x0100);
    assert_eq!(validate_header(0x0105).1.len(), 2);
}

// ---- dispatch matrix ----------------------------------------------------

#[test]
fn dispatch_type_matrix() {
    let d = |m: u16| dispatch_type(m, 0x8000, 0x8000 + 128);
    assert_eq!(d(0x0000), pcb::TYPE0);
    assert_eq!(d(0x0001), pcb::TYPE1);
    assert_eq!(d(0x0002), pcb::TYPE2);
    assert_eq!(d(0x0003), pcb::TYPE3);
    assert_eq!(d(0x0006), pcb::TYPE6);
    assert_eq!(d(0x000A), pcb::TYPEA);
    assert_eq!(d(0x0022), pcb::VERSABOARD);
    assert_eq!(d(0x0080), pcb::TYPE8);
    assert_eq!(d(0x0100), pcb::ACTIVISION);
    assert_eq!(d(0x0200), pcb::ABSOLUTE);
    assert_eq!(d(0x2000), pcb::BANKSET);
    assert_eq!(d(0x6000), pcb::BANKSET_BANKRAM);
    assert_eq!(d(0xE000), pcb::BANKSET_BANKRAM_POK800);
    assert_eq!(d(0x2002), pcb::BANKSET_SG);
    assert_eq!(d(0x6002), pcb::BANKSET_SG_BANKRAM);
    assert_eq!(d(0xA002), pcb::BANKSET_SG_POK800);
    assert_eq!(d(0xE002), pcb::BANKSET_SG_BANKRAM_POK800);
    assert_eq!(d(0x8000), pcb::TYPE0_POK800);
    assert_eq!(d(0x8001), pcb::TYPE1_POK800);
    assert_eq!(d(0x8002), pcb::TYPE2_POK800);
    assert_eq!(d(0x8006), pcb::TYPE6_POK800);
    assert_eq!(d(0x800A), pcb::TYPEA_POK800);
    // 52K threshold on ROM length.
    assert_eq!(dispatch_type(0x2001, 0x1A000, 0), pcb::BANKSET_52K_POK4000);
    assert_eq!(dispatch_type(0x2001, 0x10000, 0), pcb::BANKSET_POK4000);
    assert_eq!(dispatch_type(0x2000, 0x1A000, 0), pcb::BANKSET_52K);
    // POKEY@0450 addition (non-bankset only)...
    assert_eq!(d(0x0040), pcb::TYPE0_POK450);
    assert_eq!(d(0x0042), pcb::TYPE2_POK450);
    // ...but bankset mappers keep 0x2000 (no +0x40).
    assert_eq!(dispatch_type(0x2040, 0x8000, 0), pcb::BANKSET);
    // Versa + POKEY bit = 0x60: no slot entry (faithful quirk).
    let versa_pokey = dispatch_type(0x0062, 0x40000, 0x40000);
    assert_eq!(versa_pokey, 0x60);
    assert_eq!(slot_key_for_type(versa_pokey), "a78_rom");
    // Mega vs Versa on file size.
    assert_eq!(dispatch_type(0x0022, 0, 0x40001), pcb::MEGACART);
    assert_eq!(slot_key_for_type(pcb::MEGACART), "a78_megacart");
    assert_eq!(slot_key_for_type(pcb::VERSABOARD), "a78_versa");
    assert_eq!(slot_key_for_type(pcb::HSC), "a78_hsc");
    assert_eq!(slot_key_for_type(pcb::XM_BOARD), "a78_xm");
}

#[test]
fn ram_and_nvram_sizes() {
    assert_eq!(ram_size_for_type(pcb::TYPE6), 0x4000);
    assert_eq!(ram_size_for_type(pcb::TYPE8), 0x4000);
    assert_eq!(ram_size_for_type(pcb::TYPE6_POK800), 0x4000);
    assert_eq!(ram_size_for_type(pcb::VERSABOARD), 0x8000);
    assert_eq!(ram_size_for_type(pcb::MEGACART), 0x8000);
    assert_eq!(ram_size_for_type(pcb::BANKSET_SG_BANKRAM), 0x8000);
    assert_eq!(ram_size_for_type(pcb::TYPE0), 0);
    assert_eq!(ram_size_for_type(pcb::XM_BOARD), 0x20000);
    assert_eq!(nvram_size_for_type(pcb::HSC), 0x800);
    assert_eq!(nvram_size_for_type(pcb::XM_BOARD), 0x800);
    assert_eq!(nvram_size_for_type(pcb::TYPE0), 0);
    // SG9 non-power-of-2 mask compensation (9 banks -> mask 7).
    assert_eq!(bank_mask(9 * 0x4000), 7);
    assert_eq!(bank_mask(8 * 0x4000), 7);
    assert_eq!(bank_mask(16 * 0x4000), 15);
}

// ---- flat / SG / SG9 / MRAM / ABS / ACT --------------------------------

#[test]
fn flat_open_bus_below_base() {
    let mut c = Flat::new(marker_banks(2)); // 32K, base 0x8000
    let mut bus = NullBus;
    assert_eq!(c.read_40xx(0x4000, false, &mut bus), 0xFF);
    assert_eq!(c.read_40xx(0x8000, false, &mut bus), 0x00);
    assert_eq!(c.read_40xx(0xC000, false, &mut bus), 0x01);
}

#[test]
fn supergame_bank_switching() {
    let mut c = SuperGame::new(marker_banks(8));
    let mut bus = NullBus;
    assert_eq!(c.read_40xx(0x8000, false, &mut bus), 0x00); // bank 0
    assert_eq!(c.read_40xx(0xC000, false, &mut bus), 0x07); // last
    assert_eq!(c.read_40xx(0x4000, false, &mut bus), 0x06); // 2nd-to-last
    c.write_40xx(0x8000, 0x03, &mut bus);
    assert_eq!(c.read_40xx(0x8000, false, &mut bus), 0x03);
    assert_eq!(c.read_40xx(0xC000, false, &mut bus), 0x07); // fixed
    c.reset();
    assert_eq!(c.read_40xx(0x8000, false, &mut bus), 0x00);
}

#[test]
fn supergame9_nine_banks() {
    let mut c = SuperGame9::new(marker_banks(9));
    let mut bus = NullBus;
    assert_eq!(c.read_40xx(0x4000, false, &mut bus), 0x00); // bank 0 fixed low
    assert_eq!(c.read_40xx(0xC000, false, &mut bus), 0x08); // last fixed high
    c.write_40xx(0x8000, 0x02, &mut bus); // (2 & 7) + 1 = 3
    assert_eq!(c.read_40xx(0x8000, false, &mut bus), 0x03);
}

#[test]
fn supergame_ram_window() {
    let mut c = SuperGameRam::new(marker_banks(8), vec![0u8; 0x4000]);
    let mut bus = NullBus;
    c.write_40xx(0x4000, 0x5A, &mut bus);
    assert_eq!(c.read_40xx(0x4000, false, &mut bus), 0x5A);
    c.write_40xx(0x8000, 0x05, &mut bus);
    assert_eq!(c.read_40xx(0x8000, false, &mut bus), 0x05);
}

#[test]
fn mirror_ram_a8_ignored() {
    let mut c = MirrorRam::new(marker_banks(2), vec![0u8; 0x4000]);
    let mut bus = NullBus;
    c.write_40xx(0x4000, 0x77, &mut bus);
    // Odd-page mirror of the even page (offset & 0xFEFF).
    assert_eq!(c.read_40xx(0x4100, false, &mut bus), 0x77);
    assert_eq!(c.read_40xx(0xC000, false, &mut bus), 0x01); // ROM high
}

#[test]
fn absolute_bank_select() {
    let mut c = Absolute::new(marker_banks(4)); // 64K
    let mut bus = NullBus;
    assert_eq!(c.read_40xx(0x4000, false, &mut bus), 0x00);
    c.write_40xx(0x8000, 0x02, &mut bus);
    assert_eq!(c.read_40xx(0x4000, false, &mut bus), 0x01);
    c.write_40xx(0x8000, 0x01, &mut bus);
    assert_eq!(c.read_40xx(0x4000, false, &mut bus), 0x00);
    assert_eq!(c.read_40xx(0x8000, false, &mut bus), 0x02); // fixed upper
}

#[test]
fn activision_windows() {
    let mut c = Activision::new(marker_banks(8)); // 128K
    let mut bus = NullBus;
    assert_eq!(c.read_40xx(0x4000, false, &mut bus), 0x06); // bank6 hi@0x1A000
    assert_eq!(c.read_40xx(0x6000, false, &mut bus), 0x06); // bank6 lo@0x18000
    assert_eq!(c.read_40xx(0x8000, false, &mut bus), 0x07); // bank7 hi@0x1E000
    assert_eq!(c.read_40xx(0xE000, false, &mut bus), 0x07); // bank7 lo@0x1C000
    c.write_40xx(0xFF83, 0x00, &mut bus); // bank = 3
    assert_eq!(c.read_40xx(0xA000, false, &mut bus), 0x03);
    assert_eq!(c.read_40xx(0xC000, false, &mut bus), 0x03);
}

// ---- SG+POKEY visibility (rom.cpp:15 outcome pinned) --------------------

#[test]
fn sg_pokey_visibility() {
    let mut c = SuperGamePokey::new(marker_banks(8));
    let mut bus = RecordBus { read_val: 0x42, ..Default::default() };
    // $4000-$7FFF reads hit the POKEY (banked ROM unreadable there).
    assert_eq!(c.read_40xx(0x4000, false, &mut bus), 0x42);
    assert_eq!(bus.log, vec![(0x4000, 0x00, 0xFF)]);
    c.write_40xx(0x4103, 0x99, &mut bus);
    assert_eq!(bus.log.last(), Some(&(0x4000, 0x03, 0x99)));
    // Banked/high windows still ROM.
    assert_eq!(c.read_40xx(0x8000, false, &mut bus), 0x00);
    assert_eq!(c.pokey_bases(), vec![0x4000]);
    let mut f = FlatPokey::new(marker_banks(2));
    let mut bus = NullBus;
    f.write_40xx(0x4000, 0x00, &mut bus); // no panic, routed to POKEY
    assert_eq!(f.pokey_bases(), vec![0x4000]);
}

// ---- banksets -----------------------------------------------------------

#[test]
fn bankset_sg_sally_maria_split() {
    let mut c = BanksetSg::new(marker_banks(16), false); // 2x128K
    let mut bus = NullBus;
    // Sally: last bank of first half at $C000; Maria: last of second half.
    assert_eq!(c.read_40xx(0xC000, false, &mut bus), 0x07);
    assert_eq!(c.read_40xx(0xC000, true, &mut bus), 0x0F);
    c.write_40xx(0x8000, 0x03, &mut bus);
    assert_eq!(c.read_40xx(0x8000, false, &mut bus), 0x03);
    assert_eq!(c.read_40xx(0x8000, true, &mut bus), 0x0B);
    // P800 flavor: low window open bus.
    let mut p = BanksetSg::new(marker_banks(16), true);
    assert_eq!(p.read_40xx(0x4000, false, &mut bus), 0xFF);
    assert_eq!(p.read_40xx(0xC000, true, &mut bus), 0x0F);
}

#[test]
fn bankset_ram_sally_writes_maria_bank() {
    let mut c = BanksetSgBankram::new(marker_banks(16), vec![0u8; 0x8000]);
    let mut bus = NullBus;
    c.write_40xx(0x4000, 0xAA, &mut bus); // Sally's bank
    c.write_40xx(0xC000, 0xBB, &mut bus); // -> Maria's bank via $C000
    assert_eq!(c.read_40xx(0x4000, false, &mut bus), 0xAA);
    assert_eq!(c.read_40xx(0x4000, true, &mut bus), 0xBB);
    // Banked ROM still split-view.
    c.write_40xx(0x8000, 0x01, &mut bus);
    assert_eq!(c.read_40xx(0x8000, false, &mut bus), 0x01);
    assert_eq!(c.read_40xx(0x8000, true, &mut bus), 0x09);
}

#[test]
fn bankset_flat_and_bankram_windows() {
    let mut c = BanksetRom::new(marker_banks(4)); // 64K halves of... 64K total
    let mut bus = NullBus;
    // window = 0xC000 - 0x4000 = 0x4000: below is open bus.
    assert_eq!(c.read_40xx(0x4000, false, &mut bus), 0xFF);
    assert_eq!(c.read_40xx(0x8000, false, &mut bus), 0x00);
    assert_eq!(c.read_40xx(0x8000, true, &mut bus), 0x02);
    let mut b = BanksetBankram::new(marker_banks(4), vec![0u8; 0x8000]);
    b.write_40xx(0x4000, 0x11, &mut bus);
    b.write_40xx(0xC000, 0x22, &mut bus);
    assert_eq!(b.read_40xx(0x4000, false, &mut bus), 0x11);
    assert_eq!(b.read_40xx(0x4000, true, &mut bus), 0x22);
    assert_eq!(b.read_40xx(0x8000, false, &mut bus), 0x00);
}

#[test]
fn bankset_52k_windows() {
    let mut c = Bankset52k::new(marker_banks(7)[..0x1A000].to_vec(), false);
    // Pad to exactly 0x1A000 with a distinct tail marker.
    c.rom[0x19FFF] = 0x7E;
    let mut bus = NullBus;
    assert_eq!(c.read_40xx(0x4000, false, &mut bus), c.rom[0x1000]);
    assert_eq!(c.read_40xx(0x4000, true, &mut bus), c.rom[0x1000 + 0xD000]);
    assert_eq!(c.read_30xx(0x3000, false, &mut bus), c.rom[0]);
    assert_eq!(c.read_30xx(0x3000, true, &mut bus), c.rom[0xD000]);
    // P4000 flavor routes $4000-$7FFF writes to the POKEY.
    let mut p = Bankset52k::new(vec![0u8; 0x1A000], true);
    let mut rec = RecordBus::default();
    p.write_40xx(0x4005, 0x66, &mut rec);
    assert_eq!(rec.log, vec![(0x4000, 0x05, 0x66)]);
    assert_eq!(p.pokey_bases(), vec![0x4000]);
    let mut r = BanksetRomPok4000::new(marker_banks(4));
    let mut rec = RecordBus::default();
    r.write_40xx(0x4003, 0x77, &mut rec);
    assert_eq!(rec.log, vec![(0x4000, 0x03, 0x77)]);
    r.write_40xx(0x4010, 0x77, &mut rec); // outside 0..0x10: ignored
    assert_eq!(rec.log.len(), 1);
}

// ---- P450 / P800 matrix --------------------------------------------------

#[test]
fn pokey_address_matrix() {
    let mut rec = RecordBus::default();
    // P450: only $0450-$045F answers; rest of 04xx is open bus.
    let mut t0 = Pokey450::new(Flat::new(marker_banks(2)));
    assert_eq!(t0.read_04xx(0x0450, &mut rec), 0x00);
    assert_eq!(rec.log, vec![(0x0450, 0x00, 0xFF)]);
    assert_eq!(t0.read_04xx(0x0460, &mut rec), 0xFF);
    assert_eq!(rec.log.len(), 1);
    t0.write_04xx(0x045F, 0xAB, &mut rec);
    assert_eq!(rec.log.last(), Some(&(0x0450, 0x0F, 0xAB)));
    assert_eq!(t0.pokey_bases(), vec![0x0450]);
    // Base ROM still visible through the wrapper.
    let mut bus = NullBus;
    assert_eq!(t0.read_40xx(0xC000, false, &mut bus), 0x01);
    // P800: whole 08xx window hits the POKEY.
    let mut u0 = Pokey800::new(Flat::new(marker_banks(2)));
    let mut rec = RecordBus::default();
    assert_eq!(u0.read_08xx(0x0FFF, &mut rec), 0x00);
    assert_eq!(rec.log, vec![(0x0800, 0x0F, 0xFF)]);
    u0.write_08xx(0x0800, 0x12, &mut rec);
    assert_eq!(rec.log.last(), Some(&(0x0800, 0x00, 0x12)));
    assert_eq!(u0.pokey_bases(), vec![0x0800]);
    // Stacked flavors accumulate bases.
    let t1 = Pokey450::new(FlatPokey::new(marker_banks(2)));
    assert_eq!(t1.pokey_bases(), vec![0x0450, 0x4000]);
    let u1 = Pokey800::new(FlatPokey::new(marker_banks(2)));
    assert_eq!(u1.pokey_bases(), vec![0x0800, 0x4000]);
    let vb = VersaPokey::new(marker_banks(16), vec![0u8; 0x8000]);
    assert_eq!(vb.pokey_bases(), vec![0x0450]);
}

// ---- Versa / Mega ---------------------------------------------------------

#[test]
fn versa_and_mega_bank_bits() {
    let mut bus = NullBus;
    let mut v = Versa::new(marker_banks(16), vec![0u8; 0x8000]);
    v.write_40xx(0x8000, 0x23, &mut bus); // bank 3, ram_bank 1
    assert_eq!(v.read_40xx(0x8000, false, &mut bus), 0x03);
    v.write_40xx(0x4000, 0xCC, &mut bus); // RAM bank 1
    v.write_40xx(0x8000, 0x00, &mut bus); // bank 0, ram_bank 0
    assert_eq!(v.read_40xx(0x4000, false, &mut bus), 0x00);
    v.write_40xx(0x8000, 0x20, &mut bus); // ram_bank 1 again
    assert_eq!(v.read_40xx(0x4000, false, &mut bus), 0xCC);
    v.reset();
    assert_eq!(v.read_40xx(0x8000, false, &mut bus), 0x00);
    // MegaCart: 5 bank bits reach bank 31.
    let mut m = Mega::new(marker_banks(32), vec![0u8; 0x8000]);
    m.write_40xx(0x8000, 0x3F, &mut bus);
    assert_eq!(m.read_40xx(0x8000, false, &mut bus), 0x1F);
    // Versa masks bit 4 of the bank select (0x0F).
    v.write_40xx(0x8000, 0x1F, &mut bus);
    assert_eq!(v.read_40xx(0x8000, false, &mut bus), 0x0F);
}

// ---- XM --------------------------------------------------------------------

#[test]
fn xm_control_registers_and_paging() {
    let mut xm = Xm::new(vec![0u8; 0x20000], vec![0xA5u8; 0x1000], None);
    let mut bus = NullBus;
    let mut rec = RecordBus::default();
    // Power-on: everything disabled.
    assert_eq!(xm.read_30xx(0x3000, false, &mut bus), 0xFF);
    assert_eq!(xm.read_04xx(0x0450, &mut bus), 0xFF);
    assert!(!xm.ym_present() && xm.pokey_bases().is_empty());
    // NVRAM powers to 0xFF (battery fill) and round-trips.
    assert_eq!(xm.read_10xx(0x1000, &mut bus), 0xFF);
    xm.write_10xx(0x1000, 0x3C, &mut bus);
    assert_eq!(xm.read_10xx(0x1000, &mut bus), 0x3C);
    // Enable banks/POKEY/YM/HSC but leave the rof bits (0-1) clear: reads
    // OR the rof bit into the address while writes don't (upstream
    // asymmetry), so paging tests need rof off to read back Sally writes.
    xm.write_04xx(0x0470, 0xFC, &mut bus);
    assert!(xm.ym_present());
    assert_eq!(xm.pokey_bases(), vec![0x0450]);
    assert_eq!(xm.read_04xx(0x0453, &mut rec), 0x00);
    assert_eq!(rec.log, vec![(0x0450, 0x03, 0xFF)]);
    assert_eq!(xm.read_04xx(0x0461, &mut rec), 0x00);
    assert_eq!(rec.ym_log, vec![(1, 0xFF)]);
    // HSC ROM appears when cntrl1 bit 3 set.
    assert_eq!(xm.read_30xx(0x3000, false, &mut bus), 0xA5);
    // Banked RAM: Sally page 1 vs Maria page 0 differ.
    xm.write_04xx(0x0478, 0x10, &mut bus); // cntrl2: Sally page1 = 1
    xm.write_04xx(0x047C, 0x00, &mut bus); // cntrl3: Maria pages 0
    xm.write_40xx(0x6000, 0x71, &mut bus); // Sally writes page 1
    assert_eq!(xm.read_40xx(0x6000, false, &mut bus), 0x71);
    assert_eq!(xm.read_40xx(0x6000, true, &mut bus), 0x00);
    // Flat-48K mode + write protect.
    xm.write_04xx(0x0472, 0x01, &mut bus); // cntrl5 bit 0
    xm.write_40xx(0x4000, 0x0E, &mut bus);
    assert_eq!(xm.read_40xx(0x4000, false, &mut bus), 0x0E);
    xm.write_04xx(0x0472, 0x03, &mut bus); // + WE# disable
    xm.write_40xx(0x4000, 0xFF, &mut bus);
    assert_eq!(xm.read_40xx(0x4000, false, &mut bus), 0x0E);
    xm.reset();
    assert_eq!(xm.read_30xx(0x3000, false, &mut bus), 0xFF);
}

// ---- HSC -------------------------------------------------------------------

#[test]
fn hiscore_nvram_and_passthrough() {
    let mut rom = vec![0u8; 0x1000];
    rom[0x42] = 0x99;
    let mut hsc = Hiscore::new(rom, None);
    let mut bus = NullBus;
    assert_eq!(hsc.read_30xx(0x3042, false, &mut bus), 0x99);
    assert_eq!(hsc.read_10xx(0x1000, &mut bus), 0xFF); // battery fill
    hsc.write_10xx(0x1000, 0x5A, &mut bus);
    assert_eq!(hsc.read_10xx(0x1000, &mut bus), 0x5A);
    let saved = save_nvram(&hsc.nvram);
    assert_eq!(saved.len(), 0x800);
    // Reload from the saved image: score survives.
    let hsc2 = Hiscore::new(vec![0u8; 0x1000], Some(&saved));
    assert_eq!(hsc2.nvram[0], 0x5A);
    // history image shorter than NVRAM pads with 0xFF.
    assert_eq!(load_nvram(Some(&[0x01]), 0x800)[1], 0xFF);
    // Passthrough to the top-slot cart.
    let mut hsc3 = Hiscore::new(vec![0u8; 0x1000], None);
    hsc3.inner = Some(Box::new(Flat::new(marker_banks(1)))); // 16K flat
    assert_eq!(hsc3.read_40xx(0xC000, false, &mut bus), 0x00);
    // No inner cart: open bus.
    let mut hsc4 = Hiscore::new(vec![0u8; 0x1000], None);
    assert_eq!(hsc4.read_40xx(0xC000, false, &mut bus), 0xFF);
}

// ---- overlays -----------------------------------------------------------------

#[test]
fn machine_start_overlays() {
    use rusty7800_core::cart::Overlay;
    assert!(overlays_for_type(pcb::TYPE0).is_empty());
    assert_eq!(
        overlays_for_type(pcb::HSC),
        vec![
            Overlay { start: 0x1000, end: 0x17FF, window: 0x10, writable: true },
            Overlay { start: 0x3000, end: 0x3FFF, window: 0x30, writable: true },
        ]
    );
    // 52K+P4000 fallthrough installs both handlers.
    assert_eq!(overlays_for_type(pcb::BANKSET_52K_POK4000).len(), 2);
    assert_eq!(overlays_for_type(pcb::BANKSET_POK4000).len(), 1);
    let p450 = overlays_for_type(pcb::TYPE2_POK450);
    assert_eq!(p450.len(), 1);
    assert_eq!((p450[0].start, p450[0].end, p450[0].window), (0x0400, 0x04FF, 0x04));
    let p800 = overlays_for_type(pcb::BANKSET_BANKRAM_POK800);
    assert_eq!((p800[0].start, p800[0].end, p800[0].window), (0x0800, 0x0FFF, 0x08));
    assert_eq!(overlays_for_type(pcb::XM_BOARD).len(), 3);
}

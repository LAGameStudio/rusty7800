//! Phase 0 acceptance: spec constants + palette tables.
//!
//! Each assertion traces to the A7800 5.2 source cited in `specs.rs` / `palette.rs`.

use rusty7800_core::{palette, specs};
use std::collections::HashSet;

#[test]
fn clocks_and_screen_timing() {
    assert_eq!(specs::CLK_NTSC, 1_789_772);
    assert_eq!(specs::CLK_PAL, 1_773_447);
    assert_eq!(specs::HTOTAL, 454);
    assert_eq!(*specs::H_VISIBLE.end(), 320);
    assert_eq!(specs::LINES_NTSC, 263);
    assert_eq!(specs::LINES_PAL, 313);
    assert_eq!(specs::VISIBLE_LINES_NTSC, 243);
    assert_eq!(specs::VISIBLE_LINES_PAL, 293);
    assert_eq!(specs::FRAME_W, 320);
}

#[test]
fn address_map_ranges() {
    assert_eq!(specs::MAP_TIA, 0x0000..=0x001F);
    assert_eq!(specs::MAP_TIA_MIRROR, 0x300);
    assert_eq!(specs::MAP_MARIA, 0x0020..=0x003F);
    assert_eq!(specs::MAP_RIOT, 0x0280..=0x029F);
    assert_eq!(specs::MAP_RIOT_MIRROR, 0x160);
    assert_eq!(specs::MAP_RIOT_RAM, 0x0480..=0x04FF);
    assert_eq!(specs::MAP_SRAM1, 0x1800..=0x1FFF);
    assert_eq!(specs::MAP_SRAM2, 0x2000..=0x27FF);
    assert_eq!(specs::MAP_CART_READ_LO, 0x4000..=0xBFFF);
    assert_eq!(specs::MAP_BIOS_CART_READ_HI, 0xC000..=0xFFFF);
}

#[test]
fn controller_slots_match_cpp() {
    // ctrl.cpp SLOT_INTERFACE_START: 9 devices, proline default, plus
    // atarivox/savekey (new slots; no MAME originals, Stella-named).
    assert_eq!(specs::CONTROL_PORT_DEVICES.len(), 11);
    assert_eq!(specs::DEFAULT_PORT_DEVICE, "proline_joystick");
    let names: HashSet<_> = specs::CONTROL_PORT_DEVICES.iter().map(|(n, _)| *n).collect();
    for expected in [
        "proline_joystick",
        "vcs_joystick",
        "paddle",
        "lightgun",
        "driving_wheel",
        "cx22_trakball",
        "keypad",
        "amiga_mouse",
        "st_mouse",
        "atarivox",
        "savekey",
    ] {
        assert!(names.contains(expected), "missing port device {expected}");
    }
}

#[test]
fn cart_slots_match_cpp_header() {
    // a78_carts.h: 35 SLOT_INTERFACE_INTERNAL entries, all distinct.
    assert_eq!(specs::CART_SLOTS.len(), 35);
    let uniq: HashSet<_> = specs::CART_SLOTS.iter().collect();
    assert_eq!(uniq.len(), 35, "duplicate cart slot keys");
    assert!(uniq.contains(&"a78_bankset_p4000"));
    assert!(uniq.contains(&"a78_xm"));
    assert!(uniq.contains(&"a78_hsc"));
}

#[test]
fn machines_cover_all_variants() {
    assert_eq!(specs::MACHINES.len(), 8);
    let devmode: Vec<_> = specs::MACHINES.iter().filter(|m| m.devmode).collect();
    assert_eq!(devmode.len(), 2);
    for m in &specs::MACHINES {
        assert_eq!(m.lines, if m.is_pal { 313 } else { 263 });
    }
}

#[test]
fn palettes_have_256_literal_pens_each() {
    assert_eq!(palette::PENS_DEFINED, 256);
    assert_eq!(palette::PENS_TOTAL, 768);
    assert_eq!(palette::ALL_PALETTES.len(), 6);
    // Spot checks against a7800.cpp literal triplets.
    // HUE0 grayscale ramp head (all palettes share NTSC_HUE0_PAL_HUE0 tail/head).
    assert_eq!(palette::PALETTE_WARM_NTSC[0], (0x00, 0x00, 0x00));
    assert_eq!(palette::PALETTE_WARM_NTSC[15], (0xFF, 0xFF, 0xFF));
    // Warm NTSC hue1 start: NTSC_HUE1_PAL_HUE2 head rgb_t(0x1A,0x00,0x00).
    assert_eq!(palette::PALETTE_WARM_NTSC[16], (0x1A, 0x00, 0x00));
    // Warm PAL hue1 start: PAL_HUE1 head rgb_t(0x00,0x13,0x00).
    assert_eq!(palette::PALETTE_WARM_PAL[16], (0x00, 0x13, 0x00));
    // Hot NTSC hue1 start: NTSC_HUE1_PAL_HUE2_U2 head rgb_t(0x1A,0x00,0x00).
    assert_eq!(palette::PALETTE_HOT_NTSC[16], (0x1A, 0x00, 0x00));
    // Hot NTSC gold tail differs from Warm: NTSC_HUE15_U2 tail rgb_t(0xFF,0xFA,0xA1).
    assert_eq!(palette::PALETTE_HOT_NTSC[255], (0xFF, 0xFA, 0xA1));
    // 0X51 typo triplet preserved: NTSC_HUE9_PAL_HUE10_U2 row 8 of hue 9.
    // Hue 9 starts at pen 9*16=144; 9th entry (idx 8) = (0x51,0xA4,0xE3).
    assert_eq!(palette::PALETTE_HOT_NTSC[144 + 8], (0x51, 0xA4, 0xE3));
    // NTSC 320-mode gold tail: NTSC_HUE15 tail rgb_t(0xFF,0xFF,0x8E).
    assert_eq!(palette::PALETTE_WARM_NTSC[255], (0xFF, 0xFF, 0x8E));
}

#[test]
fn rom_identity_and_sizes() {
    assert_eq!(specs::A78_HEADER_LEN, 128);
    assert_eq!(specs::HSC_ROM_LEN, 4096);
    assert_eq!(specs::HSC_NVRAM_LEN, 2048);
    assert_eq!(specs::XM_RAM_LEN, 131_072);
    assert_eq!(specs::POKEY_BASE_P450, 0x0450);
    assert_eq!(specs::POKEY_BASE_P800, 0x0800);
    assert_eq!(specs::PADDLE_CYCLE_DIVISOR, 97);
}

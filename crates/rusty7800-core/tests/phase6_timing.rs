//! Phase 6 acceptance: 8 machine configs, screen params, scanline
//! schedule, palette selector + checksums, BIOS boot vectors, mute flag.

use rusty7800_core::timing::{
    audio_muted, palette_checksum, reset_ctrl_reg, BiosImage, MachineId, ScreenGeometry,
    DMA_DELAY_CYCLES, H_VISIBLE_END, HTOTAL, PIXEL_CLOCK_MUL, SCREEN_DEFAULT_POSITION,
    VISIBLE_DELAY_CYCLES, V_VISIBLE_START,
};

#[test]
fn eight_machines_cover_drivers() {
    let ids = MachineId::all();
    assert_eq!(ids.len(), 8);
    let names: Vec<_> = ids.iter().map(|id| id.driver_name()).collect();
    assert_eq!(
        names,
        vec!["a7800", "a7800u1", "a7800u2", "a7800dev", "a7800p", "a7800pu1", "a7800pu2", "a7800pdev"]
    );
    for id in ids {
        let spec = id.spec();
        assert_eq!(spec.name, id.driver_name());
        assert_eq!(spec.lines, if spec.is_pal { 313 } else { 263 });
        assert_eq!(id.clock_hz(), if spec.is_pal { 1_773_447 } else { 1_789_772 });
        assert_eq!(id.softlist_filter(), if spec.is_pal { "PAL" } else { "NTSC" });
    }
    // Dev machines: same clocks/lines as base, devmode set.
    assert!(!MachineId::Ntsc.spec().devmode && MachineId::NtscDev.spec().devmode);
    assert!(!MachineId::Pal.spec().devmode && MachineId::PalDev.spec().devmode);
    assert_eq!(MachineId::NtscDev.clock_hz(), MachineId::Ntsc.clock_hz());
}

#[test]
fn screen_params_match_raw_macros() {
    assert_eq!((PIXEL_CLOCK_MUL, HTOTAL, H_VISIBLE_END), (4, 454, 320));
    assert_eq!(V_VISIBLE_START, 16);
    assert_eq!(SCREEN_DEFAULT_POSITION, (1.0, 0.0, 1.0, 0.0));
    // NTSC frame = 263 lines / PAL = 313; visible windows 16..16+243/293.
    let ntsc = ScreenGeometry::for_machine(MachineId::Ntsc);
    let pal = ScreenGeometry::for_machine(MachineId::Pal);
    assert_eq!((ntsc.lines, pal.lines), (263, 313));
    assert_eq!(ntsc.visible_range(), (16, 259));
    assert_eq!(pal.visible_range(), (16, 309));
    assert!(ntsc.is_visible_line(16) && ntsc.is_visible_line(258));
    assert!(!ntsc.is_visible_line(15) && !ntsc.is_visible_line(259));
    assert!(pal.is_visible_line(308) && !pal.is_visible_line(309));
    // Derived rates: NTSC ~59.95 Hz, PAL ~49.92 Hz.
    let ntsc_fps = ntsc.frame_rate_hz(MachineId::Ntsc.clock_hz());
    let pal_fps = pal.frame_rate_hz(MachineId::Pal.clock_hz());
    assert!((ntsc_fps - 59.95).abs() < 0.05, "{ntsc_fps}");
    assert!((pal_fps - 49.92).abs() < 0.05, "{pal_fps}");
}

#[test]
fn scanline_schedule() {
    // DMA begins 7 cycles after hblank; pixels after 33.5 cycles.
    assert_eq!((DMA_DELAY_CYCLES, VISIBLE_DELAY_CYCLES), (7.0, 33.5));
}

#[test]
fn palette_selector_and_checksums() {
    use rusty7800_core::palette;
    // Warm/Cool/Hot × NTSC/PAL; dev machines reuse Warm. (Compared by
    // value: ALL_PALETTES holds copies, so pointer identity is meaningless.)
    assert_eq!(*MachineId::Ntsc.palette(), palette::PALETTE_WARM_NTSC);
    assert_eq!(*MachineId::NtscCool.palette(), palette::PALETTE_COOL_NTSC);
    assert_eq!(*MachineId::NtscHot.palette(), palette::PALETTE_HOT_NTSC);
    assert_eq!(*MachineId::NtscDev.palette(), palette::PALETTE_WARM_NTSC);
    assert_eq!(*MachineId::Pal.palette(), palette::PALETTE_WARM_PAL);
    assert_eq!(*MachineId::PalCool.palette(), palette::PALETTE_COOL_PAL);
    assert_eq!(*MachineId::PalHot.palette(), palette::PALETTE_HOT_PAL);
    assert_eq!(*MachineId::PalDev.palette(), palette::PALETTE_WARM_PAL);
    // Per-table checksums: all six distinct and nonzero (pinned).
    let sums: Vec<u32> = MachineId::all()
        .iter()
        .map(|id| palette_checksum(id.palette()))
        .collect();
    let mut uniq = sums.clone();
    uniq.sort_unstable();
    uniq.dedup();
    assert_eq!(uniq.len(), 6, "{sums:X?}");
    assert!(sums.iter().all(|s| *s != 0));
    println!("palette checksums: {sums:08X?}");
}

#[test]
fn bios_placement_and_routing() {
    // NTSC 7800.u7: 4K at 0x3000; ERASEFF elsewhere.
    let mut bios = BiosImage::new();
    assert!(!bios.present);
    let file = vec![0xABu8; 0x1000];
    bios.place(&file, 0x3000);
    assert!(bios.present);
    assert_eq!(bios.read(0xF000, 0x00, 0xFF), 0xAB);
    assert_eq!(bios.read(0xC000, 0x00, 0xFF), 0xFF);
    // Prototype / PAL full 16K at 0x0000.
    let mut full = BiosImage::new();
    full.place(&vec![0xCDu8; 0x4000], 0x0000);
    assert_eq!(full.read(0xC000, 0x00, 0xFF), 0xCD);
    assert_eq!(full.read(0xFFFF, 0x00, 0xFF), 0xCD);
    // Oversized file truncates to the region.
    let mut trunc = BiosImage::new();
    trunc.place(&vec![0x11u8; 0x5000], 0x0000);
    assert_eq!(trunc.read(0xFFFF, 0x00, 0xFF), 0x11);
    // INPTCTRL bit 2 flips $C000-$FFFF to the cart.
    assert_eq!(bios.read(0xF000, 0x04, 0x77), 0x77);
    assert_eq!(bios.read(0xF000, 0x00, 0x77), 0xAB);
}

#[test]
fn reset_vector_flip_and_mute() {
    // Blank vectors (no BIOS) -> cart flip; present BIOS -> stay.
    assert_eq!(reset_ctrl_reg(0xFF, 0xFF), 0x04);
    assert_eq!(reset_ctrl_reg(0x00, 0xFF), 0x00);
    assert_eq!(reset_ctrl_reg(0xFF, 0x00), 0x00);
    assert_eq!(reset_ctrl_reg(0x78, 0xFF), 0x00);
    // Silent empty-boot: muted unless cart or BIOS present.
    assert!(audio_muted(false, false));
    assert!(!audio_muted(true, false));
    assert!(!audio_muted(false, true));
}

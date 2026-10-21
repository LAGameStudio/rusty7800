//! Phase 7a acceptance: the machine boots and runs.
//!
//! A synthesized headerless 4K ROM (flat, no repo binaries needed) brings
//! up MARIA for real: 16-line zone, one 8-byte direct-mode header, pens
//! out of `palette[7]`. Covers cart boot, BIOS handoff (flip + trampoline
//! re-entry, see below), and silent PAL empty-boot. Bar geometry: hpos
//! 20, 8 bytes × 4 cells = cells 20..52, half-pixels 40..104, displayed
//! rows 17..=33 (one-line DMA lag plus MAME's doubled first zone line:
//! the latch line draws but does not advance the offset).

use rusty7800_core::cart::load_image;
use rusty7800_core::machine::FRAME_W;
use rusty7800_core::timing::MachineId;

/// 4K flat ROM: reset → MARIA init → spin. Vectors point at $F000.
fn bar_rom() -> Vec<u8> {
    let mut img = vec![0xFFu8; 0x1000];
    // Code at $F000.
    let code: &[u8] = &[
        0x78, // SEI
        0xD8, // CLD
        0xA2, 0xFF, // LDX #$FF
        0x9A, // TXS
        0xA9, 0x00, // LDA #$00
        0x8D, 0x20, 0x00, // STA $0020 BACKGRND
        0xA9, 0x05, // LDA #$05
        0x8D, 0x21, 0x00, // STA $0021 palette[1]
        0xA9, 0x3F, // LDA #$3F
        0x8D, 0x27, 0x00, // STA $0027 palette[7]
        0xA9, 0xF1, // LDA #$F1
        0x8D, 0x2C, 0x00, // STA $002C DPPH -> DLL at $F100
        0xA9, 0x00, // LDA #$00
        0x8D, 0x30, 0x00, // STA $0030 DPPL (DPP must name the DLL, not the DL)
        0xA9, 0x40, // LDA #$40
        0x8D, 0x3C, 0x00, // STA $003C CTRL: DMA on, 160A
        0x4C, 0x23, 0xF0, // JMP $F023 (self)
    ];
    img[..code.len()].copy_from_slice(code);
    // DLL at $F100: 16-line zone -> DL $F110, then a zero-offset entry
    // (DL $00F1, ignored) whose post-zone draws end at the MARIA-shadow
    // gate: header byte `dl+1` = $00F2 reads 0x00, so every trailing line
    // costs a clean 16 cycles with no cells and no NMI.
    img[0x100..0x108].copy_from_slice(&[0x0F, 0xF1, 0x10, 0x00, 0x00, 0xF1, 0x2F, 0x00]);
    // DL at $F110: graph $F026, width 8, pal 1, hpos 20. The end-of-list
    // gate reads `dl+1` *after* the 4-byte header advances past it, so
    // the terminator belongs at $F115 — a 0x00 at $F114 would be skipped
    // and DMA would run away through $FF fill to DMALIMIT every line.
    // Graph placement matters too: DMA data fetches wrap at 64 KiB, so
    // graph $F1xx + (offset 15 << 8) wraps to the $00xx MARIA shadows
    // (reads 0x00 = transparent) and the zone's first two lines draw
    // blank. $F026 sits past the init code with $F026 + $0F00 = $FF26
    // staying inside the cart for every zone offset.
    img[0x110..0x114].copy_from_slice(&[0x26, 0x38, 0xF0, 20]);
    img[0x115] = 0x00;
    // Graphics at $F026: solid pens (pal 1 -> cell 7 -> palette[7]).
    img[0x026..0x02E].copy_from_slice(&[0xFF; 8]);
    img[0x130] = 0x00;
    // Vectors -> $F000 / spin loop.
    img[0xFFA] = 0x23;
    img[0xFFB] = 0xF0;
    img[0xFFC] = 0x00;
    img[0xFFD] = 0xF0;
    img[0xFFE] = 0x23;
    img[0xFFF] = 0xF0;
    img
}

fn row(frame: &[u8], line: usize) -> &[u8] {
    &frame[line * FRAME_W..(line + 1) * FRAME_W]
}

fn assert_bar(frame: &[u8]) {
    for line in 17..=33 {
        let r = row(frame, line);
        assert!(r[..40].iter().all(|&p| p == 0), "line {line} left bg");
        assert!(r[40..104].iter().all(|&p| p == 0x3F), "line {line} bar");
        assert!(r[104..].iter().all(|&p| p == 0), "line {line} right bg");
    }
    assert!(row(frame, 0).iter().all(|&p| p == 0));
    assert!(row(frame, 16).iter().all(|&p| p == 0));
    assert!(row(frame, 34).iter().all(|&p| p == 0));
}

/// Cart variant for the BIOS test: after `STA $00` flips INPTCTRL,
/// subsequent code fetches come from the *cart* (hardware truth — the
/// BIOS bytes are unmapped the instant the flip executes). The
/// trampoline at $FF04 re-enters through the cart's own reset vector,
/// so the handoff lands in real cart code instead of $FF fog.
fn bar_rom_trampoline() -> Vec<u8> {
    let mut img = bar_rom();
    img[0xF04..0xF07].copy_from_slice(&[0x6C, 0xFC, 0xFF]); // JMP ($FFFC)
    img
}

#[test]
fn cart_boot_draws_bar() {
    let loaded = load_image(&bar_rom()).expect("headerless 4K loads flat");
    assert_eq!(loaded.slot_key, "a78_rom");
    let mut m = rusty7800_core::machine::Machine::new(MachineId::Ntsc);
    m.load_cart(loaded);
    assert!(!m.muted());
    m.reset();
    // No BIOS: vectors blank -> flipped straight to cart.
    assert_eq!(m.ctrl_reg(), 4);
    assert_eq!(m.sally.pc, 0xF000);
    m.step_frame();
    assert_eq!(m.vpos(), 0);
    assert_eq!(m.frame_dims(), (320, 263));
    assert_bar(m.frame());
    // Spinning in the JMP-self loop.
    assert_eq!(m.sally.pc, 0xF023);
    // ~1s of TIA silence at the driver rate (31400/59.94 ~= 524).
    let audio = m.take_audio();
    assert!((520..=528).contains(&audio.len()), "len {}", audio.len());
    assert!(audio.iter().all(|&s| s == 0));
}

#[test]
fn bios_hands_off_to_cart() {
    // Fake 4K BIOS at $3000 (like 7800.u7): INPTCTRL flip + jump through
    // the cart's reset vector.
    let mut bios = vec![0xFFu8; 0x1000];
    bios[0xF00..0xF07].copy_from_slice(&[
        0xA9, 0x04, // LDA #$04
        0x85, 0x00, // STA $00 (INPTCTRL: cart on)
        0x6C, 0xFC, 0xFF, // JMP ($FFFC)
    ]);
    bios[0xFFC] = 0x00;
    bios[0xFFD] = 0xFF;
    // MAME detects the BIOS via $FFFE/$FFFF (not the reset vector): leave
    // the IRQ vector non-blank or reset flips straight to cart.
    bios[0xFFE] = 0x00;
    bios[0xFFF] = 0xFF;
    let mut m = rusty7800_core::machine::Machine::new(MachineId::Ntsc);
    m.load_bios(&bios, 0x3000);
    m.load_cart(load_image(&bar_rom_trampoline()).expect("cart loads"));
    assert!(!m.muted());
    m.reset();
    // BIOS present: no flip; reset vector comes from the BIOS.
    assert_eq!(m.ctrl_reg(), 0);
    assert_eq!(m.sally.pc, 0xFF00);
    m.step_frame();
    // BIOS flipped to cart and the cart drew its bar.
    assert_eq!(m.ctrl_reg(), 4);
    assert_bar(m.frame());
}

#[test]
fn pal_empty_boot_is_silent_black() {
    let mut m = rusty7800_core::machine::Machine::new(MachineId::Pal);
    assert!(m.muted());
    m.reset();
    assert_eq!(m.ctrl_reg(), 4);
    assert_eq!(m.sally.pc, 0xFFFF);
    m.step_frame();
    assert_eq!(m.frame_dims(), (320, 313));
    assert!(m.frame().iter().all(|&p| p == 0));
    let audio = m.take_audio();
    assert!((625..=633).contains(&audio.len()), "len {}", audio.len());
    assert!(audio.iter().all(|&s| s == 0));
}

//! Phase 2 acceptance: MARIA registers, DMA, modes, DLL walk, overlay.
//!
//! Hand-built display lists executed against flat RAM, with hand-computed
//! DMA cycle expectations from the C++ accounting rules.

use rusty7800_core::maria::{Maria, MariaBus};

struct Mem([u8; 65536]);

impl Mem {
    fn new() -> Self {
        Mem([0; 65536])
    }
}

impl MariaBus for Mem {
    fn read(&mut self, addr: u16) -> u8 {
        self.0[addr as usize]
    }
}

/// Enable DMA with given CTRL low bits (DMA-on bit 0x40 + rm etc.).
fn dma_on(m: &mut Maria, ctrl_lo: u8) {
    m.write(0x1C, 0x40 | ctrl_lo);
}

#[test]
fn registers_decode() {
    let mut m = Maria::new();
    // BACKGRND fans out to pens 0,4,...,28.
    m.write(0x00, 0xAB);
    for i in 0..8 {
        assert_eq!(m.palette[4 * i], 0xAB);
    }
    // Color-RAM writes ((offset&3)!=0).
    m.write(0x01, 0x11);
    m.write(0x1F, 0x22);
    assert_eq!(m.palette[1], 0x11);
    assert_eq!(m.palette[0x1F], 0x22);
    // CTRL decode.
    m.write(0x1C, 0x40 | 0x10 | 0x04 | 0x03);
    assert!(m.dmaon());
    m.write(0x1C, 0x60);
    assert!(!m.dmaon());
    m.write(0x1C, 0x00); // DMA test mode: dmaon unchanged (still off)
    assert!(!m.dmaon());
    m.write(0x1C, 0x80 | 0x40); // color kill + dma on
    // MSTAT + undefined reads.
    assert_eq!(m.read(0x08), 0x80); // vblank set after reset
    assert_eq!(m.read(0x00), 0x00);
    // WSYNC latches; interrupt() releases with trigger.
    m.write(0x04, 0);
    assert!(m.wsync_latched());
    assert!(m.interrupt(100, 263));
    assert!(!m.wsync_latched());
    assert!(!m.interrupt(100, 263));
}

#[test]
fn vblank_timing() {
    let mut m = Maria::new();
    m.interrupt(16, 263);
    assert_eq!(m.vblank(), 0x00);
    assert_eq!(m.read(0x08), 0x00);
    m.interrupt(258, 263); // lines - 5
    assert_eq!(m.vblank(), 0x80);
    // PAL geometry.
    m.interrupt(16, 313);
    assert_eq!(m.vblank(), 0x00);
    m.interrupt(308, 313);
    assert_eq!(m.vblank(), 0x80);
}

#[test]
fn direct_mode_draw_and_cycle_count() {
    let mut mem = Mem::new();
    let mut m = Maria::new();
    dma_on(&mut m, 0x00); // rm = 160A/B
    // DPP -> DLL at 0x3000.
    m.write(0x0C, 0x30);
    m.write(0x10, 0x00);
    // DLL: offset 0 (last line), DL @ 0x3100, no DLI.
    mem.0[0x3000] = 0x00;
    mem.0[0x3001] = 0x31;
    mem.0[0x3002] = 0x00;
    mem.0[0x3003] = 0x00;
    // DL normal header: graph @ 0x4000, width 4 (0x1C), pal 1, hpos 10.
    mem.0[0x3100] = 0x00;
    mem.0[0x3101] = 0x3C;
    mem.0[0x3102] = 0x40;
    mem.0[0x3103] = 10;
    mem.0[0x3104] = 0x00; // terminator ((b & 0x5F) == 0)
    // Graphics bytes.
    mem.0[0x4000] = 0xFF; // -> 7,7,7,7
    mem.0[0x4001] = 0x00; // transparent -> stays 0
    mem.0[0x4002] = 0x93; // -> 6,5,skip,7
    mem.0[0x4003] = 0x00; // transparent
    // pal=1 -> palb = 4.
    m.write(0x01, 0x50); // palette[1] (unused here, cells carry own pens)

    let (cycles, halt, nmi) = m.startdma(&mut mem, 16, 263);
    assert!(!halt && !nmi);
    // 16 startup + 8 header + 4*3 data + 8 shutdown = 44.
    assert_eq!(cycles, 44);
    assert_eq!(m.dma_cycles(), 44);
    let buf = m.working_buffer();
    assert_eq!(&buf[10..14], &[7, 7, 7, 7]);
    assert_eq!(&buf[14..18], &[0, 0, 0, 0]);
    assert_eq!(&buf[18..22], &[6, 5, 0, 7]);
    assert_eq!(&buf[22..26], &[0, 0, 0, 0]);
    assert_eq!(buf[9], 0);
    assert_eq!(buf[26], 0);
}

#[test]
fn indirect_cwidth_draw_and_hole_penalty() {
    let mut mem = Mem::new();
    let mut m = Maria::new();
    dma_on(&mut m, 0x00);
    m.write(0x0C, 0x30);
    m.write(0x10, 0x00);
    m.write(0x14, 0x20); // CHARBASE 0x2000
    // DLL: offset 2 (continuing zone -> no shutdown penalty), holey 0.
    mem.0[0x3000] = 0x02;
    mem.0[0x3001] = 0x31;
    mem.0[0x3002] = 0x00;
    mem.0[0x3003] = 0x00;
    // Extended header: write_mode + indirect, width 2, pal 2, hpos 0.
    // Byte1 0xE0: 80 write_mode | 20 ind | 40 end-gate filler (low5 == 0
    // marks extended; bit 6 keeps (b & 0x5F) != 0 so the list continues).
    mem.0[0x3100] = 0x00; // graph lo
    mem.0[0x3101] = 0xE0;
    mem.0[0x3102] = 0x40; // graph hi
    mem.0[0x3103] = 0x5E; // pal 2 (>>5), width ((^FF)&1F)+1 = 2
    mem.0[0x3104] = 0; // hpos
    mem.0[0x3105] = 0x00; // terminator
    // Map bytes -> chars 0x05, 0x06; data at 0x2005+0x200, 0x2006+0x200.
    mem.0[0x4000] = 0x05;
    mem.0[0x4001] = 0x06;
    mem.0[0x2205] = 0xE4;
    mem.0[0x2206] = 0x00;
    mem.0[0x2305] = 0x00;
    mem.0[0x2306] = 0x00;

    let (cycles, halt, _) = m.startdma(&mut mem, 16, 263);
    assert!(!halt);
    // 16 startup + 10 ext header + 2*(3 indirect + 3 write), no shutdown
    // (zone continues, offset 2) = 38.
    assert_eq!(cycles, 38);
    // pal=2 -> palb=8. 0xE4: c1 = 8&10? (8&0x10)=0 | (0xE4&0x0C)=0x04 |
    // (0xE4>>6)=0x03 -> 0x07; c2 = 0 | ((0xE4&3)<<2)=0 | ((0xE4&0x30)>>4)=2.
    let buf = m.working_buffer();
    assert_eq!(&buf[0..4], &[7, 2, 0, 0]);

    // Now the holey variant: DLL holey=2 (0x9000 holes), CHARBASE 0x90,
    // same zone offset 2 -> data at 0x9205/0x9206: both holey.
    let mut m2 = Maria::new();
    dma_on(&mut m2, 0x00);
    m2.write(0x0C, 0x30);
    m2.write(0x10, 0x00);
    m2.write(0x14, 0x90);
    mem.0[0x3000] = 0x02 | 0x40; // offset 2, holey 2
    mem.0[0x9205] = 0xFF; // would render if not holey
    let (cycles, halt, _) = m2.startdma(&mut mem, 16, 263);
    assert!(!halt);
    // 16 + 10 header + 3 (first-hole penalty, once) = 29.
    assert_eq!(cycles, 29);
    assert!(m2.working_buffer().iter().all(|&c| c == 0));
    assert!(m2.is_holey(0x9205));
    assert!(!m2.is_holey(0x8205));
}

#[test]
fn cwidth_double_write_costs_six() {
    let mut mem = Mem::new();
    let mut m = Maria::new();
    dma_on(&mut m, 0x10); // CW bit
    m.write(0x0C, 0x30);
    m.write(0x10, 0x00);
    m.write(0x14, 0x20);
    mem.0[0x3000] = 0x02; // continuing zone
    mem.0[0x3001] = 0x31;
    mem.0[0x3002] = 0x00;
    mem.0[0x3003] = 0x00;
    mem.0[0x3100] = 0x00;
    mem.0[0x3101] = 0xE0; // write_mode + indirect + end-gate bit 6
    mem.0[0x3102] = 0x40;
    mem.0[0x3103] = 0x5E; // width 2, pal 2
    mem.0[0x3104] = 0;
    mem.0[0x3105] = 0x00;
    mem.0[0x4000] = 0x05;
    mem.0[0x2205] = 0xE4;
    mem.0[0x2206] = 0x1B;
    let (cycles, halt, _) = m.startdma(&mut mem, 16, 263);
    assert!(!halt);
    // 16 + 10 + 2*(3 + 6) = 44.
    assert_eq!(cycles, 44);
    // 0xE4 -> [7,2]. 0x1B: c1 = 0|(0x1B&0x0C)=0x08|0 = 8, but c&3 == 0 so the
    // cell stays transparent (no kangaroo); c2 = 0x0C|0x01 = 0x0D written.
    assert_eq!(&m.working_buffer()[0..4], &[7, 2, 0, 13]);
}

#[test]
fn dli_penalty_and_dma_limit_halt() {
    let mut mem = Mem::new();
    let mut m = Maria::new();
    dma_on(&mut m, 0x00);
    m.write(0x0C, 0x30);
    m.write(0x10, 0x00);
    // Last-line zone with DLI bit in DLL byte 3.
    mem.0[0x3000] = 0x00;
    mem.0[0x3001] = 0x31;
    mem.0[0x3002] = 0x00;
    mem.0[0x3003] = 0x80;
    // One normal DL, width 1 (0x1F: low5 all set -> width 1, pal 0).
    mem.0[0x3100] = 0x00;
    mem.0[0x3101] = 0x1F;
    mem.0[0x3102] = 0x40;
    mem.0[0x3103] = 0;
    mem.0[0x3104] = 0x00;
    mem.0[0x4000] = 0xFF;
    let (cycles, halt, _) = m.startdma(&mut mem, 16, 263);
    assert!(!halt);
    // 16 + 8 + 3 + 8 shutdown + 17 DLI = 52.
    assert_eq!(cycles, 52);

    // DMA-limit: 4 chained width-32 DLs -> 16 + 4*(8+96) = 432 >= 430.
    let mut m2 = Maria::new();
    dma_on(&mut m2, 0x00);
    m2.write(0x0C, 0x30);
    m2.write(0x10, 0x00);
    mem.0[0x3000] = 0x03; // multi-line zone (offset counts down; shutdown
    mem.0[0x3001] = 0x31; // hits on the last line, after the limit anyway)
    mem.0[0x3002] = 0x00;
    mem.0[0x3003] = 0x00;
    // Extended headers (low5 of byte1 == 0): [g_lo, mode|0x40 gate, g_hi,
    // width_pal, hpos]. Byte3 0x1F -> width 32, pal 0; +10 header cycles each.
    for i in 0..4 {
        let dl = 0x3100 + i * 5;
        mem.0[dl] = 0x00;
        mem.0[dl + 1] = 0x40; // direct, mode0, end-gate bit 6
        mem.0[dl + 2] = 0x40;
        // Byte3 0x00: width ((^FF)&1F)+1 = 32, pal 0.
        mem.0[dl + 3] = 0x00;
        mem.0[dl + 4] = 0;
    }
    mem.0[0x3114] = 0x00; // terminator (gate byte of next header)
    mem.0[0x3114 + 1] = 0x00;
    for a in 0x4000..0x4020 {
        mem.0[a] = 0xFF;
    }
    let (cycles, halt, _) = m2.startdma(&mut mem, 16, 263);
    assert!(halt);
    assert!(cycles >= 430);
    assert!(m2.dma_limit_hit());
    assert!(m2.wsync_latched()); // CPU held to HSYNC like MAME
}

#[test]
fn dll_walk_counts_down_and_advances_with_nmi() {
    let mut mem = Mem::new();
    let mut m = Maria::new();
    dma_on(&mut m, 0x00);
    m.write(0x0C, 0x30);
    m.write(0x10, 0x00);
    // Zone 1: 3 lines (offset 2), DL1 draws nothing (terminator immediately).
    mem.0[0x3000] = 0x02;
    mem.0[0x3001] = 0x31;
    mem.0[0x3002] = 0x00;
    mem.0[0x3003] = 0x00;
    mem.0[0x3100] = 0x00;
    mem.0[0x3101] = 0x00; // terminator
    // DLL entries are 3 bytes: [b0, dl_lo, dl_hi]. Zone1 @0x3000,
    // zone2 @0x3003. Zone 2: 2 lines (offset 1) + NMI; DL2 draws opaque.
    // NOTE: DLL address bytes are big-endian: [b0, DL_HI, DL_LO] per the
    // C++ `m_dl = (READ_MEM(m_dll+1) << 8) | READ_MEM(m_dll+2)`.
    mem.0[0x3000] = 0x02;
    mem.0[0x3001] = 0x32; // DL1 hi (0x3200)
    mem.0[0x3002] = 0x00; // DL1 lo
    mem.0[0x3200] = 0x00;
    mem.0[0x3201] = 0x00; // terminator
    mem.0[0x3003] = 0x01 | 0x80; // zone2: offset 1 + NMI
    mem.0[0x3004] = 0x33; // DL2 hi (0x3300)
    mem.0[0x3005] = 0x00; // DL2 lo
    mem.0[0x3300] = 0x00; // graph lo
    mem.0[0x3301] = 0x1E; // width 1, pal 0
    mem.0[0x3302] = 0x40; // graph hi
    mem.0[0x3303] = 20; // hpos
    mem.0[0x3304] = 0x00; // terminator
    mem.0[0x4000] = 0xFF; // opaque byte (zone offset 0 line)
    mem.0[0x4100] = 0xFF; // opaque byte (zone offset 1 line: graph + 0x100)

    // Frame 16: zone1 latched (offset 2), drawn, no advance (frame == 16).
    let (_, _, nmi) = m.startdma(&mut mem, 16, 263);
    assert!(!nmi);
    assert_eq!(m.dll(), 0x3000);
    assert_eq!(m.zone_offset(), 2);
    // Frames 17,18: countdown 2 -> 1 -> 0.
    m.startdma(&mut mem, 17, 263);
    assert_eq!(m.zone_offset(), 1);
    m.startdma(&mut mem, 18, 263);
    assert_eq!(m.zone_offset(), 0);
    // Frame 19: last line drawn, DLL advances to zone2, NMI fires.
    let (_, _, nmi) = m.startdma(&mut mem, 19, 263);
    assert!(nmi);
    assert_eq!(m.dll(), 0x3003);
    assert_eq!(m.zone_offset(), 1);
    // Frame 20: zone2's DL2 draws the opaque byte at hpos 20.
    m.startdma(&mut mem, 20, 263);
    assert_eq!(&m.working_buffer()[20..24], &[3, 3, 3, 3]);
    // Frame 15 and below / past the end: no drawing.
    let (cycles, _, _) = m.startdma(&mut mem, 10, 263);
    assert_eq!(cycles, 0);
    let (cycles, _, _) = m.startdma(&mut mem, 260, 263);
    assert_eq!(cycles, 0);
}

#[test]
fn render_modes_and_mid_scanline_palette() {
    let mut mem = Mem::new();
    let mut m = Maria::new();
    dma_on(&mut m, 0x00);
    m.write(0x0C, 0x30);
    m.write(0x10, 0x00);
    m.write(0x00, 0x07); // BACKGRND = 7
    m.write(0x05, 0x12); // palette[5]
    m.write(0x1E, 0x1C); // palette[0x1E]
    mem.0[0x3000] = 0x00;
    mem.0[0x3001] = 0x31;
    mem.0[0x3002] = 0x00;
    mem.0[0x3003] = 0x00;
    // DL: width 2 at hpos 0, pal 0, transparent data.
    mem.0[0x3100] = 0x00;
    mem.0[0x3101] = 0x1D; // width 2, pal 0
    mem.0[0x3102] = 0x40;
    mem.0[0x3103] = 0;
    mem.0[0x3104] = 0x00;
    mem.0[0x4000] = 0x00;
    mem.0[0x4001] = 0x00;
    m.startdma(&mut mem, 16, 263);
    m.startdma(&mut mem, 10, 263); // flip only: standby now holds the draw
    // 160A: doubled background pen.
    let p = m.render_pixel(0);
    assert!(!p.dev_overlay);
    assert_eq!((p.left, p.right), (0x07, 0x07));

    // Mid-scanline BACKGRND change shows mid-scanline.
    m.write(0x00, 0x09);
    let p2 = m.render_pixel(1);
    assert_eq!((p2.left, p2.right), (0x09, 0x09));

    // 320B bit math on a crafted cell is covered via direct cell checks:
    // standby cell at hpos 0 is 0 (transparent). Draw opaque 0x1E cell:
    mem.0[0x4000] = 0x78; // 01 11 10 00 -> pens 1,3,2,0 (pal 0)
    m.write(0x00, 0x07);
    m.startdma(&mut mem, 16, 263);
    m.startdma(&mut mem, 10, 263);
    // rm = 2 (320B/D). Byte 0x78 -> pens [1,3,2,0] at cells 0..3 (verified
    // through the render path: the draw sits in the standby buffer now).
    dma_on(&mut m, 0x02);
    // Cell 0x01: d0 = b4|b1|b3 = 0; d1 = b4|b0|b2 = 2.
    let px = m.render_pixel(0);
    assert_eq!((px.left, px.right), (m.palette[0], m.palette[2]));
    // rm = 3 (320A/C). Byte 0x1E -> pens [0,1,3,2] at cells 0..3.
    mem.0[0x4000] = 0x1E;
    dma_on(&mut m, 0x03);
    m.startdma(&mut mem, 16, 263);
    m.startdma(&mut mem, 10, 263);
    // Cell 0x01: d0 = b4b3b2|b1 = 0; d1 = b4b3b2|b0 = 2.
    let px = m.render_pixel(1);
    assert_eq!((px.left, px.right), (m.palette[0], m.palette[2]));
    // Color kill masks pens to luminance.
    m.write(0x1C, 0x80 | 0x40 | 0x03);
    let px = m.render_pixel(1);
    assert_eq!(
        (px.left, px.right),
        (m.palette[0] & 0x0F, m.palette[2] & 0x0F)
    );
}

#[test]
fn devmode_dma_overlay() {
    let mut mem = Mem::new();
    let mut m = Maria::new();
    m.devmode_flag = true;
    dma_on(&mut m, 0x00);
    m.write(0x0C, 0x30);
    m.write(0x10, 0x00);
    mem.0[0x3000] = 0x00;
    mem.0[0x3001] = 0x31;
    mem.0[0x3002] = 0x00;
    mem.0[0x3003] = 0x00;
    mem.0[0x3100] = 0x00;
    mem.0[0x3101] = 0x3C; // width 4, pal 1
    mem.0[0x3102] = 0x40;
    mem.0[0x3103] = 10;
    mem.0[0x3104] = 0x00;
    mem.0[0x4000] = 0xFF;
    mem.0[0x4001] = 0xFF;
    mem.0[0x4002] = 0xFF;
    mem.0[0x4003] = 0xFF;
    m.startdma(&mut mem, 16, 263);
    // cycles = 44 -> (44*15)/428 = 1 -> 0x41, width 4.
    assert_eq!(m.dev_overlay(), (0x41, 4));
    let p = m.render_pixel(0);
    assert!(p.dev_overlay && p.left == 0x41 && p.right == 0x41);
    assert!(!m.render_pixel(4).dev_overlay);
}

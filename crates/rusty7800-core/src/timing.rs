//! Rusty7800 `timing` — machine variants, screen timing, BIOS boot.
//!
//! Phase 6 port of the `a7800-5.2/src/mame/drivers/a7800.cpp` machine
//! layer (BSD-3-Clause): the 8 `CONS` machine configs (clocks, line
//! counts, `m_ispal`, devmode flag, softlist filter), the 6
//! `PALETTE_INIT`s + per-machine selector, `MACHINE_CONFIG` screen
//! params, the scanline `interrupt` schedule (DMA +7 cycles, pixels
//! +33.5), `machine_reset` BIOS-detect, and `bios_or_cart_r` routing.
//! Silent empty-boot (README: no noise with no ROM loaded) is exposed as
//! a mute flag the audio backend honors.

use crate::palette::{self, Palette};
use crate::specs::{self, MachineVariant};

/// Scanline interrupt schedule (`interrupt` in a7800.cpp): MARIA DMA
/// starts 7 CPU cycles after hblank; visible pixels begin 33.5 cycles
/// after the interrupt.
pub const DMA_DELAY_CYCLES: f64 = 7.0;
pub const VISIBLE_DELAY_CYCLES: f64 = 33.5;

/// Screen raw params (shared NTSC/PAL except line count): pixel clock =
/// SALLY clock × 4, 454 clocks/line, visible window 0–320.
pub const PIXEL_CLOCK_MUL: u32 = 4;
pub const HTOTAL: u32 = 454;
pub const H_VISIBLE_END: u32 = 320;
pub const V_VISIBLE_START: u32 = 16;

/// Screen default position (`MCFG_SCREEN_DEFAULT_POSITION(1,0,1,0)`).
pub const SCREEN_DEFAULT_POSITION: (f64, f64, f64, f64) = (1.0, 0.0, 1.0, 0.0);

/// One of the 8 machine variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineId {
    Ntsc,
    NtscCool,
    NtscHot,
    NtscDev,
    Pal,
    PalCool,
    PalHot,
    PalDev,
}

impl MachineId {
    pub fn all() -> [MachineId; 8] {
        use MachineId::*;
        [Ntsc, NtscCool, NtscHot, NtscDev, Pal, PalCool, PalHot, PalDev]
    }

    /// MAME driver name (`CONS` line tag).
    pub fn driver_name(self) -> &'static str {
        match self {
            MachineId::Ntsc => "a7800",
            MachineId::NtscCool => "a7800u1",
            MachineId::NtscHot => "a7800u2",
            MachineId::NtscDev => "a7800dev",
            MachineId::Pal => "a7800p",
            MachineId::PalCool => "a7800pu1",
            MachineId::PalHot => "a7800pu2",
            MachineId::PalDev => "a7800pdev",
        }
    }

    pub fn spec(self) -> &'static MachineVariant {
        specs::MACHINES
            .iter()
            .find(|m| m.name == self.driver_name())
            .expect("machine spec table covers all driver names")
    }

    /// SALLY clock in Hz.
    pub fn clock_hz(self) -> u32 {
        if self.spec().is_pal {
            specs::CLK_PAL
        } else {
            specs::CLK_NTSC
        }
    }

    /// Softlist filter (`MCFG_SOFTWARE_LIST_FILTER`): NTSC vs PAL list.
    pub fn softlist_filter(self) -> &'static str {
        if self.spec().is_pal {
            "PAL"
        } else {
            "NTSC"
        }
    }

    /// The literal 256-pen palette for this machine. Dev machines reuse
    /// the base Warm palettes (their DMA overlay comes from MARIA state,
    /// not a separate table).
    pub fn palette(self) -> &'static Palette {
        let want = self.spec().palette_cpp;
        let idx = palette::ALL_PALETTES
            .iter()
            .position(|(name, _)| *name == want)
            .expect("palette table covers all machine selectors");
        &palette::ALL_PALETTES[idx].1
    }
}

/// Screen geometry derived from the `MCFG_SCREEN_RAW_PARAMS` rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenGeometry {
    pub lines: u32,
    pub visible_lines: u32,
}

impl ScreenGeometry {
    pub fn for_machine(id: MachineId) -> Self {
        let spec = id.spec();
        ScreenGeometry {
            lines: spec.lines,
            visible_lines: if spec.is_pal {
                specs::VISIBLE_LINES_PAL
            } else {
                specs::VISIBLE_LINES_NTSC
            },
        }
    }

    /// First and one-past-last visible raster lines (16..16+243/293).
    pub fn visible_range(self) -> (u32, u32) {
        (V_VISIBLE_START, V_VISIBLE_START + self.visible_lines)
    }

    pub fn is_visible_line(self, line: u32) -> bool {
        let (first, end) = self.visible_range();
        (first..end).contains(&line)
    }

    /// Scanline rate in Hz: pixel clock / clocks-per-line.
    pub fn line_rate_hz(self, clock_hz: u32) -> f64 {
        clock_hz as f64 * PIXEL_CLOCK_MUL as f64 / HTOTAL as f64
    }

    /// Frame rate in Hz: line rate / lines per frame.
    pub fn frame_rate_hz(self, clock_hz: u32) -> f64 {
        self.line_rate_hz(clock_hz) / self.lines as f64
    }
}

/// 32-bit FNV-1a checksum of a palette table (per-table identity for the
/// Phase 6 acceptance "palette CRC per table" — detects swaps/corruption;
/// values pinned in `tests/phase6_timing.rs`).
pub fn palette_checksum(pal: &Palette) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for (r, g, b) in pal.iter() {
        for byte in [*r, *g, *b] {
            h ^= byte as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
    }
    h
}

/// BIOS region image: 16K (`ROM_REGION(0x4000, "maincpu", ERASEFF)`)
/// backing `$C000–$FFFF` for `bios_or_cart_r`. Absent bytes stay `0xFF`.
#[derive(Clone, Debug)]
pub struct BiosImage {
    /// `true` once any BIOS file was placed (drives mute + boot logic).
    pub present: bool,
    pub data: [u8; 0x4000],
}

impl Default for BiosImage {
    fn default() -> Self {
        BiosImage { present: false, data: [0xFF; 0x4000] }
    }
}

impl BiosImage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Place a BIOS file at its `ROM_LOAD` offset. NTSC `7800.u7` is 4K
    /// at `0x3000`; the prototype `c300558-001a.u7` and `7800pal.rom`
    /// are full 16K at `0x0000`. Oversized files truncate; the region is
    /// `ERASEFF` so unmapped bytes read `0xFF`.
    pub fn place(&mut self, file: &[u8], offset: usize) {
        let n = file.len().min(self.data.len().saturating_sub(offset));
        self.data[offset..offset + n].copy_from_slice(&file[..n]);
        self.present = true;
    }

    /// `$C000–$FFFF` read: BIOS bytes while `INPTCTRL` bit 2
    /// (`m_ctrl_reg & 0x04`) is clear, else the cart (`bios_or_cart_r`).
    pub fn read(&self, addr: u16, ctrl_reg: u8, cart_byte: u8) -> u8 {
        if ctrl_reg & 0x04 == 0 {
            self.data[addr.wrapping_sub(0xC000) as usize]
        } else {
            cart_byte
        }
    }
}

/// `machine_reset` control-register init: all state zeroed, then flipped
/// to cart (`m_ctrl_reg = 4`) when the boot vectors are blank (`$FFFE`
/// and `$FFFF` both `0xFF` — no BIOS present at the top of memory).
pub fn reset_ctrl_reg(top_fffe: u8, top_ffff: u8) -> u8 {
    if top_fffe == 0xFF && top_ffff == 0xFF {
        0x04
    } else {
        0x00
    }
}

/// Silent empty-boot (README: "no ROM loaded" is silent, booting to
/// black/background instead of noise). The audio backend mutes unless a
/// cartridge or BIOS is present.
pub fn audio_muted(has_cart: bool, bios_present: bool) -> bool {
    !(has_cart || bios_present)
}

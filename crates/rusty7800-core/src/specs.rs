//! Rusty7800 Phase 0 — spec constants transcribed from A7800 5.2 sources.
//!
//! Every constant cites its source file + line context. Do not change values
//! without citing the corresponding upstream change.

use core::ops::RangeInclusive;

// ---------------------------------------------------------------------------
// Clocks (a7800.cpp: `#define CLK_NTSC 1789772` / `#define CLK_PAL 1773447`)
// SALLY drops to ~1.19 MHz equivalent on TIA/RIOT-RAM access via stall
// cycles, not a clock change (MACHINE_CONFIG comment, a7800_ntsc).
// ---------------------------------------------------------------------------
/// SALLY clock, NTSC machines (Hz).
pub const CLK_NTSC: u32 = 1_789_772;
/// SALLY clock, PAL machines (Hz).
pub const CLK_PAL: u32 = 1_773_447;

// ---------------------------------------------------------------------------
// Screen timing (a7800.cpp: MCFG_SCREEN_RAW_PARAMS + comments)
// ---------------------------------------------------------------------------
/// Master pixel clock multiplier (RAW_PARAMS first arg = CLK*4).
pub const PIXEL_CLOCK_MUL: u32 = 4;
/// Total pixels per scanline (NTSC + PAL).
pub const HTOTAL: u32 = 454;
/// Visible pixel window start/end (both regions).
pub const H_VISIBLE: RangeInclusive<u32> = 0..=320;
/// Total scanlines: NTSC.
pub const LINES_NTSC: u32 = 263;
/// Total scanlines: PAL.
pub const LINES_PAL: u32 = 313;
/// Visible-line window start (both regions).
pub const V_VISIBLE_START: u32 = 16;
/// Max possible visible lines, NTSC (16 + 243).
pub const VISIBLE_LINES_NTSC: u32 = 243;
/// Max possible visible lines, PAL (16 + 293).
pub const VISIBLE_LINES_PAL: u32 = 293;
/// MARIA framebuffer width in pixels.
pub const FRAME_W: usize = 320;

// ---------------------------------------------------------------------------
// Address map (a7800.cpp: `ADDRESS_MAP_START(a7800_mem, …)`)
// ---------------------------------------------------------------------------
/// 0x0000–0x001F, mirror 0x300: TIA.
pub const MAP_TIA: RangeInclusive<u16> = 0x0000..=0x001F;
pub const MAP_TIA_MIRROR: u16 = 0x300;
/// 0x0020–0x003F, mirror 0x300: MARIA.
pub const MAP_MARIA: RangeInclusive<u16> = 0x0020..=0x003F;
pub const MAP_MARIA_MIRROR: u16 = 0x300;
/// 0x0040–0x00FF: zero-page mirror bank -> 0x2040 base (6116_2 + 0x40).
pub const MAP_ZPMIRROR: RangeInclusive<u16> = 0x0040..=0x00FF;
/// 0x0140–0x01FF: stack-page mirror bank -> 0x2140 base (6116_2 + 0x140).
pub const MAP_SPMIRROR: RangeInclusive<u16> = 0x0140..=0x01FF;
/// 0x0280–0x029F, mirror 0x160: RIOT 6532.
pub const MAP_RIOT: RangeInclusive<u16> = 0x0280..=0x029F;
pub const MAP_RIOT_MIRROR: u16 = 0x160;
/// 0x0480–0x04FF, mirror 0x100: RIOT RAM (128 B).
pub const MAP_RIOT_RAM: RangeInclusive<u16> = 0x0480..=0x04FF;
pub const MAP_RIOT_RAM_MIRROR: u16 = 0x100;
/// 0x1800–0x1FFF: 6116_1 SRAM (2 KiB).
pub const MAP_SRAM1: RangeInclusive<u16> = 0x1800..=0x1FFF;
/// 0x2000–0x27FF: 6116_2 SRAM (4 KiB; hosts zp/sp mirrors).
pub const MAP_SRAM2: RangeInclusive<u16> = 0x2000..=0x27FF;
/// 0x4000–0xFFFF cart writes; 0x4000–0xBFFF cart reads; 0xC000–0xFFFF BIOS/cart.
pub const MAP_CART_WRITE: RangeInclusive<u16> = 0x4000..=0xFFFF;
pub const MAP_CART_READ_LO: RangeInclusive<u16> = 0x4000..=0xBFFF;
pub const MAP_BIOS_CART_READ_HI: RangeInclusive<u16> = 0xC000..=0xFFFF;

// Dynamic overlays installed by machine_start() per cart type (a7800.cpp):
/// P450 family POKEY/RAM regs 0x0400–0x04FF.
pub const OV_P450: RangeInclusive<u16> = 0x0400..=0x04FF;
/// P800 family POKEY 0x0800–0x0FFF.
pub const OV_P800: RangeInclusive<u16> = 0x0800..=0x0FFF;
/// XM POKEY/RAM/YM regs 0x0400–0x047F.
pub const OV_XM_REGS: RangeInclusive<u16> = 0x0400..=0x047F;
/// HSC/XM NVRAM+ROM windows 0x1000–0x17FF and 0x3000–0x3FFF.
pub const OV_HSC_LO: RangeInclusive<u16> = 0x1000..=0x17FF;
pub const OV_HSC_HI: RangeInclusive<u16> = 0x3000..=0x3FFF;
/// Bankset-52K ROM window 0x3000–0x3FFF (read).
pub const OV_BANKSET52K: RangeInclusive<u16> = 0x3000..=0x3FFF;
/// Bankset-P4000 extra write window 0x4000–0x7FFF.
pub const OV_BANKSET_P4000_WR: RangeInclusive<u16> = 0x4000..=0x7FFF;

// ---------------------------------------------------------------------------
// Audio (a7800.cpp: MCFG_SOUND_TIA_ADD("tia", 31400) + mono speaker)
// ---------------------------------------------------------------------------
/// TIA sample rate used by the driver.
pub const TIA_SAMPLE_RATE: u32 = 31_400;

// ---------------------------------------------------------------------------
// MARIA modes + hole logic (maria.cpp / maria.h)
// ---------------------------------------------------------------------------
/// MARIA graphics modes (write_mode values in DL byte 1).
pub const MARIA_MODES: [&str; 7] = [
    "160A", "160B", "320A", "320B", "320C", "320D", "Kangaroo",
];
/// Holey mask: (m_holey & 0x02) && (addr & 0x9000) == 0x9000 (maria.cpp is_holey).
pub const HOLEY_MASK_A: u16 = 0x9000;
/// Holey mask: (m_holey & 0x01) && (addr & 0x8800) == 0x8800.
pub const HOLEY_MASK_B: u16 = 0x8800;
/// Line-RAM working width in low-res pixels.
pub const MARIA_LINE_W: usize = 160;

// ---------------------------------------------------------------------------
// Controllers (ctrl.cpp: SLOT_INTERFACE_START(a7800_control_port_devices))
// ---------------------------------------------------------------------------
/// (slot name, device) for both control ports; default = proline_joystick.
/// Slots 1–9 mirror `SLOT_INTERFACE_START(a7800_control_port_devices)`;
/// `atarivox`/`savekey` are new (no MAME slot exists) and follow Stella's
/// controller-type naming.
pub const CONTROL_PORT_DEVICES: [(&str, &str); 11] = [
    ("proline_joystick", "A7800_JOYSTICK_PROLINE"),
    ("vcs_joystick", "A7800_JOYSTICK"),
    ("paddle", "A7800_PADDLES"),
    ("lightgun", "A7800_LIGHTGUN"),
    ("driving_wheel", "A7800_WHEEL"),
    ("cx22_trakball", "A7800_TRACKBALL"),
    ("keypad", "A7800_KEYPAD"),
    ("amiga_mouse", "A7800_AMIGAMOUSE"),
    ("st_mouse", "A7800_STMOUSE"),
    ("atarivox", "A7800_ATARIVOX"),
    ("savekey", "A7800_SAVEKEY"),
];
/// Default device for joy1/joy2 (MACHINE_CONFIG).
pub const DEFAULT_PORT_DEVICE: &str = "proline_joystick";

// ---------------------------------------------------------------------------
// Cartridge slots (a78_carts.h SLOT_INTERFACE_START(a7800_cart))
// ---------------------------------------------------------------------------
/// All cart slot keys in SLOT_INTERFACE order (a78_carts.h:35 entries).
pub const CART_SLOTS: [&str; 35] = [
    "a78_rom",
    "a78_pokey",
    "a78_sg",
    "a78_sg_pokey",
    "a78_sg_ram",
    "a78_sg9",
    "a78_mram",
    "a78_abs",
    "a78_act",
    "a78_hsc",
    "a78_xm",
    "a78_bankset_sg",
    "a78_bankset_sg_bankram",
    "a78_bankset",
    "a78_bankset_bankram",
    "a78_bankset_52k",
    "a78_megacart",
    "a78_versa",
    "a78_p450_t0",
    "a78_p450_t1",
    "a78_p450_t2",
    "a78_p450_t6",
    "a78_p450_ta",
    "a78_p450_vb",
    "a78_p800_t0",
    "a78_p800_t1",
    "a78_p800_t2",
    "a78_p800_t6",
    "a78_p800_ta",
    "a78_bankset_p800",
    "a78_bankset_sg_p800",
    "a78_bankset_sg_bankram_p800",
    "a78_bankset_bankram_p800",
    "a78_bankset_52k_p4000",
    "a78_bankset_p4000",
];

// ---------------------------------------------------------------------------
// Machines (a7800.cpp: CONS lines + DRIVER_INITs)
// ---------------------------------------------------------------------------
/// Machine variant descriptor.
pub struct MachineVariant {
    pub name: &'static str,
    pub full_name: &'static str,
    pub is_pal: bool,
    pub lines: u32,
    pub devmode: bool,
    pub palette_cpp: &'static str,
}

/// All 8 machine variants in CONS order.
pub const MACHINES: [MachineVariant; 8] = [
    MachineVariant { name: "a7800",    full_name: "Atari 7800 NTSC Warm",           is_pal: false, lines: 263, devmode: false, palette_cpp: "a7800_palette" },
    MachineVariant { name: "a7800u1",  full_name: "Atari 7800 NTSC Cool",           is_pal: false, lines: 263, devmode: false, palette_cpp: "a7800u1_palette" },
    MachineVariant { name: "a7800u2",  full_name: "Atari 7800 NTSC Hot",            is_pal: false, lines: 263, devmode: false, palette_cpp: "a7800u2_palette" },
    MachineVariant { name: "a7800dev", full_name: "Atari 7800 NTSC Developer Mode", is_pal: false, lines: 263, devmode: true,  palette_cpp: "a7800_palette" },
    MachineVariant { name: "a7800p",   full_name: "Atari 7800 PAL Warm",            is_pal: true,  lines: 313, devmode: false, palette_cpp: "a7800p_palette" },
    MachineVariant { name: "a7800pu1", full_name: "Atari 7800 PAL Cool",            is_pal: true,  lines: 313, devmode: false, palette_cpp: "a7800pu1_palette" },
    MachineVariant { name: "a7800pu2", full_name: "Atari 7800 PAL Hot",             is_pal: true,  lines: 313, devmode: false, palette_cpp: "a7800pu2_palette" },
    MachineVariant { name: "a7800pdev", full_name: "Atari 7800 PAL Developer Mode", is_pal: true,  lines: 313, devmode: true,  palette_cpp: "a7800p_palette" },
];

// ---------------------------------------------------------------------------
// Console inputs (a7800.cpp: INPUT_PORTS_START(a7800))
// ---------------------------------------------------------------------------
/// Console button default keys: Reset=T, Select=E, Pause=O.
pub const CONSOLE_DEFAULT_KEYS: [(&str, &str); 3] = [
    ("Reset", "T"),
    ("Select", "E"),
    ("Pause", "O"),
];

// ---------------------------------------------------------------------------
// ROM/BIOS identity (a7800.cpp: ROM_START blocks)
// ---------------------------------------------------------------------------
/// NTSC BIOS 7800.u7 @ 0x3000 len 0x1000 (optional).
pub const BIOS_NTSC_NAME: &str = "7800.u7";
pub const BIOS_NTSC_CRC: &str = "5d13730c";
pub const BIOS_NTSC_SHA1: &str = "d9d134bb6b36907c615a594cc7688f7bfcef5b43";
/// Prototype BIOS c300558-001a.u7 full 0x4000 (optional).
pub const BIOS_PROTO_CRC: &str = "a0e10edf";
/// PAL BIOS 7800pal.rom full 0x4000 (optional).
pub const BIOS_PAL_NAME: &str = "7800pal.rom";
pub const BIOS_PAL_CRC: &str = "d5b61170";
/// High-score cart ROM hash (hash/a7800.xml).
pub const HSC_ROM_CRC: &str = "9be408d3";

// ---------------------------------------------------------------------------
// Cart hardware sizes (hash/a7800.xml + bus sources)
// ---------------------------------------------------------------------------
/// .a78 header size (a78_slot.cpp call_load reads 128-byte header).
pub const A78_HEADER_LEN: usize = 128;
/// HSC cart ROM / NVRAM sizes.
pub const HSC_ROM_LEN: usize = 4096;
pub const HSC_NVRAM_LEN: usize = 2048;
/// XM expansion RAM size (131072).
pub const XM_RAM_LEN: usize = 131_072;
/// POKEY base addresses by family.
pub const POKEY_BASE_STD: u16 = 0x4000;
pub const POKEY_BASE_XM: u16 = 0x0400;
pub const POKEY_BASE_P450: u16 = 0x0450;
pub const POKEY_BASE_P800: u16 = 0x0800;
/// POKEY @4000 header flag bits (a78_slot.cpp A78_POKEY0450/0800).
pub const A78_HEAD_POKEY0450: u16 = 0x40;
pub const A78_HEAD_POKEY0800: u16 = 0x80;
/// Paddle discharge scaler (driver tia_r: elapsed = cycles/97).
pub const PADDLE_CYCLE_DIVISOR: u64 = 97;

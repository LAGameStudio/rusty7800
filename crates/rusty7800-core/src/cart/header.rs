//! `.a78` header handling: parse, quirk repair, mapper dispatch.
//!
//! Phase 5 port of `a78_cart_slot_device::verify_header`,
//! `validate_header`, `call_load`/`get_default_card_software` dispatch,
//! and `internal_header_logging` field layout in
//! `a7800-5.2/src/devices/bus/a7800/a78_slot.cpp` (BSD-3-Clause).
//!
//! Header layout (bytes): 0 = version, 1..16 = `"ATARI7800     "`,
//! 17..48 = title (32), 49..52 = data length (big-endian u32), 53..54 =
//! cart type (big-endian u16), 55/56 = controller types, 57 = TV system
//! (0 NTSC / 1 PAL), 58 = save type (v2: 0 none, 1 HSC, 2 SaveKey),
//! 63 = XM expansion required.
//!
//! Cart-type low byte flags (byte 54): bit 0 POKEY@4000, 1 SuperCart,
//! 2 SC RAM@4000, 3 bank0@4000, 4 bank6@4000, 5 banked RAM, 6 POKEY@0450,
//! 7 mirror RAM. High byte (byte 53): 0x01 = Activision, 0x02 = Absolute.

use super::pcb;

/// Header size in bytes (`unhashed_header_length() == 128`).
pub const HEADER_LEN: usize = 128;
/// Magic text at bytes 1..10 (`strncmp(magic, header + 1, 9)`).
pub const MAGIC: &[u8; 9] = b"ATARI7800";

/// Parsed `.a78` header fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct A78Header {
    /// Data length from bytes 49..52 (big-endian).
    pub len: u32,
    /// Raw mapper from bytes 53..54 (big-endian), before quirk repair.
    pub mapper: u16,
    /// Controller types (bytes 55/56): 0 none, 1 joystick, 2 light gun.
    pub ctrl1: u8,
    pub ctrl2: u8,
    /// TV system (byte 57): false NTSC, true PAL.
    pub is_pal: bool,
    /// Save type (byte 58): 0 none, 1 HSC, 2 SaveKey.
    pub save: u8,
    /// XM expansion required (byte 63 nonzero).
    pub needs_xm: bool,
    /// Cart title (bytes 17..49, not trimmed — matches `%.32s` logging).
    pub title: [u8; 32],
}

/// Header verification outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderError {
    TooShort,
    BadMagic,
}

/// Parse and verify a 128-byte header (`verify_header` + field layout).
pub fn parse_header(head: &[u8]) -> Result<A78Header, HeaderError> {
    if head.len() < HEADER_LEN {
        return Err(HeaderError::TooShort);
    }
    if &head[1..10] != MAGIC {
        return Err(HeaderError::BadMagic);
    }
    let len =
        ((head[49] as u32) << 24) | ((head[50] as u32) << 16) | ((head[51] as u32) << 8) | head[52] as u32;
    let mapper = ((head[53] as u16) << 8) | head[54] as u16;
    let mut title = [0u8; 32];
    title.copy_from_slice(&head[0x11..0x31]);
    Ok(A78Header {
        len,
        mapper,
        ctrl1: head[55],
        ctrl2: head[56],
        is_pal: head[57] != 0,
        save: head[58],
        needs_xm: head[63] != 0,
        title,
    })
}

/// A single `validate_header` quirk correction, for logging/tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quirk {
    /// `(head & 0x3d)` arm: POKEY/RAM/bank0/bank6 repair (clears one bit).
    MaskRepair { masked: u16, cleared: u16 },
    /// Activision (`0x100`) / Absolute (`0x200`) bankswitch bits dropped.
    SpecialMasked { kept: u16 },
}

/// The ten `(head & 0x3d)` repair arms: matched value → bit cleared.
const MASK_ARMS: [(u16, u16); 10] = [
    (0x05, 0x01),
    (0x09, 0x01),
    (0x11, 0x01),
    (0x21, 0x01),
    (0x0C, 0x04),
    (0x14, 0x04),
    (0x24, 0x04),
    (0x18, 0x08),
    (0x28, 0x08),
    (0x30, 0x10),
];

/// Quirk repair (`validate_header`). Returns the corrected mapper plus
/// the list of applied corrections. The two commented-out C++ branches
/// (missing-SuperCart-bit repair, `>0x2000` unsupported-mapper mask) are
/// disabled upstream and stay disabled here.
pub fn validate_header(head: u16) -> (u16, Vec<Quirk>) {
    let mut quirks = Vec::new();
    let mut mapper = head;
    if let Some(&(_, clear)) = MASK_ARMS.iter().find(|(v, _)| mapper & 0x3d == *v) {
        mapper &= !clear;
        quirks.push(Quirk::MaskRepair { masked: mapper & 0x3d, cleared: clear });
    }
    if (mapper & 0xff00) == 0x100 && (mapper & 0xff) != 0 {
        mapper &= 0xff00;
        quirks.push(Quirk::SpecialMasked { kept: 0x0100 });
    }
    if (mapper & 0xff00) == 0x200 && (mapper & 0xff) != 0 {
        mapper &= 0xff00;
        quirks.push(Quirk::SpecialMasked { kept: 0x0200 });
    }
    (mapper, quirks)
}

/// Raw PCB id dispatch (`call_load` / `get_default_card_software` switch
/// on `mapper & 0xe02e`, then the POKEY@0450 add and special overrides).
/// `rom_len` is the image length; `file_len` the whole file length (the
/// default-slot path compares those instead — both are the ROM length for
/// headered images modulo the 128-byte header, which never crosses the
/// `0x40000`/`0x1A000` thresholds differently... except exactly at the
/// boundary: preserved as written, callers pass the C++-matching value).
pub fn dispatch_type(mapper: u16, rom_len: usize, file_len: usize) -> i32 {
    let ty = match mapper & 0xe02e {
        0x0000 => {
            if mapper & 0x01 != 0 {
                pcb::TYPE1
            } else {
                pcb::TYPE0
            }
        }
        0x0002 => {
            if mapper & 0x01 != 0 {
                pcb::TYPE3
            } else {
                pcb::TYPE2
            }
        }
        0x0006 => pcb::TYPE6,
        0x000a => pcb::TYPEA,
        0x0022 | 0x0026 => {
            if file_len > 0x40000 {
                pcb::MEGACART
            } else {
                pcb::VERSABOARD
            }
        }
        0x2000 => {
            if mapper & 0x01 != 0 {
                if rom_len >= 0x1A000 {
                    pcb::BANKSET_52K_POK4000
                } else {
                    pcb::BANKSET_POK4000
                }
            } else if rom_len >= 0x1A000 {
                pcb::BANKSET_52K
            } else {
                pcb::BANKSET
            }
        }
        0x6000 => pcb::BANKSET_BANKRAM,
        0xE000 => pcb::BANKSET_BANKRAM_POK800,
        0x2002 => pcb::BANKSET_SG,
        0x6002 => pcb::BANKSET_SG_BANKRAM,
        0xA002 => pcb::BANKSET_SG_POK800,
        0xE002 => pcb::BANKSET_SG_BANKRAM_POK800,
        0x8000 => {
            if mapper & 0x01 != 0 {
                pcb::TYPE1_POK800
            } else {
                pcb::TYPE0_POK800
            }
        }
        0x8002 => pcb::TYPE2_POK800,
        0x8006 => pcb::TYPE6_POK800,
        0x800a => pcb::TYPEA_POK800,
        _ => pcb::TYPE0,
    };
    // Note: unmatched switch arms fall through with type untouched; in
    // `call_load` that means whatever `m_type` held before (0 = TYPE0 at
    // power-on... in practice device_start zeroes it). Default TYPE0 here.
    let mut ty = ty;
    if mapper & 0x40 != 0 && mapper & 0x2000 == 0 {
        ty += pcb::POKEY0450;
    }
    if (mapper & 0xff00) == 0x0100 {
        ty = pcb::ACTIVISION;
    } else if (mapper & 0xff00) == 0x0200 {
        ty = pcb::ABSOLUTE;
    } else if mapper & 0x0080 == 0x0080 {
        ty = pcb::TYPE8;
    }
    ty
}

/// On-cart RAM size selected by `call_load` per cart type.
pub fn ram_size_for_type(ty: i32) -> usize {
    // NOTE: the C++ tests `A78_TYPE6_POK800` but not `A78_TYPE6_POK450`
    // (likely an upstream oversight — kept as written for parity).
    if ty == pcb::TYPE6 || ty == pcb::TYPE8 || ty == pcb::TYPE6_POK800 {
        0x4000
    } else if ty == pcb::MEGACART
        || ty == pcb::BANKSET_SG_BANKRAM
        || ty == pcb::BANKSET_SG_BANKRAM_POK800
        || ty == pcb::BANKSET_BANKRAM
        || ty == pcb::BANKSET_BANKRAM_POK800
        || (pcb::VERSABOARD..=pcb::VERSA_POK450).contains(&ty)
    {
        0x8000
    } else if ty == pcb::XM_BOARD {
        0x20000
    } else {
        0
    }
}

/// NVRAM size selected by `call_load` (HSC/XM get `0x800` + `0xFF` fill).
pub fn nvram_size_for_type(ty: i32) -> usize {
    if ty == pcb::HSC || ty == pcb::XM_BOARD {
        0x800
    } else {
        0
    }
}

/// Effective ROM length: header length clamped to the file, like
/// `call_load` (`len + 128 > length()` → `len = length() - 128`).
/// `total` is the whole file length including the 128-byte header.
pub fn effective_rom_len(claimed: u32, total: usize) -> usize {
    let claimed = claimed as usize;
    if claimed + HEADER_LEN > total {
        total.saturating_sub(HEADER_LEN)
    } else {
        claimed
    }
}

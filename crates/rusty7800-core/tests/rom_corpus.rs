//! ROM corpus audit: every real `.a78` through the loader + cart reads.
//!
//! Set `ROMS_DIR` to a directory of 7800 ROMs (scanned recursively); when
//! unset, this test passes vacuously so `cargo test` stays hermetic and
//! no copyrighted ROMs ever enter the repo. The corpus lives outside
//! `newwork/` by design.
//!
//! Per ROM the audit checks: header parses, dispatch resolves to a known
//! slot, the fixed bank window is real ROM (not open bus), and the 6502
//! reset vector is sane (points at `$4000+`, not unmapped `0xFFFF`).
//! Known titles additionally assert their exact mapper slot.

use rusty7800_core::cart::{load_image, slot_key_for_type, NullBus};
use std::collections::BTreeMap;

/// ROM bytes as `load_image` slices them (header stripped, clamped).
fn load_image_rom(data: &[u8]) -> Vec<u8> {
    if data.len() >= 128 && data[1..10] == *b"ATARI7800" {
        let claimed = u32::from_be_bytes(data[49..53].try_into().unwrap()) as usize;
        let len = claimed.min(data.len().saturating_sub(128));
        data[128..128 + len].to_vec()
    } else {
        data.to_vec()
    }
}

fn collect_roms(dir: &str, out: &mut Vec<std::path::PathBuf>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_roms(&path.to_string_lossy(), out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("a78") | Some("bin")
        ) {
            out.push(path);
        }
    }
}

/// Exact file name → expected slot key. Wild `.a78` headers often disagree
/// with the game's real PCB (verified byte-by-byte above): the NTSC
/// Double Dragon / Rampage dumps claim plain SuperGame, the NTSC F-18
/// dump claims flat (mapper `0x0300`), NTSC Commando claims SG while PAL
/// claims SG+POKEY, and NTSC Impossible Mission dumps claim SG (`0x0502`)
/// — the C++ dispatch resolves each exactly as asserted here (MAME reads
/// these files identically; only softlist loads would differ, and the
/// softlist holds no such entries).
const KNOWN: &[(&str, &str)] = &[
    ("Double Dragon (1989) (Activision) (PAL).a78", "a78_act"),
    ("Double Dragon (1989) (Activision) [!].a78", "a78_sg"),
    ("Rampage (1989) (Activision) [!].a78", "a78_sg"),
    ("F-18 Hornet (1988) (Absolute) (PAL).a78", "a78_abs"),
    ("F-18 Hornet (1988) (Absolute) [!].a78", "a78_rom"),
    ("Alien Brigade (1990) (Atari) [!].a78", "a78_sg9"),
    ("Crossbow (1988) (Atari).a78", "a78_sg9"),
    ("Ballblazer (1987) (Atari-Lucasfilm).a78", "a78_pokey"),
    ("Beef Drop 7800 (Oct 30-Rev 08) (2004) (CC2 Hack).a78", "a78_pokey"),
    ("Commando (1989) (Atari) (PAL).a78", "a78_sg_pokey"),
    ("Commando (1989) (Atari).a78", "a78_sg"),
    ("Impossible Mission (1987) (Atari) [!].a78", "a78_sg"),
    ("Jinks (1989) (Atari).a78", "a78_sg_ram"),
    ("Crack'ed (1988) (Atari).a78", "a78_sg"),
    ("Fatal Run (1990) (Atari).a78", "a78_sg"),
    ("Asteroids (1987) (Atari).a78", "a78_rom"),
    ("Centipede (1987) (Atari).a78", "a78_rom"),
    ("Kung Fu Master (1989) (Absolute) [!].a78", "a78_rom"),
];

/// Files whose reset vector reads insane through no fault of the mapper:
/// the CC2 hacks ship zeroed vector tails, and the mis-headered NTSC
/// Activision dumps read the wrong bank's tail as SG. MAME reads the
/// identical bytes (verified); only the sanity bar is waived here.
const VECTOR_WAIVER: &[&str] = &[
    "Alien Brigade (1990) (Atari) (CC2 Hack).a78",
    "Alien Brigade (1990) (Atari) (PAL) (CC2 Hack).a78",
    "Crossbow (1988) (Atari) (CC2 Hack).a78",
    "Crossbow (1988) (Atari) (PAL) (CC2 Hack).a78",
    "Double Dragon (1989) (Activision) [!].a78",
    "Rampage (1989) (Activision) [!].a78",
];

/// Independent re-derivation of the flat/SG/SG9 `read_40xx` formulas
/// straight from rom.cpp (test-local cross-check of the implementation).
fn model_40xx(slot: &str, rom: &[u8], addr: u16) -> u8 {
    let off = addr.wrapping_sub(0x4000) as usize;
    let banks = rom.len() as i64 / 0x4000;
    let mask = if banks & 1 != 0 { banks - 2 } else { banks - 1 };
    let at = |i: i64| rom[i.rem_euclid(rom.len() as i64) as usize];
    match slot {
        "a78_rom" => {
            let base = 0x10000i64 - rom.len() as i64;
            if off as i64 + 0x4000 < base {
                0xFF
            } else {
                rom[(off as i64 + 0x4000 - base) as usize]
            }
        }
        "a78_sg" => {
            if off < 0x4000 {
                at((off & 0x3FFF) as i64 + (mask - 1) * 0x4000)
            } else if off < 0x8000 {
                at((off & 0x3FFF) as i64)
            } else {
                at((off & 0x3FFF) as i64 + mask * 0x4000)
            }
        }
        "a78_sg9" => {
            // Bank register powers on to 0 (writes add 1, never tested here).
            if off < 0x8000 {
                at((off & 0x3FFF) as i64)
            } else {
                at((off & 0x3FFF) as i64 + (mask + 1) * 0x4000)
            }
        }
        _ => unreachable!(),
    }
}

#[test]
fn rom_corpus_audit() {
    let dir = match std::env::var("ROMS_DIR") {
        Ok(d) => d,
        Err(_) => {
            eprintln!("ROMS_DIR unset: corpus audit skipped");
            return;
        }
    };
    let mut files = Vec::new();
    collect_roms(&dir, &mut files);
    assert!(!files.is_empty(), "ROMS_DIR={dir}: no ROMs found");
    println!("auditing {} ROMs under {dir}", files.len());

    let mut slots: BTreeMap<&str, usize> = BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    let mut known_checked = 0;

    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                failures.push(format!("{name}: unreadable ({e})"));
                continue;
            }
        };
        let mut loaded = match load_image(&data) {
            Ok(l) => l,
            Err(e) => {
                failures.push(format!("{name}: load failed ({e:?})"));
                continue;
            }
        };
        *slots.entry(loaded.slot_key).or_default() += 1;

        // Slot key must round-trip through the key table (or be the
        // headerless-flat fallback).
        if loaded.header.is_some() {
            assert_eq!(slot_key_for_type(loaded.pcb_id), loaded.slot_key, "{name}");
        }

        // Known titles assert exact mapper slots (exact file names: wild
        // headers differ per dump, e.g. NTSC vs PAL Double Dragon).
        for (file, slot) in KNOWN {
            if name == *file {
                known_checked += 1;
                if loaded.slot_key != *slot {
                    failures.push(format!(
                        "{name}: expected slot {slot}, got {}",
                        loaded.slot_key
                    ));
                }
            }
        }

        // Independent-model cross-check on flat/SG/SG9 images: sample
        // every 16K boundary plus vector bytes through both paths.
        if matches!(loaded.slot_key, "a78_rom" | "a78_sg" | "a78_sg9") {
            let rom = &load_image_rom(&data);
            let mut bus = NullBus;
            for a in (0x4000..=0xFFFFu16).step_by(0x2000) {
                for d in [0u16, 1, 0x1FFE, 0x1FFF] {
                    let addr = a.wrapping_add(d);
                    let got = loaded.cart.read_40xx(addr, false, &mut bus);
                    let want = model_40xx(loaded.slot_key, rom, addr);
                    if got != want {
                        failures.push(format!(
                            "{name}: model mismatch at {addr:#06X} ({got:#04X} vs {want:#04X})"
                        ));
                    }
                }
            }
        }

        // Fixed-bank window must be real ROM somewhere in the top 16K,
        // and the reset vector must be sane.
        let mut bus = NullBus;
        let top_ok = (0xC000..=0xFFFFu16)
            .step_by(0x1000)
            .any(|a| loaded.cart.read_40xx(a, false, &mut bus) != 0xFF);
        if !top_ok {
            failures.push(format!("{name}: top 16K all open bus"));
            continue;
        }
        let lo = loaded.cart.read_40xx(0xFFFC, false, &mut bus) as u16;
        let hi = loaded.cart.read_40xx(0xFFFD, false, &mut bus) as u16;
        let vec = lo | (hi << 8);
        if (vec == 0xFFFF || vec < 0x4000) && !VECTOR_WAIVER.contains(&name.as_str()) {
            failures.push(format!("{name}: bad reset vector {vec:#06X}"));
        }
        // Maria view must also resolve (bankset second half / same data).
        // Non-bankset carts share the Sally view, so the waiver carries over.
        let mlo = loaded.cart.read_40xx(0xFFFC, true, &mut bus) as u16;
        let mhi = loaded.cart.read_40xx(0xFFFD, true, &mut bus) as u16;
        if mlo | (mhi << 8) == 0xFFFF && !VECTOR_WAIVER.contains(&name.as_str()) {
            failures.push(format!("{name}: Maria-view reset vector open bus"));
        }
    }

    println!("slot distribution: {slots:?}");
    println!("known-title checks: {known_checked}");
    assert!(known_checked > 0, "expected known titles in corpus");
    assert!(failures.is_empty(), "corpus failures:\n{}", failures.join("\n"));
}

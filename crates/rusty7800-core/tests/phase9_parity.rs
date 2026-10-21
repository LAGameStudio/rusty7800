//! Phase 9 acceptance: validation & 1:1 parity evidence (hermetic).
//!
//! Plan §9 wants a corpus (≥1 ROM per mapper), replay determinism, and
//! perf numbers, with the report in `docs/PARITY.md`. No copyrighted ROMs
//! may enter the repo, and the optional C++ socket shim (§10) does not
//! exist yet — so this file builds the corpus synthetically: one minimal
//! bootable image per slot group (headered where the header dispatch can
//! reach the slot, `build_cart` where only the slot option can: HSC, XM,
//! Versa+POKEY, bankset+POKEY@800). Every image carries a spin loop and
//! vectors placed for its mapper's reset window; the harness asserts the
//! slot key, a clean boot, a pinned spin PC where the window math is
//! exact, and — for all entries — bit-identical replay across two
//! machines (frame bytes + CPU state). A scripted input replay (§5
//! replays concept) and a frame-time perf sample round out the phase.

use rusty7800_core::cart::{build_cart, load_image, LoadedCart};
use rusty7800_core::machine::Machine;
use rusty7800_core::state::fnv1a;
use rusty7800_core::timing::MachineId;

/// 128-byte `.a78` header: magic at 1..10, BE length at 49..52, BE mapper
/// at 53..54, title at 0x11... title is cosmetic here.
fn header(mapper: u16, len: u32) -> [u8; 128] {
    let mut h = [0u8; 128];
    h[1..10].copy_from_slice(b"ATARI7800");
    h[49..53].copy_from_slice(&len.to_be_bytes());
    h[53..55].copy_from_slice(&mapper.to_be_bytes());
    h[0x11..0x17].copy_from_slice(b"PARITY");
    h
}

/// One corpus entry: how to build the image and where reset should land.
/// `spin`/`vec` are image offsets; `expect_pc` pins the post-boot PC only
/// where the reset-window math is exact (otherwise `None` = fog-boot
/// tolerance: slot + determinism still asserted).
struct Entry {
    name: &'static str,
    slot: &'static str,
    via: Via,
    spin_img: usize,
    spin_cpu: u16,
    vec_img: usize,
    expect_pc: Option<u16>,
}

enum Via {
    Header {
        mapper: u16,
        rom_len: usize,
    },
    SlotKey {
        key: &'static str,
        pcb_id: i32,
        rom_len: usize,
    },
}

/// Stamp `JMP cpu` at `spin_img` and NMI/RST/IRQ vectors at `vec_img`.
fn stamp_spin(rom: &mut [u8], spin_img: usize, spin_cpu: u16, vec_img: usize) {
    rom[spin_img] = 0x4C; // JMP abs
    rom[spin_img + 1] = (spin_cpu & 0xFF) as u8;
    rom[spin_img + 2] = (spin_cpu >> 8) as u8;
    for v in [vec_img, vec_img + 2, vec_img + 4] {
        rom[v] = (spin_cpu & 0xFF) as u8;
        rom[v + 1] = (spin_cpu >> 8) as u8;
    }
}

fn corpus() -> Vec<Entry> {
    let flat = |name: &'static str, slot: &'static str, mapper: u16| Entry {
        name,
        slot,
        via: Via::Header {
            mapper,
            rom_len: 0x1000,
        },
        spin_img: 0x000,
        spin_cpu: 0xF000,
        vec_img: 0xFFA,
        expect_pc: Some(0xF000),
    };
    let sg32 = |name: &'static str, slot: &'static str, mapper: u16| Entry {
        name,
        slot,
        via: Via::Header {
            mapper,
            rom_len: 0x8000,
        },
        spin_img: 0x4000,
        spin_cpu: 0xC000,
        vec_img: 0x7FFA,
        expect_pc: Some(0xC000),
    };
    // Last-16K-at-$C000 family (SG9 144K + SuperGame-9 POKEY flavors).
    let sg9 = |name: &'static str, slot: &'static str, mapper: u16| Entry {
        name,
        slot,
        via: Via::Header {
            mapper,
            rom_len: 0x24000,
        },
        spin_img: 0x20000,
        spin_cpu: 0xC000,
        vec_img: 0x23FFA,
        expect_pc: Some(0xC000),
    };
    vec![
        flat("flat", "a78_rom", 0x0000),
        flat("pokey", "a78_pokey", 0x0001),
        sg32("sg", "a78_sg", 0x0002),
        sg32("sg_pokey", "a78_sg_pokey", 0x0003),
        sg32("sg_ram", "a78_sg_ram", 0x0006),
        sg9("sg9", "a78_sg9", 0x000A),
        // Mirror RAM 16K: $8000+ reads flat tail (base $C000).
        Entry {
            name: "mram",
            slot: "a78_mram",
            via: Via::Header {
                mapper: 0x0080,
                rom_len: 0x4000,
            },
            spin_img: 0x3000,
            spin_cpu: 0xF000,
            vec_img: 0x3FFA,
            expect_pc: Some(0xF000),
        },
        // Absolute 64K: upper 32K fixed at $8000; the window formula keeps
        // the image tail at the top ($FFFC reads img 0xFFFC).
        Entry {
            name: "abs",
            slot: "a78_abs",
            via: Via::Header {
                mapper: 0x0200,
                rom_len: 0x10000,
            },
            spin_img: 0x8000,
            spin_cpu: 0x8000,
            vec_img: 0xFFFA,
            expect_pc: Some(0x8000),
        },
        // Activision 128K: $E000-$FFFF = bank-7 lower half (img 0x1C000+).
        Entry {
            name: "act",
            slot: "a78_act",
            via: Via::Header {
                mapper: 0x0100,
                rom_len: 0x20000,
            },
            spin_img: 0x1C000,
            spin_cpu: 0xE000,
            vec_img: 0x1DFFA,
            expect_pc: Some(0xE000),
        },
        // Flat bankset 64K, Sally view: $F000 -> img 0x7000, vectors 0x7FFC.
        Entry {
            name: "bankset",
            slot: "a78_bankset",
            via: Via::Header {
                mapper: 0x2000,
                rom_len: 0x10000,
            },
            spin_img: 0x7000,
            spin_cpu: 0xF000,
            vec_img: 0x7FFA,
            expect_pc: Some(0xF000),
        },
        Entry {
            name: "bankset_p4000",
            slot: "a78_bankset_p4000",
            via: Via::Header {
                mapper: 0x2001,
                rom_len: 0x10000,
            },
            spin_img: 0x7000,
            spin_cpu: 0xF000,
            vec_img: 0x7FFA,
            expect_pc: Some(0xF000),
        },
        // Remaining bankset matrix: the Sally window shows the same
        // first-half image as plain bankset, so the reset vectors land on
        // the stamped spin loops (verified boot PCs, now pinned).
        Entry {
            name: "bankset_bankram",
            slot: "a78_bankset_bankram",
            via: Via::Header {
                mapper: 0x6000,
                rom_len: 0x10000,
            },
            spin_img: 0x7000,
            spin_cpu: 0xF000,
            vec_img: 0x7FFA,
            expect_pc: Some(0xF000),
        },
        Entry {
            name: "bankset_sg",
            slot: "a78_bankset_sg",
            via: Via::Header {
                mapper: 0x2002,
                rom_len: 0x10000,
            },
            spin_img: 0x7000,
            spin_cpu: 0xF000,
            vec_img: 0x7FFA,
            expect_pc: Some(0xF000),
        },
        Entry {
            name: "bankset_sg_bankram",
            slot: "a78_bankset_sg_bankram",
            via: Via::Header {
                mapper: 0x6002,
                rom_len: 0x10000,
            },
            spin_img: 0x7000,
            spin_cpu: 0xF000,
            vec_img: 0x7FFA,
            expect_pc: Some(0xF000),
        },
        Entry {
            name: "bankset_sg_p800",
            slot: "a78_bankset_sg_p800",
            via: Via::Header {
                mapper: 0xA002,
                rom_len: 0x10000,
            },
            spin_img: 0x7000,
            spin_cpu: 0xF000,
            vec_img: 0x7FFA,
            expect_pc: Some(0xF000),
        },
        Entry {
            name: "bankset_sg_bankram_p800",
            slot: "a78_bankset_sg_bankram_p800",
            via: Via::Header {
                mapper: 0xE002,
                rom_len: 0x10000,
            },
            spin_img: 0x7000,
            spin_cpu: 0xF000,
            vec_img: 0x7FFA,
            expect_pc: Some(0xF000),
        },
        Entry {
            name: "bankset_bankram_p800",
            slot: "a78_bankset_bankram_p800",
            via: Via::Header {
                mapper: 0xE000,
                rom_len: 0x10000,
            },
            spin_img: 0x7000,
            spin_cpu: 0xF000,
            vec_img: 0x7FFA,
            expect_pc: Some(0xF000),
        },
        // 52K: rom_len >= 0x1A000 wraps the bankset window below zero, so
        // the Sally window is open bus upstream too — fog boot, still
        // deterministic (parity-true wrap, see BanksetRom::window).
        Entry {
            name: "bankset_52k",
            slot: "a78_bankset_52k",
            via: Via::Header {
                mapper: 0x2000,
                rom_len: 0x1A000,
            },
            spin_img: 0x7000,
            spin_cpu: 0xF000,
            vec_img: 0x7FFA,
            expect_pc: None,
        },
        Entry {
            name: "bankset_52k_p4000",
            slot: "a78_bankset_52k_p4000",
            via: Via::Header {
                mapper: 0x2001,
                rom_len: 0x1A000,
            },
            spin_img: 0x7000,
            spin_cpu: 0xF000,
            vec_img: 0x7FFA,
            expect_pc: None,
        },
        Entry {
            name: "megacart",
            slot: "a78_megacart",
            via: Via::Header {
                mapper: 0x0022,
                rom_len: 0x44000,
            },
            spin_img: 0x40000,
            spin_cpu: 0xC000,
            vec_img: 0x43FFA,
            expect_pc: None,
        },
        Entry {
            name: "versa",
            slot: "a78_versa",
            via: Via::Header {
                mapper: 0x0022,
                rom_len: 0x10000,
            },
            spin_img: 0xC000,
            spin_cpu: 0xC000,
            vec_img: 0xFFFA,
            expect_pc: Some(0xC000),
        },
        flat("p450_t0", "a78_p450_t0", 0x0040),
        flat("p450_t1", "a78_p450_t1", 0x0041),
        sg32("p450_t2", "a78_p450_t2", 0x0042),
        sg32("p450_t6", "a78_p450_t6", 0x0046),
        sg9("p450_ta", "a78_p450_ta", 0x004A),
        flat("p800_t0", "a78_p800_t0", 0x8000),
        flat("p800_t1", "a78_p800_t1", 0x8001),
        sg32("p800_t2", "a78_p800_t2", 0x8002),
        sg32("p800_t6", "a78_p800_t6", 0x8006),
        sg9("p800_ta", "a78_p800_ta", 0x800A),
        // Slot-only devices: no header dispatch reaches these (HSC/XM are
        // slot options upstream; Versa+POKEY resolves to no key; plain
        // bankset+POKEY@800 has no dispatch arm), so they build straight
        // from the slot key — the same device code boots either way.
        Entry {
            name: "hsc",
            slot: "a78_hsc",
            via: Via::SlotKey {
                key: "a78_hsc",
                pcb_id: 9,
                rom_len: 0x1000,
            },
            spin_img: 0x000,
            spin_cpu: 0xF000,
            vec_img: 0xFFA,
            expect_pc: None, // bare HSC: no top-slot ROM, vectors read FF
        },
        Entry {
            name: "xm",
            slot: "a78_xm",
            via: Via::SlotKey {
                key: "a78_xm",
                pcb_id: 11,
                rom_len: 0x1000,
            },
            spin_img: 0x000,
            spin_cpu: 0xF000,
            vec_img: 0xFFA,
            expect_pc: None, // passthrough idle: control regs map RAM, not ROM
        },
        Entry {
            name: "p450_vb",
            slot: "a78_p450_vb",
            via: Via::SlotKey {
                key: "a78_p450_vb",
                pcb_id: 0x50,
                rom_len: 0x10000,
            },
            spin_img: 0xC000,
            spin_cpu: 0xC000,
            vec_img: 0xFFFA,
            expect_pc: Some(0xC000),
        },
        Entry {
            name: "bankset_p800",
            slot: "a78_bankset_p800",
            via: Via::SlotKey {
                key: "a78_bankset_p800",
                pcb_id: 13,
                rom_len: 0x10000,
            },
            spin_img: 0x7000,
            spin_cpu: 0xF000,
            vec_img: 0x7FFA,
            expect_pc: Some(0xF000), // reads as plain BanksetRom
        },
    ]
}

fn build_loaded(e: &Entry) -> LoadedCart {
    let mut rom = vec![
        0xFFu8;
        match e.via {
            Via::Header { rom_len, .. } => rom_len,
            Via::SlotKey { rom_len, .. } => rom_len,
        }
    ];
    stamp_spin(&mut rom, e.spin_img, e.spin_cpu, e.vec_img);
    match &e.via {
        Via::Header { mapper, rom_len } => {
            let mut file = header(*mapper, *rom_len as u32).to_vec();
            file.extend_from_slice(&rom);
            load_image(&file).expect("synthetic headered image loads")
        }
        Via::SlotKey { key, pcb_id, .. } => {
            let cart = build_cart(key, rom, None);
            LoadedCart {
                cart,
                pcb_id: *pcb_id,
                slot_key: key,
                header: None,
                quirks: Vec::new(),
                notes: vec!["phase9 synthetic slot-key image"],
            }
        }
    }
}

fn run_frames(id: MachineId, e: &Entry, frames: u64) -> (Vec<u8>, u16, u64) {
    let mut m = Machine::new(id);
    m.load_cart(build_loaded(e));
    m.reset();
    for _ in 0..frames {
        m.step_frame();
    }
    (m.frame().to_vec(), m.sally.pc, m.sally.cycles)
}

#[test]
fn mapper_corpus_boots_deterministically() {
    let entries = corpus();
    // Slot-group coverage: every key the loader can produce, plus the
    // slot-only devices.
    let mut slots: Vec<&str> = entries.iter().map(|e| e.slot).collect();
    slots.sort_unstable();
    slots.dedup();
    assert!(
        slots.len() >= 30,
        "corpus covers the slot universe: {slots:?}"
    );
    eprintln!("corpus slots ({}): {}", slots.len(), slots.join(" "));

    for e in &entries {
        let loaded = build_loaded(e);
        assert_eq!(loaded.slot_key, e.slot, "{} dispatches", e.name);
        if loaded.header.is_some() {
            assert!(loaded.quirks.is_empty(), "{}: clean mapper", e.name);
        }
        let (f1, pc1, cyc1) = run_frames(MachineId::Ntsc, e, 3);
        if let Some(pc) = e.expect_pc {
            assert_eq!(pc1, pc, "{} boots to its spin loop", e.name);
        }
        let (f2, pc2, cyc2) = run_frames(MachineId::Ntsc, e, 3);
        assert_eq!(fnv1a(&f1), fnv1a(&f2), "{}: frame replay identical", e.name);
        assert_eq!((pc1, cyc1), (pc2, cyc2), "{}: cpu replay identical", e.name);
        eprintln!(
            "{}: slot={} pc={pc1:04X} cyc={cyc1} hash={:08x}",
            e.name,
            e.slot,
            fnv1a(&f1)
        );
    }
}

#[test]
fn pal_and_devmode_boot_deterministically() {
    let e = corpus().into_iter().find(|e| e.name == "flat").unwrap();
    for id in [MachineId::Pal, MachineId::NtscDev] {
        let (f1, pc1, _) = run_frames(id, &e, 2);
        let (f2, pc2, _) = run_frames(id, &e, 2);
        assert_eq!(fnv1a(&f1), fnv1a(&f2), "{id:?} replay identical");
        assert_eq!(pc1, pc2);
        assert_eq!(pc1, 0xF000);
    }
    let mut pal = Machine::new(MachineId::Pal);
    pal.load_cart(build_loaded(&e));
    pal.reset();
    assert_eq!(pal.frame_dims(), (320, 313));
}

/// Compact DMA-on bar ROM (geometry authority: phase7_machine.rs): MARIA
/// init + 16-line zone, graph at $F026 (no 64K wrap), pens from palette[7].
fn bar_image() -> Vec<u8> {
    let mut img = vec![0xFFu8; 0x1000];
    #[rustfmt::skip]
    let code: &[u8] = &[
        0x78, 0xD8, 0xA2, 0xFF, 0x9A, 0xA9, 0x00, 0x8D, 0x20, 0x00,
        0xA9, 0x05, 0x8D, 0x21, 0x00, 0xA9, 0x3F, 0x8D, 0x27, 0x00,
        0xA9, 0xF1, 0x8D, 0x2C, 0x00, 0xA9, 0x00, 0x8D, 0x30, 0x00,
        0xA9, 0x40, 0x8D, 0x3C, 0x00, 0x4C, 0x23, 0xF0,
    ];
    img[..code.len()].copy_from_slice(code);
    img[0x100..0x108].copy_from_slice(&[0x0F, 0xF1, 0x10, 0x00, 0x00, 0xF1, 0x2F, 0x00]);
    img[0x110..0x114].copy_from_slice(&[0x26, 0x38, 0xF0, 20]);
    img[0x115] = 0x00;
    img[0x026..0x02E].copy_from_slice(&[0xFF; 8]);
    img[0x130] = 0x00;
    img[0xFFA] = 0x23;
    img[0xFFB] = 0xF0;
    img[0xFFC] = 0x00;
    img[0xFFD] = 0xF0;
    img[0xFFE] = 0x23;
    img[0xFFF] = 0xF0;
    img
}

fn bar_machine() -> Machine {
    let mut m = Machine::new(MachineId::Ntsc);
    m.load_cart(load_image(&bar_image()).expect("bar image loads flat"));
    m.reset();
    m
}

/// Scripted input replay (§5 replays/*.json concept, hermetic edition):
/// frame-stamped port + console events through the socket, then the same
/// script on a fresh session must reproduce the frame hash and CPU state
/// bit-identically — while the events observably take effect mid-run.
#[test]
fn scripted_input_replay_is_deterministic() {
    use rusty7800_core::socket::AgentSession;
    use serde_json::{json, Value};

    fn dispatch(s: &mut AgentSession, method: &str, params: Value) -> Value {
        let resp =
            s.dispatch(&json!({"jsonrpc": "2.0", "method": method, "params": params, "id": 1}));
        resp.get("result").cloned().unwrap_or_else(|| {
            panic!(
                "{method} errored: {}",
                resp.get("error").unwrap_or(&json!(null))
            )
        })
    }
    // Frame-stamped script: (frame, method, params).
    let script: Vec<(u64, &str, Value)> = vec![
        (
            5,
            "input.event",
            json!({"port": 1, "control": "up", "value": true}),
        ),
        (
            10,
            "input.event",
            json!({"port": 1, "control": "fire", "value": 1}),
        ),
        (
            12,
            "input.console",
            json!({"button": "pause", "pressed": true}),
        ),
        (
            15,
            "input.event",
            json!({"port": 1, "control": "up", "value": false}),
        ),
        (
            16,
            "input.console",
            json!({"button": "pause", "pressed": false}),
        ),
    ];
    fn play(script: &[(u64, &str, Value)]) -> (u32, Value, Value) {
        let mut s = AgentSession::new(MachineId::Ntsc);
        dispatch(
            &mut s,
            "control.load",
            json!({"image": bar_image(), "label": "bar"}),
        );
        let mut mid_joy = Value::Null;
        for frame in 0..20u64 {
            for (at, method, params) in script {
                if *at == frame {
                    dispatch(&mut s, method, params.clone());
                }
            }
            dispatch(&mut s, "control.step", json!({"frames": 1}));
            if frame == 6 {
                mid_joy = dispatch(&mut s, "state.input", json!({}));
            }
        }
        let frame = dispatch(&mut s, "screen.frame", json!(null));
        let pens: Vec<u8> = frame
            .get("pens")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u8)
            .collect();
        let cpu = dispatch(&mut s, "state.cpu", json!(null));
        (fnv1a(&pens), cpu, mid_joy)
    }
    let (h1, cpu1, mid1) = play(&script);
    // Events took effect mid-run: `up` clears its active-low bit ($AF->$AE).
    assert_eq!(mid1.get("p1").unwrap().get("joy"), Some(&json!(0xAE)));
    assert!(cpu1.get("cycles").and_then(Value::as_u64).unwrap() > 0);
    let (h2, cpu2, _) = play(&script);
    assert_eq!(h1, h2, "replay reproduces the frame hash");
    assert_eq!(cpu1, cpu2, "replay reproduces CPU state");
    eprintln!("replay hash={h1:08x}");
}

/// Frame-time perf sample: 60 DMA-on NTSC frames, p50/p99 reported for
/// `docs/PARITY.md`. The ceiling is generous (debug builds count too) —
/// its job is catching >10× regressions, not benchmarking hardware.
#[test]
fn frame_time_sample() {
    let mut m = bar_machine();
    m.step_frame(); // warm up (audio frac, caches)
    let mut times: Vec<u128> = Vec::with_capacity(60);
    for _ in 0..60 {
        let t0 = std::time::Instant::now();
        m.step_frame();
        times.push(t0.elapsed().as_micros());
    }
    times.sort_unstable();
    let p50 = times[times.len() / 2];
    let p99 = times[times.len() * 99 / 100];
    eprintln!("frame-time p50={p50}us p99={p99}us (debug build)");
    assert!(p99 < 250_000, "p99 frame time {p99}us exceeds 250ms budget");
}

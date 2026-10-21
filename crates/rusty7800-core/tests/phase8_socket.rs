//! Phase 8 acceptance: headless agent sessions through JSON-RPC.
//!
//! Plan §8: "headless script plays a ROM: asserts pixels change, injects
//! input, reads perf — all without the GUI." Every interaction here goes
//! through [`AgentSession::dispatch`] (single + batch envelopes included).
//!
//! The ROM is synthesized (no repo binaries): a 4K flat image that copies
//! its display list into `$1800` RAM, then `INC`s the hpos byte about
//! once per frame — so consecutive frame samples differ (the bar drifts
//! right) while two identical sessions stay bit-identical.

use rusty7800_core::socket::AgentSession;
use rusty7800_core::state::fnv1a;
use rusty7800_core::timing::MachineId;
use serde_json::{json, Value};

/// Animated 4K ROM (vectors → $F000 / main loop at $F030).
fn anim_rom() -> Vec<u8> {
    let mut img = vec![0xFFu8; 0x1000];
    #[rustfmt::skip]
    let code: &[u8] = &[
        0x78,                   // SEI
        0xD8,                   // CLD
        0xA2, 0xFF,             // LDX #$FF
        0x9A,                   // TXS
        0xA9, 0x00,             // LDA #$00
        0x8D, 0x20, 0x00,       // STA $0020 BACKGRND
        0xA9, 0x05,             // LDA #$05
        0x8D, 0x21, 0x00,       // STA $0021 palette[1]
        0xA9, 0x3F,             // LDA #$3F
        0x8D, 0x27, 0x00,       // STA $0027 palette[7]
        0xA9, 0xF1,             // LDA #$F1
        0x8D, 0x2C, 0x00,       // STA $002C DPPH -> DLL at $F100
        0xA9, 0x00,             // LDA #$00
        0x8D, 0x30, 0x00,       // STA $0030 DPPL
        0xA2, 0x00,             // LDX #$00
        // copy: LDA $F110,X : STA $1800,X : INX : CPX #4 : BNE copy
        0xBD, 0x10, 0xF1,
        0x9D, 0x00, 0x18,
        0xE8,
        0xE0, 0x04,
        0xD0, 0xF5,             // -11 -> copy
        0xA9, 0x40,             // LDA #$40
        0x8D, 0x3C, 0x00,       // STA $003C CTRL: DMA on, 160A
        // main ($F030): INC $1803 : JSR wait : JMP main
        0xEE, 0x03, 0x18,
        0x20, 0x39, 0xF0,       // JSR $F039
        0x4C, 0x30, 0xF0,       // JMP $F030
        // wait ($F039): X=24 outer, Y=256 inner -> ~31k cycles ~= 1 frame
        0xA2, 0x18,             // LDX #$18
        0xA0, 0x00,             // LDY #$00
        0x88,                   // DEY
        0xD0, 0xFC,             // BNE inner (-3)
        0xCA,                   // DEX
        0xD0, 0xF8,             // BNE outer (-8)
        0x60,                   // RTS
    ];
    img[..code.len()].copy_from_slice(code);
    // DLL at $F100: 16-line zone -> RAM DL $1800, then end-of-list.
    img[0x100..0x107].copy_from_slice(&[0x0F, 0x18, 0x00, 0x00, 0xF1, 0x2F, 0x00]);
    // DL template at $F110: graph $F120, width 8, pal 1, hpos 20.
    img[0x110..0x114].copy_from_slice(&[0x20, 0x38, 0xF1, 20]);
    img[0x120..0x128].copy_from_slice(&[0xFF; 8]);
    img[0x130] = 0x00; // DL terminator after the trailing entry
    img[0xFFA] = 0x30;
    img[0xFFB] = 0xF0;
    img[0xFFC] = 0x00;
    img[0xFFD] = 0xF0;
    img[0xFFE] = 0x30;
    img[0xFFF] = 0xF0;
    img
}

fn call(session: &mut AgentSession, method: &str, params: Value) -> Value {
    let id = 1;
    let resp = session.dispatch(&json!({"jsonrpc": "2.0", "method": method, "params": params, "id": id}));
    assert_eq!(resp.get("jsonrpc"), Some(&json!("2.0")));
    assert_eq!(resp.get("id"), Some(&json!(id)));
    resp.get("result").cloned().unwrap_or_else(|| {
        panic!("{method} errored: {}", resp.get("error").unwrap_or(&json!(null)))
    })
}

fn call_err(session: &mut AgentSession, method: &str, params: Value) -> (i64, String) {
    let resp = session.dispatch(&json!({"jsonrpc": "2.0", "method": method, "params": params, "id": 7}));
    let err = resp.get("error").unwrap_or_else(|| panic!("{method} should fail"));
    (err.get("code").and_then(Value::as_i64).unwrap(), err.get("message").and_then(Value::as_str).unwrap_or("").to_string())
}

fn pens_of(result: &Value) -> Vec<u8> {
    result
        .get("pens")
        .and_then(Value::as_array)
        .expect("pens array")
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect()
}

fn frame_hash(session: &mut AgentSession) -> (u32, usize, usize) {
    let r = call(session, "screen.frame", json!({}));
    let pens = pens_of(&r);
    let w = r.get("width").and_then(Value::as_u64).unwrap() as usize;
    let h = r.get("height").and_then(Value::as_u64).unwrap() as usize;
    assert_eq!(pens.len(), w * h);
    assert_eq!((w, h), (320, 263));
    // DMA trace covers the completed frame.
    let dma = r.get("dma_per_line").and_then(Value::as_array).unwrap();
    assert_eq!(dma.len(), 263);
    (fnv1a(&pens), w, h)
}

fn load_anim(session: &mut AgentSession) {
    let r = call(session, "control.load", json!({"image": anim_rom(), "label": "anim"}));
    assert_eq!(r.get("slot"), Some(&json!("a78_rom")));
}

#[test]
fn headless_replay_pixels_change_and_deterministic() {
    let mut a = AgentSession::new(MachineId::Ntsc);
    load_anim(&mut a);
    // First frame already draws the bar (not a blank boot).
    let r = call(&mut a, "control.step", json!({"frames": 1}));
    assert_eq!(r.get("frames_run"), Some(&json!(1)));
    let (h0, _, _) = frame_hash(&mut a);
    let r = call(&mut a, "screen.frame", json!({}));
    let pens = pens_of(&r);
    assert!(pens.contains(&0x3F), "bar pens visible");
    assert!(pens.contains(&0x00), "background visible");

    // Animation: samples 10 frames apart differ (hpos drifts ~+10).
    let mut hashes = vec![h0];
    for _ in 0..3 {
        call(&mut a, "control.step", json!({"frames": 10}));
        hashes.push(frame_hash(&mut a).0);
    }
    for w in hashes.windows(2) {
        assert_ne!(w[0], w[1], "bar moves between samples");
    }

    // Determinism: an identical session replays bit-identical frames.
    let mut b = AgentSession::new(MachineId::Ntsc);
    load_anim(&mut b);
    call(&mut b, "control.step", json!({"frames": 1}));
    assert_eq!(frame_hash(&mut b).0, h0);
    for want in &hashes[1..] {
        call(&mut b, "control.step", json!({"frames": 10}));
        assert_eq!(&frame_hash(&mut b).0, want);
    }
    // CPU/RIOT state matches too.
    let ca = call(&mut a, "state.cpu", json!({}));
    let cb = call(&mut b, "state.cpu", json!({}));
    assert_eq!(ca, cb);
}

#[test]
fn input_event_and_console_visible_in_state() {
    let mut s = AgentSession::new(MachineId::Ntsc);
    load_anim(&mut s);
    let idle = call(&mut s, "state.input", json!({}));
    assert_eq!(idle.get("p1").unwrap().get("joy"), Some(&json!(0xAF)));
    let r = call(&mut s, "input.event", json!({"port": 1, "control": "up", "value": true}));
    assert_eq!(r.get("joy"), Some(&json!(0xAE)));
    let st = call(&mut s, "state.input", json!({}));
    assert_eq!(st.get("p1").unwrap().get("joy"), Some(&json!(0xAE)));
    call(&mut s, "input.event", json!({"port": 1, "control": "up", "value": false}));
    // Proline A button via fire alias.
    call(&mut s, "input.event", json!({"port": 2, "control": "fire", "value": 1}));
    // Console pause press clears bit 3 (active-low byte).
    let before = call(&mut s, "input.console", json!({"button": "pause", "pressed": false}));
    let after = call(&mut s, "input.console", json!({"button": "pause", "pressed": true}));
    assert_ne!(before.get("byte"), after.get("byte"));
    // Wrong-device control is a param error, not a panic.
    let (code, _) = call_err(&mut s, "input.event", json!({"port": 1, "control": "pot_x", "value": 80}));
    assert_eq!(code, -32602);
}

#[test]
fn perf_sample_reports_real_counters() {
    let mut s = AgentSession::new(MachineId::Ntsc);
    load_anim(&mut s);
    call(&mut s, "control.step", json!({"frames": 5}));
    let perf = call(&mut s, "perf.sample", json!({}));
    assert_eq!(perf.get("frames_run"), Some(&json!(5)));
    assert!(perf.get("cycles_total").and_then(Value::as_u64).unwrap() > 100_000);
    assert!(perf.get("dma_last_frame").and_then(Value::as_i64).unwrap() > 0);
    assert!(perf.get("pending_samples").and_then(Value::as_u64).unwrap() > 400);
    assert!(perf.get("last_frame_us").and_then(Value::as_u64).unwrap() > 0);
}

#[test]
fn save_load_round_trip_mid_frame_exact() {
    let mut a = AgentSession::new(MachineId::Ntsc);
    load_anim(&mut a);
    call(&mut a, "control.step", json!({"lines": 100}));
    let saved = call(&mut a, "control.save", json!({}));
    let snap = saved.get("snapshot").unwrap().clone();
    call(&mut a, "control.step", json!({"lines": 50}));
    let after_a = call(&mut a, "screen.frame", json!({}));
    let cpu_a = call(&mut a, "state.cpu", json!({}));
    let maria_a = call(&mut a, "state.maria", json!({}));

    // Reference: uninterrupted 150 lines.
    let mut b = AgentSession::new(MachineId::Ntsc);
    load_anim(&mut b);
    call(&mut b, "control.step", json!({"lines": 150}));
    let after_b = call(&mut b, "screen.frame", json!({}));
    assert_eq!(pens_of(&after_a), pens_of(&after_b));

    // Restore the 100-line snapshot, replay 50: bit-identical again.
    call(&mut a, "control.load_snapshot", json!({"snapshot": snap}));
    call(&mut a, "control.step", json!({"lines": 50}));
    let replay = call(&mut a, "screen.frame", json!({}));
    assert_eq!(pens_of(&replay), pens_of(&after_b));
    assert_eq!(call(&mut a, "state.cpu", json!({})), cpu_a);
    assert_eq!(call(&mut a, "state.maria", json!({})), maria_a);

    // Wrong-cart snapshot is rejected, not applied.
    let mut c = AgentSession::new(MachineId::Ntsc);
    let (code, msg) = call_err(&mut c, "control.load_snapshot", json!({"snapshot": snap}));
    assert_eq!(code, -32602);
    assert!(msg.contains("cart"), "{msg}");
}

#[test]
fn envelopes_and_fuzz_never_panic() {
    let mut s = AgentSession::new(MachineId::Ntsc);
    // Unknown method.
    let (code, _) = call_err(&mut s, "nope.missing", json!({}));
    assert_eq!(code, -32601);
    // Missing method field.
    let r = s.dispatch(&json!({"jsonrpc": "2.0", "id": 1}));
    assert_eq!(r.get("error").unwrap().get("code"), Some(&json!(-32600)));
    // Batch: mixed ok + errors, order preserved.
    let r = s.dispatch(&json!([
        {"jsonrpc": "2.0", "method": "state.cpu", "params": {}, "id": 1},
        {"jsonrpc": "2.0", "method": "bogus", "params": {}, "id": 2},
        {"jsonrpc": "2.0", "method": "control.step", "params": {}, "id": 3},
    ]));
    let arr = r.as_array().unwrap();
    assert_eq!(arr.len(), 3);
    assert!(arr[0].get("result").is_some());
    assert!(arr[1].get("error").is_some());
    assert!(arr[2].get("error").is_some());

    // Deterministic byte-level fuzz over a valid session (fixed seed).
    load_anim(&mut s);
    let seed = json!({"jsonrpc": "2.0", "method": "control.step", "params": {"frames": 1}, "id": 1}).to_string();
    let mut x: u64 = 0x243F_6A88_85A3_08D3;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for i in 0..300 {
        let mut bytes = seed.clone().into_bytes();
        for _ in 0..(1 + next() % 6) {
            let at = (next() as usize) % bytes.len();
            bytes[at] = (next() & 0xFF) as u8;
        }
        if i % 3 == 0 {
            bytes.truncate(1 + (next() as usize) % bytes.len().max(1));
        }
        let req: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let resp = s.dispatch(&req);
        // Every reply is a well-formed envelope (single or batch).
        match resp {
            Value::Array(items) => {
                for it in &items {
                    assert!(it.get("result").is_some() || it.get("error").is_some());
                }
            }
            obj => assert!(obj.get("result").is_some() || obj.get("error").is_some()),
        }
    }
}

#[test]
fn hiscore_nvram_battery_round_trip() {
    use rusty7800_core::cart::{build_cart, NullBus};
    // Battery write -> persist image -> fresh cart boots with the byte.
    let mut cart = build_cart("a78_hsc", vec![0u8; 0x1000], None);
    let mut bus = NullBus;
    cart.write_10xx(0x1000, 0x5A, &mut bus);
    assert_eq!(cart.read_10xx(0x1000, &mut bus), 0x5A);
    let image = cart.nvram_image().expect("HSC has a battery");
    assert_eq!(image[0], 0x5A);
    assert!(image.iter().skip(1).all(|&b| b == 0xFF));
    let mut fresh = build_cart("a78_hsc", vec![0u8; 0x1000], Some(&image));
    assert_eq!(fresh.read_10xx(0x1000, &mut bus), 0x5A);
    // Snapshot carries NVRAM too.
    let snap = cart.save_state();
    assert_eq!(snap.nvram[0], 0x5A);
    let mut other = build_cart("a78_hsc", vec![0u8; 0x1000], None);
    other.load_state(&snap);
    assert_eq!(other.read_10xx(0x1000, &mut bus), 0x5A);
}

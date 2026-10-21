# Parity report (plan §9 — validation & 1:1 parity)

> Evidence for plan §2 (feature checklist) and §10 (definition of done).
> All hermetic tests below run with plain `cargo test`; the real-ROM audit
> additionally runs when `ROMS_DIR` points at a local corpus (never
> committed). Frame-hash parity against the original C++ build is pending
> the optional §10 socket shim, which does not exist yet.

## Corpus: one bootable image per slot group (Phase 9, hermetic)

`crates/rusty7800-core/tests/phase9_parity.rs::mapper_corpus_boots_deterministically`
builds 35 synthetic images (no copyrighted bytes), asserts the exact slot
key, a clean boot, a pinned spin-loop PC where the reset-window math is
exact, and bit-identical replay (frame bytes + CPU cycles) across two
machines. PAL + dev-mode machines replay identically too.

| entry | slot | boot PC | note |
|---|---|---|---|
| flat | a78_rom | $F000 | |
| pokey | a78_pokey | $F000 | |
| sg | a78_sg | $C000 | last 16K fixed |
| sg_pokey | a78_sg_pokey | $C000 | |
| sg_ram | a78_sg_ram | $C000 | |
| sg9 | a78_sg9 | $C000 | 144K, bank 8 fixed |
| mram | a78_mram | $F000 | flat tail at $8000+ |
| abs | a78_abs | $8000 | upper 32K fixed |
| act | a78_act | $E000 | bank-7 lower half |
| bankset | a78_bankset | $F000 | Sally first-half view |
| bankset_p4000 | a78_bankset_p4000 | $F000 | reads as plain bankset |
| bankset_bankram | a78_bankset_bankram | $F000 | |
| bankset_sg | a78_bankset_sg | $F000 | |
| bankset_sg_bankram | a78_bankset_sg_bankram | $F000 | |
| bankset_sg_p800 | a78_bankset_sg_p800 | $F000 | |
| bankset_sg_bankram_p800 | a78_bankset_sg_bankram_p800 | $F000 | |
| bankset_bankram_p800 | a78_bankset_bankram_p800 | $F000 | |
| bankset_p800 | a78_bankset_p800 | $F000 | slot-only (no header arm) |
| bankset_52k / _p4000 | a78_bankset_52k… | fog ($FFFF) | window wraps below zero — parity-true open bus, deterministic |
| megacart | a78_megacart | fog ($FFFF) | deterministic |
| versa | a78_versa | $C000 | |
| p450_t0/t1/t2/t6/ta | a78_p450_* | $F000/$F000/$C000/$C000/$C000 | mirror flat/SG/SG9 windows |
| p800_t0/t1/t2/t6/ta | a78_p800_* | $F000/$F000/$C000/$C000/$C000 | |
| p450_vb | a78_p450_vb | $C000 | slot-only (0x60 hits no key) |
| hsc | a78_hsc | fog ($FFFF) | bare HSC: no top-slot ROM; NVRAM round-trip in phase8 |
| xm | a78_xm | fog ($FFFF) | passthrough idle maps RAM; deterministic |

Fog-boot entries execute open-bus `$FF` (SBC) deterministically — same
bytes, same hashes, every run. All 35 dispatch with zero header quirks.

## Replay determinism

- `phase8_socket::headless_replay_pixels_change_and_deterministic` — anim
  ROM: frames differ across time, two sessions bit-identical (frames +
  CPU state).
- `phase9_parity::scripted_input_replay_is_deterministic` — frame-stamped
  port + console script through the socket (`replay hash=a02057e9`);
  mid-run `state.input` proves events apply; replay reproduces hash + CPU.
- `phase8_socket::save_load_round_trip_mid_frame_exact` — 100-line
  snapshot → +50 lines ≡ uninterrupted 150 lines, bit-identical.

## Perf

`phase9_parity::frame_time_sample` — 60 DMA-on NTSC frames, debug build
on the dev machine (2026-10-20): **p50 = 2049 µs, p99 = 3462 µs per
frame** (~290 fps, ~5× headroom over 60 Hz even unoptimized). The test
pins a generous 250 ms p99 ceiling as a >10× regression tripwire.
Release numbers and Win/macOS/Linux matrix are app-shell work.

## §2 checklist status

Emulation core — all pass hermetically:

- SALLY + stalls/WSYNC/DMA halt/IRQ/NMI: `phase1_cpu`, `phase2_maria`,
  `phase7_machine` — PASS (unit-level; ±1-cycle C++ trace diff needs §10).
- Address map + mirrors + dynamic overlays: `phase1_bus`,
  `spec_constants`, `phase5_cart` — PASS.
- MARIA registers/DLL/DL/modes/holes/DMALIMIT/dev overlay/VBLANK/WSYNC:
  `phase2_maria`, `phase7_machine` — PASS.
- TIA audio + INPT/paddle/lightgun timing: `phase1_tia`,
  `phase3_tia_io` — PASS (unit).
- RIOT + timer fix + PA/PB + pull-ups: `phase1_riot`, `phase4_ctrl` — PASS.
- 17 cart groups + quirks + NVRAM + XM regs + bankset views: `phase5_cart`,
  `rom_corpus` (real ROMs when `ROMS_DIR` set), phase-9 corpus — PASS.
- POKEY/YM2151: routing + bases real, sound Phase-0 stubs — PARTIAL
  (audible POKEY/YM is app-shell work; `cart_pokey_bases` pinned).
- 9 controllers × 2 ports + console + DIPs: `phase4_ctrl`,
  `phase8_socket::input_event_and_console_visible_in_state` — PASS.
- NTSC/PAL + 8 variants + 6 literal palettes + BIOS + silent boot:
  `phase6_timing`, `phase7_machine`, phase-9 PAL/devmode — PASS.

System / UX — core side done, shell pending:

- ROM loading (headered/headerless + mapper auto-select): PASS.
  Softlist/zip loading: NOT IMPLEMENTED (no `hash/a7800.xml` consumer).
- Save states + NVRAM + pause/step/snapshot via socket: PASS.
  Slots/throttle/frameskip/remap UI: app-shell work — PENDING.
- 7800-focused UI, ini/keymap equivalents, debugger panel: PENDING
  (plan Phase 7 app shell).

## Known gaps (plan §9 + §10 DoD)

1. C++ frame-hash / audio-RMS parity: PENDING — needs the §10 socket
   shim in `a7800-5.2/` (optional phase, not started).
2. `cargo clippy -- -D warnings` + frontend typecheck on Win/macOS/Linux:
   clippy is clean on this machine; the matrix awaits CI + shell.
3. MAME OSD extras (cheats, artwork, HLSL/BGFX, plugins): out of scope
   per plan §9.4 unless an `a7800*` path reaches them — none do from the
   core test surface.

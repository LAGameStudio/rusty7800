# Validation report — Phases 0–3 (2026-10-20)

Fresh re-validation of all new work in `newwork/` against
`../a7800-rust-tauri-plan.md` §4 acceptance criteria. No source files were
changed by this validation; results below are from a clean tree.

## Fresh runs (exit status preserved via `pipefail` + `PIPESTATUS`)

- `cargo test -p rusty7800-core` → **exit 0**, 53/53 green:
  - phase1_bus 5, phase1_cpu 11, phase1_riot 7, phase1_tia 6,
    phase2_maria 9, phase3_tia_io 8, spec_constants 7. Doc-tests 0.
- `cargo clippy -p rusty7800-core --all-targets -- -D warnings` → **exit 0**.
- `git status --short` → clean; `git log` shows one commit per phase
  (`a8f563f` Phase 0, `90a69ec` Phase 1, `c2bf75b` Phase 2, `5609517` Phase 3).
- Palette regeneration: `python3 scripts/gen_palette.py
  ../a7800-5.2/src/mame/drivers/a7800.cpp /tmp/palette_regen.rs` →
  **byte-identical** to committed `palette.rs` (`PALETTE_REGEN_IDENTICAL`).

## Phase 0 — Scaffold + spec lock: PASS

- `#![forbid(unsafe_code)]` in `lib.rs`; `grep -rn unsafe src/` finds only
  that attribute. No exception list needed (no audio/FFI shims yet).
- `specs.rs` (232 lines): every constant carries a source-line comment
  (clocks, screen params, full address map + dynamic overlays, TIA sample
  rate, MARIA modes/holey masks, 9 controller slots + default, 35 cart
  slots, 8 machines, console keys T/E/O, BIOS/HSC/XM identity + sizes,
  POKEY bases + header flag bits, paddle divisor 97). TIA/MARIA/RIOT
  register maps live with their modules (`tia.rs`, `maria.rs`, `riot.rs`).
- `palette.rs`: 6×256 literal pens + `PENS_TOTAL=768` zero-fill space;
  `0X51` typo triplet preserved and pinned at `HOT_NTSC[152]`; Warm/PAL/Hot
  spot-checks pass. Generator verified reproducible (see above).
- `spec_constants.rs`: 7/7 green (counts, addresses, hashes, ramps).

## Phase 1 — CPU + bus + RIOT + TIA audio: PASS (3 noted gaps)

- `sally.rs` (1939 lines): hand-port NMOS 6502, official + reliable
  unofficial + visual6502-stable formulas, page-cross/branch/RMW-dummy
  costs, JMP-indirect page-wrap bug, IRQ/NMI/BRK sequences, KIL jam, BCD.
  11 CPU tests green (incl. decimal quirk, RTS SP behavior, 0xEB coverage).
- `bus.rs` (152 lines): decode table, SRAM, zp/sp mirrors (tested
  `$2040→$0040`, `$2140→$0140`), open-bus 0xFF, half-cycle stall model
  (TIA regs + RIOT RAM = 3 halves vs 2). 5 bus tests green.
- `riot.rs` (263 lines): cycle-stepped 6532 with Counting/Finishing states,
  Saarna timer-fix rules in header + `finishing_free_runs_and_keeps_counting`
  regression. 7 RIOT tests green.
- `tia.rs` (282 lines): independent TIA-audio implementation (AUDC table,
  poly4/5/9, div-n/div-3, 31400 Hz conversion); GPL provenance documented
  in module header. 6 TIA tests green.
- Gaps (none blocking; carried to listed phases):
  1. No nestest binary run — hand-written suite only (Phase 9 corpus).
  2. RIOT-RAM cross-mirror `$0480↔$0580` behaviorally correct (`& 0x7F`)
     but not explicitly asserted — add one line to `phase1_bus.rs`.
  3. TIA/RIOT address mirrors (`$300`/`$160`) in `specs` but `decode()`
     covers base ranges only — machine-layer concern (Phase 6/7).

## Phase 2 — MARIA: PASS (1 noted gap)

- `maria.rs` (527 lines): registers (BACKGRND fan-out, WSYNC latch, DPPH/L,
  CHARBASE, CTRL incl. color-kill/DMA/CW/BC/KM/RM, MSTAT), DLL/DL walker
  (normal + extended, big-endian DL bytes, `(byte1 & 0x5F)==0` end gate,
  64 KiB wrap), direct + indirect + cwidth, holey masks
  `0x9000`/`0x8800` with once-per-DL penalty, `write_line_ram`
  `(c&3)||kangaroo` transparency, DMA accounting (16 startup, 8/10
  headers, 3/byte, +6 cwidth extra, +8 last-line, +17 DLI, LIMIT 430 →
  halt + WSYNC latch), all pixel renderers (160A/B doubled, 320B/D +
  320A/C shuffles, color kill), dev overlay with `(cycles*0x0F)/428`
  gradient, VBLANK timing, DLL-end NMI. 9 MARIA tests green, incl.
  hand-computed cycle counts (44/38/44 direct/indirect, 29 hole, 52 DLI),
  mid-scanline BACKGRND split, all render modes, dev overlay.
- Gap: DMA ±1-cycle parity vs live C++ traces needs the Phase 9/10
  harness; current counts are hand-computed from the C++ source.
- `dma_cycles()` + `dev_overlay()` accessors feed the future agent socket;
  `socket.rs` remains a Phase 8 stub as planned.

## Phase 3 — TIA reads / paddles / lightguns: PASS

- `tia_io.rs` (270 lines): `TiaPort` trait (Phase 4 implements it),
  INPT0–5 mux, paddle discharge `(cycles-start)/97`, `pot^0xFF` quirk,
  pot-direct vs digital fallbacks per `has_pot`/`is_paddle`, INPT4/5
  pro-button one-button rule, beam-race (NTSC/PAL scaling, dist²<64),
  collision stub `0x00`, undefined `0xFF`, `ctrl_lock`/`ctrl_reg`/
  `maria_flag`, VBLANK ground/release stamp, audio write-through at
  `(ctrl&3)==3`, shared 0.5-cycle stall alternator. 8 tests green
  (ramps incl. boundary, quirk, pot/digital, pro-mode, NTSC hit/near/miss
  + port isolation, PAL scaling, stall sharing, lock/VBLANK/audio).

## Ground rules (§0)

- Read-before-writing: session log records full C++ reads per phase. ✓
- Feature parity = done; later-phase modules (`cart`, `ctrl`, `pokey`,
  `ym2151`, `timing`, `state`, `socket`) are 1–3-line stubs marked with
  their plan phase — no silent scope cuts. ✓
- Adaptations so far: build (Cargo), clean-room TIA audio for license
  hygiene (documented, ear/RMS parity in Phase 9), cycle-stepped RIOT
  instead of event timers (same documented rules). ✓
- Test socket: stub only — mandatory deliverable in Phase 8, on schedule. ✓
- Commit per phase. ✓

## Bottom line

Phases 0–3 meet their acceptance criteria on a clean tree: **53 tests
green, clippy `-D warnings` clean, palette regeneration identical, one
commit per phase.** No test was skipped, weakened, or suppressed to pass;
all historical failures diagnosed as test-setup errors with the
implementation matching C++. Open items: nestest binary + C++ DMA-trace
parity (Phase 9/10 harness), RIOT-RAM cross-mirror assertion (one-liner),
address-mirror wiring at machine integration (Phase 6/7).

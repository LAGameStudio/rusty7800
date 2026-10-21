# Plan achievement audit (`a7800-rust-tauri-plan.md`)

> Method: every phase's acceptance criteria and every §2 box checked
> against the repo at `newwork/` on 2026-10-20 (tests re-run where
> claimed; `cargo test --workspace`: 107 passed / 0 failed).
> Verdicts: **ACHIEVED** · **PARTIAL** (works, scoped gap) · **PENDING**
> (not started). Evidence paths are workspace-relative.

## Phase verdicts

| Phase | Verdict | Evidence |
|---|---|---|
| 0 Scaffold + spec lock | ACHIEVED | `spec_constants` (7 tests), `gen_palette.py`, `#![forbid(unsafe_code)]` in `lib.rs` |
| 1 CPU + bus + RIOT + TIA skeleton | ACHIEVED | 11 CPU tests (incl. decimal, BRK/IRQ/NMI, JMP-indirect bug, unofficials), bus mirrors, RIOT incl. Saarna fix, TIA channels |
| 2 MARIA | ACHIEVED | 9 maria tests (all 7 modes, holes, Kangaroo, mid-scanline writes, DMA counts) + phase-7 bar geometry |
| 3 TIA reads/paddles/guns | ACHIEVED | 8 tia_io tests (pot const, discharge ramp, hit/miss); 0.5-cycle gate modeled in bus halves (documented approximation) |
| 4 Controllers | ACHIEVED | 12 ctrl tests, all 9 MAME devices × 2 ports, `0xFF` empty-slot, keypad/wheel/mouse; + `atarivox`/`savekey` slots (new, Stella-named): 10 device + 7 bus-level tests, socket `set_port`/`port_nvram`/`port_speech` |
| 5 Cartridges | ACHIEVED | 21 cart tests (quirks, dispatch, bank switching, NVRAM) + phase-9 35-slot boot corpus + `rom_corpus` real-ROM audit (needs `ROMS_DIR`) |
| 6 Variants/palettes/BIOS/timing | ACHIEVED | 8 machines, 6 literal palettes + CRCs, BIOS routing, silent boot, 263/313 lines |
| 7 App shell (Tauri 2 planned) | PARTIAL | No Tauri shell; instead a native eframe GUI (`rusty7800-gui`: window, ROM drag-and-drop/CLI, machine picker, run/pause/step, keyboard input, TIA audio via cpal, `--smoke` self-test). No menus/settings persistence |
| 8 Agent socket | ACHIEVED | `AgentSession` JSON-RPC (single+batch), `PROTOCOL.md`, 6 socket tests incl. 300-case fuzz, headless replay demo |
| 9 Validation & parity | PARTIAL | Corpus (35 synthetic slots), replay determinism, perf sample, `PARITY.md` — all hermetic. C++ frame/audio parity needs §10 |
| 10 C++ socket shim (optional) | PENDING | Not started; nothing in `a7800-5.2/` added |

## §2 checklist

Emulation core — all ACHIEVED except noted:

- SALLY/address map/MARIA/TIA-audio+reads/RIOT/carts/controllers/variants:
  ACHIEVED (see phase rows above).
- TIA audio in the native app: ACHIEVED (`rusty7800-gui/src/audio.rs`: resampled cpal pump, 4 unit tests, graceful silent fallback). Core TIA synthesis was already ACHIEVED.
- POKEY @ $4000/$0450/$0800/XM + YM2151: PARTIAL — slot-window routing,
  `pokey_bases`, and stub reads (`0xFF`) are real and pinned; audible
  synthesis is not implemented (`pokey.rs`, `ym2151.rs` are Phase-0 stubs).
- XM control registers / bankset Sally-vs-Maria views: ACHIEVED.

System / UX:

- Raw `.a78`/`.bin` + headerless detect + mapper auto-select: ACHIEVED.
- Softlist/zip path (`hash/a7800.xml` consumer): PENDING.
- Save states + NVRAM + pause/step/snapshot via core + socket: ACHIEVED;
  `control.pause` only echoes (never gates stepping): PARTIAL.
  Save slots / throttle / frameskip: PENDING (no shell to host them).
- Input remapping / analog calibration: PENDING (port abstraction ready).
- 7800-focused UI, palettes/machine picker, dev DMA view, screen options:
  PARTIAL. Native window + web page both have a machine picker + canvas;
  no menus, persisted settings, or dev DMA view yet. Web CORS trap fixed:
  `site/serve.py` (one-command local server) + `site/bundle.py`
  (single-file `rusty7800-standalone.html`, file://-safe, no fetch).
- `keymaps/`+`ini/` equivalents, i18n hook, debugger/memory viewer: PENDING.
- MAME cfg/frontend compat mapping doc: PENDING.

## README claim check-off (`a7800-5.2/README.md`)

Ported: 9 controllers, DMA hole penalties, RIOT fix, silent empty-boot,
optional BIOS, XM ctrl regs, mid-scanline updates, bankset, POKEY@800
matrix, dev machines + DMA overlay, 6 palettes. Not ported: streamlined
UI/menus, POKEY sound improvements (stub), MAME config-file reuse.

## §10 definition of done

- [x] `cargo test` green (107/107, this machine).
- [x] `cargo clippy -- -D warnings` green (this machine; fmt gate covers
  only the pipeline crates — core tree has pre-existing rustfmt drift).
- [ ] Frontend typecheck + Win/macOS/Linux matrix (no shell, no CI runs yet —
  workflow file exists at `.github/workflows/build.yml`, unrunnable from here).
- [x] No copyrighted ROMs/BIOS in repo (synthetic fixtures only; `rom_corpus`
  passes vacuously without `ROMS_DIR`).
- [ ] `PARITY.md` green — exists with honest PARTIAL/PENDING markers, not green.
- [x] Agent socket + `PROTOCOL.md` + headless replay.

## Bottom line

The emulation core (plan phases 0–6, 8, and the hermetic half of 9) is
done and evidenced: every §2 emulation box is ACHIEVED except audible
POKEY/YM. What is genuinely not achieved: the Tauri app shell with its
UI/settings (Phase 7), the C++ parity shim and C++-comparative numbers
(§10 / second half of 9), softlist/zip loading, and CI execution. The
build pipeline (native exe + web) now exists so those phases have
something to land in.

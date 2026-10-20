# Rusty7800

Atari 7800 emulator in Rust + Tauri 2 — a faithful port of **A7800 5.2**
(a fork of the MAME Atari 7800 driver; sources in `../a7800-5.2/`).

Full conversion + validation plan: `docs/a7800-rust-tauri-plan.md`.

## Layout (all work lives in this `newwork/` repo)

- `Cargo.toml` — workspace (Phase 7 adds the Tauri app + UI crates)
- `crates/rusty7800-core/` — pure emulation core, no Tauri dependency
  - `src/specs.rs` — spec constants transcribed from the C++ sources
  - `src/palette.rs` — GENERATED 6×256 palette tables (see `scripts/`)
  - `src/{sally,maria,tia,riot,pokey,ym2151,bus,cart,ctrl,timing,state,socket}.rs`
  - `tests/spec_constants.rs` — Phase 0 acceptance test
- `scripts/gen_palette.py` — regenerates `palette.rs` from `a7800.cpp`
- `docs/` — phase notes and parity reports (Phase 9: `PARITY.md`)
- `../2nd/` (outside this repo) — session log of prior work

## Phase 0 acceptance

```sh
cargo test -p rusty7800-core
cargo clippy -p rusty7800-core -- -D warnings
```

## Rules

- Feature parity with A7800 5.2 is the definition of done; adapt a feature
  to the new platform only when technically required.
- `palette.rs` is generated — never hand-edit; fix the generator instead.

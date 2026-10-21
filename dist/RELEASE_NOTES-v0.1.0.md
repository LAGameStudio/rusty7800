# Rusty7800 v0.1.0 — first release

An Atari 7800 emulator in Rust, ported from A7800 5.2 / MAME, with
native, headless, and web front ends. Windows x86_64 binaries attached.

## What's inside

- **Full system emulation** — SALLY (6502), MARIA graphics, TIA
  video + audio, RIOT, all cartridge PCBs and mappers (SuperGame,
  banksets, XM, High Score cart, POKEY variants, CPUWIZ), all 9
  controller types, NTSC/PAL timing with Warm/Cool/Hot palettes and
  Developer Mode machines, BIOS boot, palette selector.
- **Validated, not just built** — 100+ tests green, including a
  146-ROM corpus audit and a headless smoke boot (`pc=F023
  hash=36ccc1d7`) that the release GUI reproduces exactly.
- **Three ways to play**
  - `rusty7800-gui` — desktop app: drag-and-drop ROMs, machine
    picker with full labels (`Atari 7800 PAL Warm`, …), run/pause/
    step/reset, 1x–4x scaling, live fps/frame/hash status, and a
    red REGION MISMATCH warning that names the right machine
    (mismatched PAL/NTSC games hang instead of booting).
  - `rusty7800-cli` — headless runner: steps N frames, writes a PPM
    screenshot, prints the frame hash. For scripting and regression
    checks.
  - Web shell + single-file standalone (see the repo's
    `crates/rusty7800-web/site/`).
- **Extras** — agent socket API (`rusty7800-agent-sock`) for
  programmatic control, Fuji-inspired app icon baked into every
  surface (exe, window, favicons).

## Quick start

1. Download the `rusty7800-gui-0.1.0-windows-x86_64.zip` below.
2. Extract, double-click `rusty7800-gui.exe`.
3. Drag a ROM (`.a78`/`.bin`) onto the window and press Run.

Player 1: arrows + `Z`/`X`. No install, no BIOS file needed.
No ROMs are included — bring your own legally-dumped images.

## Checksums

Verify with e.g. `certutil -hashfile <file> SHA256` and compare
against the values published with this release.

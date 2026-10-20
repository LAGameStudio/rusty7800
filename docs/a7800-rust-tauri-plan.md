# A7800 → Rust + Tauri 2 Conversion & Validation Plan

> Source under study: `a7800-5.2/` — **A7800 5.2, a fork of the MAME Atari 7800 driver** (`README.md`: "fork of the MAME Atari 7800 driver, with several enhancements").
> Goal: convert **and validate** 100% of the 7800 emulation functionality into a cross-platform **Rust + Tauri 2** app, losing not a single feature. Adapt a feature to the new platform **only** when a technical requirement forces it or when fidelity would otherwise cost great usability.
> Consumer of this plan: an autonomous coding agent. Every phase has inputs (exact source paths), tasks, and acceptance criteria.

---

## 0. Ground rules for the agent

1. **Read before writing.** Read every source file listed in §1 before reimplementing its behavior.
2. **Feature parity is the definition of done.** If it exists in `a7800-5.2` for the `a7800*` machines, it must exist in the Rust app or be listed in §9 with a technical justification.
3. **Prefer porting over redesigning.** Keep register addresses, bit layouts, timing constants, palette RGB values, header-quirk behaviors, and file-format compatibilities bit-identical.
4. **Allowed adaptations:** build system (make → Cargo), UI toolkit (MAME OSD → Tauri frontend), audio/video backends (MAME `screen`/`speaker` → wgpu/canvas + cpal), persistence paths (MAME `ini/`/nvram → OS app-data dirs), threading model (MAME device scheduler → Rust frame loop). Everything else needs justification in the PR description.
5. **Test socket first-class.** Both the optional original-side socket (§10) and the mandatory new-app socket (§8) are deliverables, not debug hacks.
6. **Commit per phase.** One commit per phase below, with phase number in the message.

---

## 1. Source inventory (read all of this — nothing else in the tree is in scope except as a library)

### 1.1 Driver / machine (the orchestrator)

| File | Lines | What to extract |
|---|---|---|
| `a7800-5.2/src/mame/drivers/a7800.cpp` | 1509 | `a7800_state`, full address map `a7800_mem`, TIA read/write wrappers with paddle/lightgun timing, RIOT PA/PB callbacks, `machine_start` dynamic handler installer, `machine_reset` BIOS-detect, 8 machine configs, 8 ROM defs, 8 `DRIVER_INIT`s, palettes, console input ports, save-state registrations |
| `a7800-5.2/hash/a7800.xml` | — | Software list `a7800` (NTSC/PAL filters); `hiscore` + `xm` entries with ROM hashes, RAM/NVRAM sizes |
| `a7800-5.2/README.md`, `README.MAME.md` | — | Feature claims to check off (see §2) |

### 1.2 MARIA (graphics chip)

| File | Lines | Notes |
|---|---|---|
| `a7800-5.2/src/mame/video/maria.cpp` | 532 | DLL/DL walker, `is_holey`, `write_line_ram`, DMA cycle counting + hole penalties, `m_dmaon`/`m_write_mode`/`m_kangaroo`/`m_offset`/`m_charbase`/`m_holey`, mid-scanline register updates, CPU spin during DMA/WSYNC (`m_dmaactive` global), dev-mode DMA overlay, `startdma`, scanline renderer for modes 160A/160B/320A/320B/320C/320D, `MSTAT/VBLANK` handling |
| `a7800-5.2/src/mame/video/maria.h` | 78 | Register/mode state, `m_devmode_flag`, `m_devmode_dmacolor(_width)`, `draw_scanline`, `m_vblank` |

### 1.3 Sound

| File | What to extract |
|---|---|
| `a7800-5.2/src/mame/audio/tiamc1.cpp` (339) / `tiamc1.h` (67) | TIA audio path used by driver (`tia` device @ 31400 Hz, mono). AUDC/AUDF/AUDV per channel, polynomial noise, sample generation |
| Cart POKEY instances (`rom.cpp`, `bankset.cpp`, `xm.cpp` `device_add_mconfig`) | POKEY @ `$4000` (standard), `@$0450`, `@$0800`, `@$0400` (XM); XM also has **YM2151**. Keep original POKEY behavior; note open question in `rom.cpp:15` ("Are POKEY regs readable somewhere in SG 144K + POKEY homebrew?") — preserve current behavior and log a warning |

### 1.4 CPU / support chips (reuse, do not re-derive)

- `SALLY` = MOS 6502 (`MCFG_CPU_ADD("maincpu", M6502, …)`). Source: `src/cpu/m6502/m6502.*` (upstream MAME file — treat as reference spec for the Rust 6502 core; must be cycle-accurate incl. page-cross penalties, IRQ/NMI/BRK, decimal mode even though 7800 games rarely use it).
- `RIOT 6532` (`src/machine/6532riot.*`): PA/PDIR/DDR, PB, timers (incl. the **Saarna RIOT timer fix** noted in header), edge cases.
- `TIA` (`src/mame/audio/tiaintf.*`, `tiasound.*` + `tiamc1.*`): audio registers + `INPT0–5` paddle/lightgun read path.

### 1.5 Cartridge / slot bus (all must be ported)

Files: `src/devices/bus/a7800/a78_slot.cpp` (943), `a78_slot.h` (177), `a78_carts.h` (58), `rom.cpp` (621), `rom.h` (399), `bankset.cpp` (604), `bankset.h` (273), `xm.cpp` (212), `xm.h` (53), `hiscore.cpp` (72), `hiscore.h` (41), `cpuwiz.cpp` (128), `cpuwiz.h` (76).

PCB/type universe (from `a78_carts.h` `SLOT_INTERFACE` + `rom.h` device types):

1. `a78_rom` — flat, no bankswitch
2. `a78_pokey` — flat + POKEY
3. `a78_sg` — SuperGame bankswitch
4. `a78_sg_pokey` — SG + POKEY
5. `a78_sg_ram` — SG + RAM
6. `a78_sg9` — SG 9×16K banks (note `a78_slot.cpp:74`: `m_bank_mask` non-power-of-2 compensation)
7. `a78_mram` — Mirror RAM
8. `a78_abs` — Absolute bankswitch
9. `a78_act` — Activision bankswitch
10. `a78_hsc` — High Score cart (4K ROM + 2K NVRAM; `$1000–$17FF` + `$3000–$3FFF` handlers)
11. `a78_xm` — XM expansion passthrough (128K RAM + POKEY + **YM2151** + HSC onboard; `$0400–$047F` + `$1000–$17FF` + `$3000–$3FFF` handlers)
12. `a78_bankset_sg`, `a78_bankset_sg_bankram`, `a78_bankset`, `a78_bankset_bankram`, `a78_bankset_52k` (+ banked-RAM Sally/Maria split-view semantics, Sally→Maria writes via `$C000–$FFFF`; see `bankset.cpp:9-14`)
13. `a78_megacart`, `a78_versa` (VersaBoard)
14. `a78_p450_t0/t1/t2/t6/ta` — POKEY @ `$0450` family
15. `a78_p800_t0/t1/t2/t6/ta` — POKEY @ `$0800` family (incl. "non-banked, supergame, and bankset formats" per README)
16. `a78_bankset_p800`, `a78_bankset_sg_p800`, `a78_bankset_sg_bankram_p800`, `a78_bankset_bankram_p800`, `a78_bankset_52k_p4000`, `a78_bankset_p4000` — bankset × POKEY-address matrix
17. `cpuwiz` — CPUWIZ scaffolding (verify whether any shipped ROM selects it; keep the device even if unused)

Header/format handling (`a78_slot.cpp`): 128-byte `.a78` header parse, `validate_header()` quirk corrections (`0x05/0x09/0x11/0x21/0x0c/0x14/0x24/0x18/0x28/0x30` masking, missing-SuperCart-bit repair `head |= 0x02`, Activision `0x100` / Absolute `0x200` detection, `>0x2000` unsupported-mapper path), `A78_POKEY0450 0x40` / `A78_POKEY0800 0x80` bits, `call_load` (softlist vs. raw image, `battery_load` for NVRAM `0xff` fill), `get_cart_type`/`a78_get_pcb_id` mapping.

### 1.6 Controllers (all 9 + port mechanics)

Files: `src/devices/bus/a7800_ctrl/*.cpp/*.h` + `ctrl.cpp`/`ctrl.h` (131).

Slot names (exact, from `ctrl.cpp:80-90`): `proline_joystick` (default both ports), `vcs_joystick`, `paddle`, `lightgun`, `driving_wheel`, `cx22_trakball`, `amiga_mouse`, `st_mouse`, `keypad`.

Per-device behaviors to port (read each `*.cpp`, ~76–147 lines each): `a7800_joy_r/w`, `a7800_pot_x/y_r`, `a7800_light_x/y_r`, `has_pot_x/y`, `is_paddle`, `is_lightgun`, `has_pro_buttons` dual-button logic, paddle discharge/scale math, lightgun beam-race sampling (see driver `tia_r` comments ~lines 248/340/373/415), keypad matrix, wheel gray-code, trakball/mouse quadrature deltas.

Port mechanics (`ctrl.h:93-110`): `joy_r/pot_x_r/pot_y_r/light_x_r/light_y_r/joy_w/exists()` null-safety semantics — an empty slot reads `0xFF` and ignores writes.

### 1.7 Machine variants, clocks, video timing, palettes

- Clocks: `CLK_NTSC 1789772`, `CLK_PAL 1773447` (SALLY; comment: drops to ~1.19 MHz on TIA/RIOT-RAM access — implement as CPU stall cycles, not a clock change).
- Screen (`MACHINE_CONFIG`): NTSC `RAW_PARAMS(CLK*4, 454, 0, 320, 263, 16, 16+243)` / PAL `(…, 313, 16, 16+293)`; visible lines 243/293; `DEFAULT_POSITION(1,0,1,0)`; scanline timer `MCFG_TIMER_DRIVER_ADD_SCANLINE("scantimer", interrupt, "screen", 0, 1)`.
- 8 machines (`CONS` lines): `a7800` (NTSC Warm), `a7800u1` (NTSC Cool), `a7800u2` (NTSC Hot), `a7800dev` (NTSC Developer Mode, `m_devmode_flag=1`), `a7800p` (PAL Warm), `a7800pu1` (PAL Cool), `a7800pu2` (PAL Hot), `a7800pdev` (PAL Developer Mode). `DRIVER_INIT`s set `m_ispal`, `m_lines` (263/313), `m_p1/p2_one_button=1`, devmode flag.
- Palettes: 6 `PALETTE_INIT`s (`a7800/u1/u2/p/pu1/pu2`), each `256*3` `rgb_t` table with phase-shift selections (26.7° Warm default, 26.2/26.7 docs, Cool/Hot alternates); port the **literal RGB triplets** — do not regenerate. Dev machines reuse base palettes + DMA overlay colors (`m_devmode_dmacolor`, width 8 idle / 4 active, `((maria_cycles*0x0f)/428)` scaling).
- BIOS ROM defs: NTSC `7800.u7` @ `$3000` len `$1000` CRC `5d13730c` (optional), proto `c300558-001a.u7` full `$4000` (optional); PAL `7800pal.rom` (optional). **BIOS optional; silence "no ROM loaded" audio** (README) — boot to black/background, not noise.

---

## 2. Feature checklist (every box must be ticked or §9-justified)

**Emulation core**
- [ ] SALLY 6502 cycle-accurate incl. TIA/RIOT slowdown stalls, WSYNC halt, MARIA DMA halt (`m_dmaactive`), IRQ/NMI wiring
- [ ] Address map: `0000–001F=TIA (mirror $300)`, `0020–003F=MARIA (mirror $300)`, `0040–00FF=zpmirror→$2040+`, `0140–01FF=spmirror→$2140+`, `0280–029F=RIOT (mirror $160)`, `0480–04FF=RIOT RAM (mirror $100)`, `1800–1FFF=6116_1 (2K)`, `2000–27FF=6116_2 (4K)`, `4000–BFFF/ C000–FFFF` cart/BIOS routing + dynamic `machine_start` overlays (`0400–04FF` P450, `0800–0FFF` P800, `0400–047F` XM, `1000–17FF/3000–3FFF` HSC/XM, `3000–3FFF` bankset-52K, `4000–7FFF` bankset-P4000 write path)
- [ ] MARIA full register/DLL/DL/DPP/character-base/offset/holey/DMA/mid-scanline-update semantics + all 7 documented modes (160A, 160B, 320A, 320B, 320C, 320D, Kangaroo) + DMA-hole penalties + `DMALIMIT` + dev-mode overlay + VBLANK/WSYNC
- [ ] TIA audio (2ch + noise) @ 31400 Hz mono; TIA reads with paddle-discharge timing (`paddle_start`, `/97` scaler), dual-pot ports, lightgun race + 0.5-cycle penalty path, `tia_delay`
- [ ] RIOT 6532 incl. timer fix + PA joystick mux + PB console/difficulty + pull-up write (`m_p1/p2_one_button`, `m_ctrl_lock`, `m_ctrl_reg`, `m_maria_flag` state)
- [ ] All 17 cart groups (§1.5) + `.a78` header quirks + softlist loading + NVRAM battery semantics + XM control-register updates + bankset Sally/Maria views
- [ ] POKEY @4 variants of base address ($4000/$0400-XM/$0450/$0800) + YM2151 (XM) + supergame/bankset POKEY combinations
- [ ] All 9 controllers × 2 ports + console buttons (Reset `T`, Select `E`, Pause `O` defaults) + difficulty DIPs + one/two-button modes
- [ ] NTSC/PAL clocks + screen timings + 8 machine variants + 6 palettes (literal) + optional BIOS + silent empty-boot

**System / UX (adapt presentation, keep capability)**
- [ ] Load ROM: raw `.a78`/`.bin` + softlist/zip path; headered/headerless detect; mapper auto-select with header-override option; MAME cfg/frontend compat where cheap (document mapping)
- [ ] Save states (all `save_item`s: driver + maria + carts + RIOT/POKEY/TIA state), save slots, NVRAM persistence, pause/throttle/frameskip, snapshots, input remapping incl. analog→digital paddle/mouse scale, per-port device picker
- [ ] Atari-7800-focused streamlined UI (menus, palette/machine picker incl. dev-mode DMA view, screen options) per README
- [ ] `keymaps/`, `ini/` preset equivalents (persisted settings), language/translation hook if Tauri i18n is cheap, debugger/memory-viewer equivalent when dev machine selected

---

## 3. Target architecture (Rust + Tauri 2)

```
a7800-tauri/
  Cargo.toml                      # workspace
  crates/
    a7800-core/                   # no-Tauri, no-std-optional pure emulation (the testable unit)
      src/
        lib.rs                    # Machine facade: new(Config) / reset / step / frame / save_state
        sally.rs                  # 6502 core (port; exhaustive instruction tests)
        maria.rs                  # MARIA (registers, DLL/DL, renderer → 320×~243/293 RGBA framebuffer + DMA stats)
        tia.rs                    # TIA audio + INPT/pot/lightgun read logic
        riot.rs                   # 6532 + timer fix
        pokey.rs                  # POKEY (all base addresses via generic offset)
        ym2151.rs                 # XM YM (port minimal subset actually wired; stub + log unhandled ops)
        bus.rs                    # address map + mirrors + dynamic cart overlays + stall accounting
        cart/
          mod.rs  header.rs       # .a78 128B parse + validate_header quirk-for-quirk
          rom.rs  supergame.rs bankset.rs absolute.rs activision.rs
          mram.rs megacart.rs versa.rs p450.rs p800.rs hiscore.rs xm.rs cpuwiz.rs
        ctrl/
          mod.rs port.rs proline.rs vcs.rs paddles.rs lightgun.rs wheel.rs trakball.rs
          amigamouse.rs stmouse.rs keypad.rs
        palette.rs                # 6 literal 768-entry tables + selector
        timing.rs                 # NTSC/PAL clocks, scanline schedule, interrupt timer
        state.rs                  # serde save-state (versioned) + NVRAM
        socket.rs                 # agent-socket protocol types shared with app layer
    a7800-agent-sock/             # websocket server (also usable standalone for the C++ side, §10)
    src-tauri/                    # Tauri 2 app: commands, audio (cpal), render (wgpu→canvas), input (winit+gilrs), persistence
    app-ui/                       # Tauri frontend ( framework of choice; keep MAME-menu parity: load, machine, palette, ports, throttle, slots, snapshots, dev overlay toggle)
  docs/  tests/  benches/
```

- Frame loop: fixed-step SALLY cycles per scanline → MARIA `startdma`/`draw_scanline` → TIA/POKEY/YM mix @ host rate; never tie emulation quantum to UI fps (run-ahead optional later).
- Rendering: MARIA `scanline[2*pixelx]` semantics → RGBA frame; integer-scale default + smooth-scale option ("enhanced screen options" in README).
- Audio: mono mix TIA+POKEY(+YM) → cpal; keep 31400 Hz emulation rate, resample to device rate; mute-until-ROM behavior preserved.
- Input: keyboard + gamepad + mouse mapped through the same `port.rs` abstraction; lightgun via mouse; paddle via analog axis with `/97`-equivalent scaling exposed as calibration.

---

## 4. Phase plan (agent executes in order)

### Phase 0 — Scaffold + spec lock (0.5–1 day)
1. Create workspace above; `a7800-core` builds with `#![forbid(unsafe_code)]` exception list (only audio/FFI shims may use unsafe with justification).
2. Transcribe **as Rust constants** (with source-line comments): memory map, clocks, screen params, TIA/MARIA/RIOT register maps, controller slot names, cart-type enum, palette tables (generate `palette.rs` from the C++ `rgb_t` blocks via script; verify 768 entries × 6).
3. **Accept:** `cargo test -p a7800-core spec_constants` passes (counts/addresses/hashes asserted); palette entry spot-checks (e.g. Warm hue-0 ramp) match source.

### Phase 1 — CPU + bus + RIOT + TIA-audio skeleton (2–3 days)
- Port 6502 (or adopt a proven MIT/Apache cycle-accurate crate **only if** its instruction/timing tests pass unmodified — else hand-port from `src/cpu/m6502/`).
- Implement `bus.rs` map + mirrors + stall accounting (TIA/RIOT → 1.19 MHz equivalent), `riot.rs` (timers + PA/PB), `tia.rs` audio channels.
- **Accept:** nestest-equivalent 6502 suite green; bus mirror unit tests (write `$0480` ↔ read `$0580`; `$40` ↔ `$2040`); RIOT timer regression test encoding the Saarna fix.

### Phase 2 — MARIA (3–5 days, highest risk)
1. Port registers/DLL/DL walker, direct/indirect modes, all 7 pixel modes, holey logic (`0x9000/0x8800` masks), line-RAM double buffer, `write_line_ram` transparency (`c&3 || kangaroo`), mid-scanline write visibility.
2. DMA cycle counting + hole penalties + `DMALIMIT` + CPU spin (`m_dmaactive`) + WSYNC interplay; dev-mode overlay colors.
3. Renderer → framebuffer + per-scanline DMA stats (feeds dev UI + agent socket).
4. **Accept:** hand-built DLL/DL unit ROMs render expected pixels for each mode; holey addresses skip; Kangaroo transparency; mid-scanline `BACKGRND` change splits the line; DMA-cycle counts within ±1 of C++ on reference traces (see §7).

### Phase 3 — TIA reads / paddles / lightguns (1–2 days)
- Port `tia_r/tia_w` wrappers exactly: `INPT0–5` mux, paddle discharge (`paddle_start`, `/97`), pot vs. digital paths per `has_pot/is_paddle`, lightgun beam-race + 0.5-cycle penalty gate, `tia_delay`.
- **Accept:** scripted-pot test (fixed `pot_x` returns constant), discharge-ramp test (elapsed threshold flips bit), lightgun hit/miss at known beam positions.

### Phase 4 — Controllers (1–2 days)
- Implement all 9 devices behind the `port.rs` trait (`joy/pot_x/pot_y/light_x/light_y/joy_w`, `exists→0xFF` semantics); both ports; `proline` default; dual-button/pull-up (`m_p1/p2_one_button`, `riot_button_pullup_w`); console buttons + difficulty DIPs with default keys T/E/O.
- **Accept:** matrix test: every device × every read returns spec values; unplugged reads `0xFF`; keypad rows; wheel gray sequence; mouse deltas.

### Phase 5 — Cartridges (3–4 days)
1. `header.rs` + `validate_header` quirk-for-quirk (encode each `osd_printf_info` branch as a unit test with synthetic headers).
2. Each PCB module + `machine_start` overlay installer + XM control regs + bankset Sally/Maria split + POKEY-address matrix + HSC/XM NVRAM.
3. Softlist loader honoring `hash/a7800.xml` (NTSC/PAL filter per machine) + raw `.a78`/`.bin`.
4. **Accept:** ROM-loading matrix (one test ROM per mapper incl. SG9 9-bank, bankset ±ram, P450/P800, XM, HSC NVRAM round-trip, headerless detect); write-bank tests assert visible-bank switching.

### Phase 6 — Machine variants / palettes / BIOS / timing (1–2 days)
- 8 configs (clocks, lines 263/313, `m_ispal`, devmode flag, softlist filter), 6 literal palettes + selector, optional-BIOS boot (`m_ctrl_reg=4` cart-flip when vectors are `$FFFF`), silent empty-boot, scanline `interrupt` timer.
- **Accept:** NTSC frame = 263 lines / PAL = 313; `screen` param asserts; palette CRC per table; BIOS-present vs. absent boot vectors.

### Phase 7 — Tauri 2 app shell (3–4 days)
- Commands: `load_rom`, `set_machine`, `set_palette`, `set_port_device`, `keymap`, `pause/throttle/frameskip`, `save_slot/load_slot`, `snapshot`, `nvram_persist`, `dev_overlay_toggle`.
- wgpu/canvas renderer (integer + smooth scale), cpal mono mix, settings persisted to app-data (document MAME-`ini` mapping), 7800-focused menu structure, dev-mode DMA view.
- **Accept:** boots BIOS-less silently; loads every mapper ROM; 60/50 fps throttle stable; state save/load round-trip mid-frame.

### Phase 8 — Agent socket in the new app (1–2 days, mandatory)
- Expose (default `127.0.0.1:7780`, token-gated, localhost-only unless `--agent-bind`):
  - `screen.frame` (RGBA or diff + palette indices + per-line DMA cycles), `screen.scanline` stream @ line rate
  - `input.*` — set ports, press/release/analog, lightgun xy, console buttons
  - `state.*` — CPU regs, MARIA regs/DLL/DL, TIA/RIOT/POKEY snapshots, cart bank, NVRAM hash
  - `perf.*` — fps, frame ms p50/p99, audio underruns, DMA cycles/frame, memory
  - `control.*` — load/reset/pause/step/scanline-step/save/load/snapshot
- JSON-RPC over WebSocket; schema in `crates/a7800-agent-sock/PROTOCOL.md`; fuzz-tested.
- **Accept:** headless script plays a ROM: asserts pixels change, injects input, reads perf — all without the GUI.

### Phase 9 — Validation & 1:1 parity (2–3 days)
- Corpus: at least one ROM per mapper + NTSC/PAL + HSC/XM + each controller fixture + dev-mode ROM.
- Frame-hash parity vs. original (via §10 socket or reference screenshots): N frames per ROM, Hamming/RGBA-hash thresholds documented; audio RMS comparison; input-replay determinism (same script → same hashes).
- Perf: frame-time p50/p99 on Win/macOS/Linux; no Mapper may regress >10% vs. baseline.
- **Accept:** parity report in `docs/PARITY.md` — every checklist item §2 → pass/fail + evidence (hashes, screenshots, traces).

### Phase 10 (optional, if 1:1 automation needed) — Agent socket shim for the original C++ MAME build
- Add a minimal socket to the `a7800-5.2` build (new files only, e.g. `src/mame/drivers/a7800_agentsock.*` + hooks in `a7800.cpp`/`maria.cpp`; guard with `USE_AGENT_SOCKET=1` so default build is untouched): framebuffer tap, input injection at `riot_joystick`/`tia_r` level, state/perf dump.
- Purpose strictly **testing** (not a feature port); reuse `PROTOCOL.md` so the same harness drives both.
- **Accept:** same replay script runs against both builds; comparison tool emits diff report.

---

## 5. Validation harness (build in Phase 8, run in Phase 9)

```
tests/
  roms/            # curated corpus (never commit copyrighted BIOS/ROMs; store hashes + loader notes)
  replays/*.json   # input scripts (deterministic seed + frame-stamped events)
  harness run --target [cpp|rust] --rom X --replay Y --frames N → frames/*.png + hashes.csv + perf.json
  diff-report rom → PASS/FAIL per §2 with thresholds
```

Determinism rules: fixed seed, fixed machine/palette/port config, paused-then-stepped start, NVRAM snapshotted and restored per run.

---

## 6. What "adapt only when necessary" means in practice

| Original | Rust/Tauri equivalent | Why allowed |
|---|---|---|
| MAME OSD/menus/sliders | Tauri menu + web UI | Toolkit requirement; keep every option |
| `screen`/`speaker` devices | wgpu + cpal | Backend requirement; keep rates/timings |
| `ini/` + MAME cfg paths | OS app-data + JSON settings | Platform convention; document mapping |
| C++ `save_item` blobs | `serde` versioned states + NVRAM files | Same coverage, portable format |
| Build (`makefile`, `makefile-osx`, `.travis/.drone`) | Cargo + Tauri bundler + GH Actions | Toolchain requirement |
| Debugger (MAME) | Dev-machine DMA overlay + memory/register inspector panel | Same capability, native UI |

Anything not in this table ships behavior-identical.

---

## 7. Trace points for cycle-level parity (MARIA/DMA)

Instrument both builds (behind flags) to log per-scanline: `vpos, DLL addr, DL bytes, mode bits, width/hpos/pal/ind, hole skips, maria_cycles, CPU stall cycles`. Compare on: Kung-Fu-Master-type line-RAM stress, Jinks `0x02-320D`, Kangaroo-mode, holey direct+indirect, mid-scanline `BACKGRND`/`CHARBASE` writes, VBLANK DMA stop. ±1 cycle tolerance; larger diffs are bugs.

---

## 8. Agent-socket protocol (new app; mirror for §10)

Transport: WebSocket JSON-RPC, localhost default, token auth. Minimum methods: `screen.frame`, `screen.scanline_sub`, `input.set_port`, `input.event`, `input.console`, `state.cpu`, `state.maria`, `state.cart`, `state.nvram_hash`, `perf.sample`, `control.load/reset/pause/step/save/load/snapshot`. Full schema + examples → `crates/a7800-agent-sock/PROTOCOL.md` (Phase 8 deliverable). Must support: real-time pixels, **all** inputs simulated, perf + state monitoring — per the request.

---

## 9. Known gaps / risks

1. `cpuwiz` device purpose is unclear from names alone — read `cpuwiz.*` fully; if dead code, still port the stub so softlists selecting it don't break.
2. `YM2151` in XM: confirm wiring in `xm.cpp` `device_add_mconfig`; several games may only need POKEY — implement audibly-correct YM only for wired paths, stub+log others.
3. `rom.cpp:15` POKEY-visibility question — preserve behavior, add warning log + test pinning current outcome.
4. MAME OSD extras (cheats, artwork, HLSL/BGFX, plugins, `nl_examples`, `benchmarks/`, `regtests/`) are **out of scope** unless an `a7800*` machine path reaches them; list any excluded item in `docs/PARITY.md` with the reaching-path analysis.
5. Frontend framework choice + bundle signing/notarization per OS — decide in Phase 7, not earlier.

---

## 10. Definition of done

- [ ] §2 checklist 100% ticked or §9-justified with evidence.
- [ ] `docs/PARITY.md` parity report green (frame hashes, audio RMS, replay determinism, perf).
- [ ] Agent socket (§8) live with `PROTOCOL.md` + headless replay demo.
- [ ] `cargo test` + `cargo clippy -- -D warnings` + frontend typecheck green on Win/macOS/Linux.
- [ ] No copyrighted ROMs/BIOS in the repo; loader documented with hashes from `hash/a7800.xml` + ROM defs.

*Prepared from: `a7800.cpp` (1509 lines), `maria.*` (532/78), `tiamc1.*` (339/67), `bus/a7800/*` (~2900 lines across slot/cart/bankset/xm/hiscore/rom/cpuwiz), `bus/a7800_ctrl/*` (~1100 lines, 9 devices), `hash/a7800.xml`, machine configs ×8, palettes ×6.*

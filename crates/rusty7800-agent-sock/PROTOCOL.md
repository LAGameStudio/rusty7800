# Rusty7800 Agent Socket — Protocol (Phase 8)

JSON-RPC 2.0 method set for headless control of the emulator (plan §8).
Transport-agnostic core: [`AgentSession::dispatch`](../rusty7800-core/src/socket.rs)
takes a request value and returns a response value — no I/O, no threads.

> **Transport note.** The Tauri shell (later phase) binds this to a
> token-gated WebSocket (default `127.0.0.1:7780`, localhost-only unless
> `--agent-bind`), framing each message as one JSON-RPC object (or a batch
> array). Until then, harnesses call `dispatch` in-process; the method set
> below is identical either way. `screen.scanline` streaming (`scanline_sub`)
> also waits for the transport: poll `screen.scanline` per line for now.

## Envelope

Single request → single response; an array request → an array of responses
in order. `id` is passed through verbatim (`null` when absent).

```json
{"jsonrpc": "2.0", "method": "control.step", "params": {"frames": 1}, "id": 1}
{"jsonrpc": "2.0", "result": {"frames_run": 31, "vpos": 0}, "id": 1}
{"jsonrpc": "2.0", "error": {"code": -32601, "message": "bogus: unknown method"}, "id": 2}
```

Error codes: `-32700` parse (transport level), `-32600` missing method,
`-32601` unknown method, `-32602` bad params / failed load (message says why).

## Methods

### control.*

| Method | Params | Result |
|---|---|---|
| `control.load` | `image`: byte array (raw `.a78`/`.bin`); `machine?`: driver name (`a7800`, `a7800u1`, …, `a7800pdev`), default keeps current; `label?`: string | `{slot, machine}` |
| `control.reset` | — | `{pc}` (soft reset, BIOS/cart detect re-run) |
| `control.pause` | `paused`: bool | `{paused}` (advisory flag for the shell loop) |
| `control.step` | `frames?`: u64, `lines?`: u64 (at least one > 0) | `{frames_run, vpos}` |
| `control.save` | — | `{snapshot}` (byte array, versioned, mid-frame exact) |
| `control.load_snapshot` | `snapshot`: byte array | `{vpos}` — rejects wrong machine/cart with `-32602` |

### screen.*

| Method | Params | Result |
|---|---|---|
| `screen.frame` | — | `{width, height, machine, pens, dma_per_line}` — `pens` is 320×lines palette indices (row-major); `dma_per_line` is MARIA DMA cycles per line of the last completed frame (dev-mode DMA view source) |
| `screen.scanline` | `line`: u64 | `{line, pens}` (320 indices of the last rendered frame) |

Clients map pens → RGB through the machine's 256-pen palette
(`timing::MachineId::palette`).

### input.*

| Method | Params | Result |
|---|---|---|
| `input.set_port` | `port`: 1\|2, `device`: slot name (`proline_joystick`, `vcs_joystick`, `paddle`, `lightgun`, `driving_wheel`, `cx22_trakball`, `keypad`, `amiga_mouse`, `st_mouse`) or `empty` | `{device}` (live slot name) |
| `input.event` | `port`, `control`, `value` (below) | `{joy}` (post-event port byte) |
| `input.console` | `button`: `reset`\|`select`\|`pause`\|`left_dip`\|`right_dip`, `pressed`: bool | `{byte}` (console port byte) |

`input.event` controls per device (wrong-device controls are `-32602`):

| Device | Control → value |
|---|---|
| proline/vcs joystick | `up`/`down`/`left`/`right`/`fire` → bool (proline `fire` = A; also `a`, `b` → bool) |
| paddle | `pot_x`/`pot_y` → 0–255; `btn1`/`btn2` → bool |
| lightgun | `x`/`y` → 0–255 gunsight; `trigger` → bool |
| driving_wheel | `button` → bool; `spin` → −128–127 delta |
| cx22_trakball | `button` → bool; `pos` → `{x, y}` counters |
| amiga/st mouse | `left`/`right` → bool; `move` → `{dx, dy}` |
| keypad | `key` → `{key: 0–11, pressed}` |

Booleans also accept `0`/`1`.

### state.*

| Method | Result |
|---|---|
| `state.cpu` | `{a, x, y, sp, pc, p, cycles, jammed}` |
| `state.maria` | `{dll, dl, offset, dmaon, wsync, vblank, dma_cycles, limit_hit}` |
| `state.riot` | `{timer, pa_out, pb_out, irq}` |
| `state.cart` | `null` (no cart) or `{slot, regs, pokey, ym}` — `regs` holds live bank/control registers (`bank`, `ram_bank`, `cntrl1`–`cntrl5`) |
| `state.nvram_hash` | `{hash}` — FNV-1a hex of battery contents, `null` without battery |
| `state.nvram_image` | `{image}` — raw battery bytes (persistence), `null` without battery |
| `state.input` | `{p1: {device, joy}, p2: {device, joy}, console}` |

### perf.*

| Method | Result |
|---|---|
| `perf.sample` | `{frames_run, cycles_total, dma_last_frame, pending_samples, last_frame_us}` — DMA cycles summed over the last frame; audio samples rendered but not yet drained; wall microseconds of the last stepped frame |

## Determinism rules (replay)

Fixed machine + same image + same stepped counts + same event stream ⇒
bit-identical `pens`, CPU, and MARIA state. `last_frame_us` is the only
non-deterministic field (wall clock, informational only). NVRAM starts
`0xFF`-filled unless a persisted image is supplied at load.

## Example headless session

```json
{"jsonrpc": "2.0", "method": "control.load", "params": {"image": [78, 216, 162, 255, …], "label": "anim"}, "id": 1}
{"jsonrpc": "2.0", "method": "control.step", "params": {"frames": 30}, "id": 2}
{"jsonrpc": "2.0", "method": "screen.frame", "params": {}, "id": 3}
{"jsonrpc": "2.0", "method": "input.event", "params": {"port": 1, "control": "up", "value": true}, "id": 4}
{"jsonrpc": "2.0", "method": "perf.sample", "params": {}, "id": 5}
{"jsonrpc": "2.0", "method": "control.save", "params": {}, "id": 6}
```

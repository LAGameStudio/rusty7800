//! Rusty7800 `socket` — agent-socket protocol (Phase 8 core side).
//!
//! Plan §8 mandates a token-gated agent socket exposing screen, input,
//! state, perf, and control over JSON-RPC. This module is the
//! transport-agnostic core: [`AgentSession`] owns a [`Machine`](crate::machine::Machine)
//! and [`dispatch`]es JSON-RPC 2.0 method calls (single or batch) to
//! plain `serde_json` values. The Tauri shell (later phase) binds this
//! to its WebSocket/TCP transport; headless harnesses call it directly.
//! Method set, schemas, and examples live in
//! `crates/rusty7800-agent-sock/PROTOCOL.md`.
//!
//! Acceptance (plan §8): a headless script loads a ROM, steps frames,
//! observes pixels change, injects input, and reads perf —
//! `tests/phase8_socket.rs` does exactly that through this API.

use serde_json::{json, Value};

use crate::cart::load_image;
use crate::ctrl::PortDevice;
use crate::machine::Machine;
use crate::state::fnv1a;
use crate::tia_io::TiaPort;
use crate::timing::MachineId;

/// JSON-RPC standard errors.
pub const ERR_PARSE: i64 = -32700;
pub const ERR_INVALID_REQUEST: i64 = -32600;
pub const ERR_METHOD_NOT_FOUND: i64 = -32601;
pub const ERR_INVALID_PARAMS: i64 = -32602;

/// Headless agent session: one machine + run bookkeeping.
pub struct AgentSession {
    machine: Machine,
    paused: bool,
    frames_run: u64,
    last_frame_us: u64,
    rom_label: Option<String>,
}

impl AgentSession {
    pub fn new(id: MachineId) -> Self {
        AgentSession {
            machine: Machine::new(id),
            paused: false,
            frames_run: 0,
            last_frame_us: 0,
            rom_label: None,
        }
    }

    pub fn machine(&self) -> &Machine {
        &self.machine
    }

    pub fn machine_mut(&mut self) -> &mut Machine {
        &mut self.machine
    }

    /// Dispatch one JSON-RPC 2.0 request object, or a batch array (each
    /// element answered in order). Malformed envelopes get error replies,
    /// never panics.
    pub fn dispatch(&mut self, req: &Value) -> Value {
        if let Some(batch) = req.as_array() {
            return Value::Array(batch.iter().map(|r| self.dispatch_one(r)).collect());
        }
        self.dispatch_one(req)
    }

    fn dispatch_one(&mut self, req: &Value) -> Value {
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = match req.get("method").and_then(Value::as_str) {
            Some(m) => m,
            None => return Self::err(&id, ERR_INVALID_REQUEST, "missing method"),
        };
        let params = req.get("params").cloned().unwrap_or(Value::Null);
        match self.call(method, &params) {
            Ok(result) => json!({"jsonrpc": "2.0", "result": result, "id": id}),
            Err((code, message)) => {
                Self::err(&id, code, &format!("{method}: {message}"))
            }
        }
    }

    fn err(id: &Value, code: i64, message: &str) -> Value {
        json!({"jsonrpc": "2.0", "error": {"code": code, "message": message}, "id": id})
    }

    /// Method implementations. `Ok(Value)` = result, `Err(code, msg)` =
    /// JSON-RPC error (usually `ERR_INVALID_PARAMS`).
    fn call(&mut self, method: &str, p: &Value) -> Result<Value, (i64, String)> {
        match method {
            "control.load" => {
                let image = get_bytes(p, "image")?;
                let id = match p.get("machine").and_then(Value::as_str) {
                    Some(name) => MachineId::all()
                        .into_iter()
                        .find(|m| m.driver_name() == name)
                        .ok_or_else(|| {
                            (ERR_INVALID_PARAMS, format!("unknown machine {name}"))
                        })?,
                    None => self.machine.machine_id(),
                };
                let loaded = load_image(&image)
                    .map_err(|e| (ERR_INVALID_PARAMS, format!("bad image: {e:?}")))?;
                let slot = loaded.slot_key.to_string();
                self.machine = Machine::new(id);
                self.machine.load_cart(loaded);
                self.machine.reset();
                self.paused = false;
                self.frames_run = 0;
                self.last_frame_us = 0;
                self.rom_label = p
                    .get("label")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                Ok(json!({"slot": slot, "machine": id.driver_name()}))
            }
            "control.reset" => {
                self.machine.reset();
                Ok(json!({"pc": self.machine.sally.pc}))
            }
            "control.pause" => {
                self.paused = get_bool(p, "paused")?;
                Ok(json!({"paused": self.paused}))
            }
            "control.step" => {
                let frames = p.get("frames").and_then(Value::as_u64).unwrap_or(0);
                let lines = p.get("lines").and_then(Value::as_u64).unwrap_or(0);
                if frames == 0 && lines == 0 {
                    return Err((ERR_INVALID_PARAMS, "need frames or lines".into()));
                }
                for _ in 0..lines {
                    self.machine.step_scanline();
                }
                for _ in 0..frames {
                    let t0 = std::time::Instant::now();
                    self.machine.step_frame();
                    self.last_frame_us = t0.elapsed().as_micros() as u64;
                    self.frames_run += 1;
                }
                Ok(json!({"frames_run": self.frames_run, "vpos": self.machine.vpos()}))
            }
            "control.save" => {
                let bytes = serde_json::to_vec(&self.machine.save_snapshot())
                    .map_err(|e| (ERR_INVALID_PARAMS, e.to_string()))?;
                Ok(json!({"snapshot": bytes}))
            }
            "control.load_snapshot" => {
                let bytes = get_bytes(p, "snapshot")?;
                let snap: crate::state::Snapshot = serde_json::from_slice(&bytes)
                    .map_err(|e| (ERR_INVALID_PARAMS, format!("bad snapshot: {e}")))?;
                self.machine
                    .load_snapshot(&snap)
                    .map_err(|e| (ERR_INVALID_PARAMS, e.to_string()))?;
                Ok(json!({"vpos": self.machine.vpos()}))
            }
            "screen.frame" => {
                let (w, h) = self.machine.frame_dims();
                Ok(json!({
                    "width": w,
                    "height": h,
                    "machine": self.machine.machine_id().driver_name(),
                    "pens": self.machine.frame().to_vec(),
                    "dma_per_line": self.machine.dma_per_line().to_vec(),
                }))
            }
            "screen.scanline" => {
                let line = get_u64(p, "line")? as usize;
                let (w, h) = self.machine.frame_dims();
                if line >= h {
                    return Err((ERR_INVALID_PARAMS, format!("line {line} >= {h}")));
                }
                Ok(json!({
                    "line": line,
                    "pens": self.machine.frame()[line * w..(line + 1) * w].to_vec(),
                }))
            }
            "input.set_port" => {
                let port = get_port(p)?;
                let name = get_str(p, "device")?;
                let m = if port == 1 {
                    &mut self.machine.port1
                } else {
                    &mut self.machine.port2
                };
                if name == "empty" {
                    m.unplug();
                } else {
                    m.select_slot(name);
                    if !m.exists() {
                        return Err((ERR_INVALID_PARAMS, format!("unknown device {name}")));
                    }
                }
                Ok(json!({"device": m.device().map(|d| d.slot_name()).unwrap_or("empty")}))
            }
            "input.event" => {
                let port = get_port(p)?;
                let control = get_str(p, "control")?;
                let value = p.get("value").cloned().unwrap_or(Value::Null);
                let m = if port == 1 {
                    &mut self.machine.port1
                } else {
                    &mut self.machine.port2
                };
                apply_event(m, control, &value)?;
                Ok(json!({"joy": m.joy_r()}))
            }
            "input.console" => {
                let button = get_str(p, "button")?;
                let pressed = get_bool(p, "pressed")?;
                let c = &mut self.machine.console;
                match button {
                    "reset" => c.reset = pressed,
                    "select" => c.select = pressed,
                    "pause" => c.pause = pressed,
                    "left_dip" => c.left_b = pressed,
                    "right_dip" => c.right_b = pressed,
                    _ => {
                        return Err((
                            ERR_INVALID_PARAMS,
                            "button: reset|select|pause|left_dip|right_dip".into(),
                        ))
                    }
                }
                Ok(json!({"byte": self.machine.console.byte()}))
            }
            "state.cpu" => {
                let c = &self.machine.sally;
                Ok(json!({
                    "a": c.a, "x": c.x, "y": c.y, "sp": c.sp,
                    "pc": c.pc, "p": c.p, "cycles": c.cycles, "jammed": c.jammed,
                }))
            }
            "state.maria" => {
                let m = &self.machine.maria;
                Ok(json!({
                    "dll": m.dll(), "dl": m.dl(), "offset": m.zone_offset(),
                    "dmaon": m.dmaon(), "wsync": m.wsync_latched(),
                    "vblank": m.vblank(), "dma_cycles": m.dma_cycles(),
                    "limit_hit": m.dma_limit_hit(),
                }))
            }
            "state.riot" => Ok(json!({
                "timer": self.machine.riot.timer_value(),
                "pa_out": self.machine.riot.pa_out,
                "pb_out": self.machine.riot.pb_out,
                "irq": self.machine.riot.irq_asserted,
            })),
            "state.cart" => match self.machine.slot_key() {
                Some(slot) => {
                    let regs: serde_json::Map<String, Value> = self
                        .machine
                        .cart_regs()
                        .into_iter()
                        .map(|(k, v)| (k.to_string(), json!(v)))
                        .collect();
                    Ok(json!({
                        "slot": slot,
                        "regs": regs,
                        "pokey": self.machine.cart_pokey_bases(),
                        "ym": self.machine.cart_ym_present(),
                    }))
                }
                None => Ok(Value::Null),
            },
            "state.nvram_hash" => Ok(json!({
                "hash": self.machine.nvram_image().as_ref().map(|b| format!("{:08x}", fnv1a(b))),
            })),
            "state.nvram_image" => {
                Ok(json!({"image": self.machine.nvram_image()}))
            }
            "state.input" => Ok(json!({
                "p1": {"device": slot_of(&self.machine.port1), "joy": self.machine.port1.joy_r()},
                "p2": {"device": slot_of(&self.machine.port2), "joy": self.machine.port2.joy_r()},
                "console": self.machine.console.byte(),
            })),
            "perf.sample" => Ok(json!({
                "frames_run": self.frames_run,
                "cycles_total": self.machine.sally.cycles,
                "dma_last_frame": self.machine.dma_per_line().iter().sum::<i32>(),
                "pending_samples": self.machine.pending_samples(),
                "last_frame_us": self.last_frame_us,
            })),
            _ => Err((ERR_METHOD_NOT_FOUND, "unknown method".into())),
        }
    }
}

fn slot_of(port: &crate::ctrl::ControlPort) -> &str {
    port.device().map(|d| d.slot_name()).unwrap_or("empty")
}

fn get_port(p: &Value) -> Result<u8, (i64, String)> {
    match p.get("port").and_then(Value::as_u64) {
        Some(1) => Ok(1),
        Some(2) => Ok(2),
        _ => Err((ERR_INVALID_PARAMS, "port must be 1 or 2".into())),
    }
}

fn get_str<'a>(p: &'a Value, name: &str) -> Result<&'a str, (i64, String)> {
    p.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| (ERR_INVALID_PARAMS, format!("missing string param {name}")))
}

fn get_bool(p: &Value, name: &str) -> Result<bool, (i64, String)> {
    match p.get(name) {
        Some(Value::Bool(b)) => Ok(*b),
        Some(Value::Number(n)) => Ok(n.as_u64().unwrap_or(0) != 0),
        _ => Err((ERR_INVALID_PARAMS, format!("missing bool param {name}"))),
    }
}

fn get_u64(p: &Value, name: &str) -> Result<u64, (i64, String)> {
    p.get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| (ERR_INVALID_PARAMS, format!("missing u64 param {name}")))
}

/// Byte blob params: JSON arrays of 0–255 (images, snapshots).
fn get_bytes(p: &Value, name: &str) -> Result<Vec<u8>, (i64, String)> {
    let arr = p
        .get(name)
        .and_then(Value::as_array)
        .ok_or_else(|| (ERR_INVALID_PARAMS, format!("missing byte-array param {name}")))?;
    arr.iter()
        .map(|v| {
            v.as_u64()
                .filter(|n| *n < 256)
                .map(|n| n as u8)
                .ok_or_else(|| (ERR_INVALID_PARAMS, format!("{name} must be bytes"))
                )
        })
        .collect()
}

fn get_level(v: &Value) -> Result<u8, (i64, String)> {
    v.as_u64()
        .filter(|n| *n < 256)
        .map(|n| n as u8)
        .ok_or_else(|| (ERR_INVALID_PARAMS, "level must be 0-255".into()))
}

fn get_delta(v: &Value) -> Result<i8, (i64, String)> {
    v.as_i64()
        .filter(|n| (-128i64..=127i64).contains(n))
        .map(|n| n as i8)
        .ok_or_else(|| (ERR_INVALID_PARAMS, "delta must be -128..=127".into()))
}

/// Route one `input.event` onto the live port device. Control names are
/// per device family (see PROTOCOL.md); mismatches are param errors.
fn apply_event(
    port: &mut crate::ctrl::ControlPort,
    control: &str,
    value: &Value,
) -> Result<(), (i64, String)> {
    let dev = port
        .device_mut()
        .ok_or_else(|| (ERR_INVALID_PARAMS, "port is empty".to_string()))?;
    match dev {
        PortDevice::Proline(d) => match control {
            "up" => d.up = flag(value)?,
            "down" => d.down = flag(value)?,
            "left" => d.left = flag(value)?,
            "right" => d.right = flag(value)?,
            "fire" | "a" => d.btn_a = flag(value)?,
            "b" => d.btn_b = flag(value)?,
            _ => return Err(bad_control(control)),
        },
        PortDevice::Vcs(d) => match control {
            "up" => d.up = flag(value)?,
            "down" => d.down = flag(value)?,
            "left" => d.left = flag(value)?,
            "right" => d.right = flag(value)?,
            "fire" => d.button = flag(value)?,
            _ => return Err(bad_control(control)),
        },
        PortDevice::Paddles(d) => match control {
            "pot_x" => d.pot_x = get_level(value)?,
            "pot_y" => d.pot_y = get_level(value)?,
            "btn1" => d.btn1 = flag(value)?,
            "btn2" => d.btn2 = flag(value)?,
            _ => return Err(bad_control(control)),
        },
        PortDevice::LightGun(d) => match control {
            "x" => d.light_x = get_level(value)?,
            "y" => d.light_y = get_level(value)?,
            "trigger" => d.trigger = flag(value)?,
            _ => return Err(bad_control(control)),
        },
        PortDevice::Wheel(d) => match control {
            "button" => d.button = flag(value)?,
            "spin" => d.spin(get_delta(value)?),
            _ => return Err(bad_control(control)),
        },
        PortDevice::Trakball(d) => match control {
            "button" => d.button = flag(value)?,
            "pos" => {
                let x = value
                    .get("x")
                    .ok_or_else(|| (ERR_INVALID_PARAMS, "pos needs x".into()))
                    .and_then(get_level_ref)?;
                let y = value
                    .get("y")
                    .ok_or_else(|| (ERR_INVALID_PARAMS, "pos needs y".into()))
                    .and_then(get_level_ref)?;
                d.set_position(x, y);
            }
            _ => return Err(bad_control(control)),
        },
        PortDevice::AmigaMouse(d) => match control {
            "left" => d.btn_left = flag(value)?,
            "right" => d.btn_right = flag(value)?,
            "move" => {
                let (dx, dy) = get_move(value)?;
                d.move_by(dx, dy);
            }
            _ => return Err(bad_control(control)),
        },
        PortDevice::StMouse(d) => match control {
            "left" => d.btn_left = flag(value)?,
            "right" => d.btn_right = flag(value)?,
            "move" => {
                let (dx, dy) = get_move(value)?;
                d.move_by(dx, dy);
            }
            _ => return Err(bad_control(control)),
        },
        PortDevice::Keypad(d) => match control {
            "key" => {
                let key = value
                    .get("key")
                    .and_then(Value::as_u64)
                    .filter(|k| *k < 12)
                    .ok_or_else(|| (ERR_INVALID_PARAMS, "key must be 0-11".into()))?
                    as u8;
                let pressed = value
                    .get("pressed")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| {
                        (ERR_INVALID_PARAMS, "key needs pressed".to_string())
                    })?;
                if pressed {
                    d.press(key);
                } else {
                    d.release(key);
                }
            }
            _ => return Err(bad_control(control)),
        },
    }
    Ok(())
}

fn flag(value: &Value) -> Result<bool, (i64, String)> {
    match value {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) => Ok(n.as_u64().unwrap_or(0) != 0),
        _ => Err((ERR_INVALID_PARAMS, "boolean expected".into())),
    }
}

fn bad_control(control: &str) -> (i64, String) {
    (ERR_INVALID_PARAMS, format!("bad control {control} for this device"))
}

fn get_level_ref(v: &Value) -> Result<u8, (i64, String)> {
    get_level(v)
}

fn get_move(v: &Value) -> Result<(i8, i8), (i64, String)> {
    let dx = v
        .get("dx")
        .ok_or_else(|| (ERR_INVALID_PARAMS, "move needs dx".into()))
        .and_then(get_delta_ref)?;
    let dy = v
        .get("dy")
        .ok_or_else(|| (ERR_INVALID_PARAMS, "move needs dy".into()))
        .and_then(get_delta_ref)?;
    Ok((dx, dy))
}

fn get_delta_ref(v: &Value) -> Result<i8, (i64, String)> {
    get_delta(v)
}

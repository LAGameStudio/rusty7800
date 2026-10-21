//! Rusty7800 web build: `wasm-bindgen` API over
//! [`rusty7800_core`](rusty7800_core).
//!
//! The page (`site/`) owns the frame loop: it calls
//! [`Emu::step_frames`], then [`Emu::update_rgba`] and blits the RGBA
//! buffer (see [`Emu::rgba_ptr`] / [`Emu::rgba_len`]) into a canvas.
//! Machine construction takes a driver name (`a7800`, `a7800p`, …;
//! see `MachineId::driver_name`).

use rusty7800_core::cart::load_image;
use rusty7800_core::ctrl::PortDevice;
use rusty7800_core::machine::Machine;
use rusty7800_core::state::fnv1a;
use rusty7800_core::timing::MachineId;
use wasm_bindgen::prelude::*;

/// One headless session (mirrors `AgentSession`, minus transport).
#[wasm_bindgen]
pub struct Emu {
    machine: Machine,
    rgba: Vec<u8>,
}

#[wasm_bindgen]
impl Emu {
    /// Create a session for a machine driver name (default `a7800`).
    #[wasm_bindgen(constructor)]
    pub fn new(machine: &str) -> Result<Emu, JsValue> {
        let id = lookup_machine(machine).ok_or_else(|| JsValue::from_str("unknown machine"))?;
        let (w, h) = (320usize, id.spec().lines as usize);
        Ok(Emu {
            machine: Machine::new(id),
            rgba: vec![0; w * h * 4],
        })
    }

    /// Load a `.a78`/`.bin` image; returns the slot key. Resets the machine.
    pub fn load(&mut self, image: &[u8]) -> Result<String, JsValue> {
        let loaded = load_image(image).map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
        let slot = loaded.slot_key.to_string();
        self.machine.load_cart(loaded);
        self.machine.reset();
        Ok(slot)
    }

    /// Place a BIOS file (4K images at $3000, full 16K at $0000).
    pub fn load_bios(&mut self, image: &[u8]) {
        let off = if image.len() <= 0x1000 {
            0x3000
        } else {
            0x0000
        };
        self.machine.load_bios(image, off);
    }

    pub fn reset(&mut self) {
        self.machine.reset();
    }

    /// Drive port-1 joystick state (arrows + Z/X in the page shell).
    /// Proline sets all six inputs; VCS maps `a` onto its single fire
    /// button (`b` ignored); other devices are left untouched.
    pub fn set_p1(&mut self, up: bool, down: bool, left: bool, right: bool, a: bool, b: bool) {
        match self.machine.port1.device_mut() {
            Some(PortDevice::Proline(d)) => {
                d.up = up;
                d.down = down;
                d.left = left;
                d.right = right;
                d.btn_a = a;
                d.btn_b = b;
            }
            Some(PortDevice::Vcs(d)) => {
                d.up = up;
                d.down = down;
                d.left = left;
                d.right = right;
                d.button = a;
            }
            _ => {}
        }
    }

    /// Same as [`Emu::set_p1`] for port 2 (WASD + G/H in the page shell).
    pub fn set_p2(&mut self, up: bool, down: bool, left: bool, right: bool, a: bool, b: bool) {
        match self.machine.port2.device_mut() {
            Some(PortDevice::Proline(d)) => {
                d.up = up;
                d.down = down;
                d.left = left;
                d.right = right;
                d.btn_a = a;
                d.btn_b = b;
            }
            Some(PortDevice::Vcs(d)) => {
                d.up = up;
                d.down = down;
                d.left = left;
                d.right = right;
                d.button = a;
            }
            _ => {}
        }
    }

    /// Console buttons: Reset (T), Select (E), Pause (O) in the page shell.
    pub fn set_console(&mut self, reset: bool, select: bool, pause: bool) {
        self.machine.console.reset = reset;
        self.machine.console.select = select;
        self.machine.console.pause = pause;
    }

    /// Live port-1 JOY byte (host-testable input plumbing check).
    pub fn joy_p1(&self) -> u8 {
        use rusty7800_core::tia_io::TiaPort;
        self.machine.port1.joy_r()
    }

    /// Console byte (host-testable input plumbing check).
    pub fn console_byte(&self) -> u8 {
        self.machine.console.byte()
    }

    /// Core render rate (TIA_AUDIO_CLOCK, 31400 Hz); the page resamples
    /// to the device rate like the desktop `Pump`.
    pub fn audio_rate(&self) -> u32 {
        self.machine.sample_rate()
    }

    /// Silent-boot mute (no cart/BIOS yet); page may skip playback.
    pub fn muted(&self) -> bool {
        self.machine.muted()
    }

    /// TV region declared by the loaded ROM header (`undefined` for
    /// headerless images, which claim no region). The page warns when
    /// this disagrees with the machine: PAL-timed games hang on a
    /// 263-line machine and vice versa.
    pub fn rom_is_pal(&self) -> Option<bool> {
        self.machine.rom_is_pal()
    }

    /// True when the loaded ROM declares a TV region and it disagrees
    /// with this session's machine.
    pub fn region_mismatch(&self) -> bool {
        self.machine.region_mismatch()
    }

    /// Drain one frame's mono `i16` samples (surfaced as `Int16Array`).
    /// Call once per `step_frames` batch or samples accumulate.
    pub fn take_audio(&mut self) -> Vec<i16> {
        self.machine.take_audio()
    }

    pub fn step_frames(&mut self, n: u32) {
        for _ in 0..n {
            self.machine.step_frame();
        }
    }

    pub fn width(&self) -> usize {
        self.machine.frame_dims().0
    }

    pub fn height(&self) -> usize {
        self.machine.frame_dims().1
    }

    pub fn pc(&self) -> u16 {
        self.machine.sally.pc
    }

    pub fn frame_hash(&self) -> u32 {
        fnv1a(self.machine.frame())
    }

    /// Re-render pens through the machine palette into the RGBA buffer.
    pub fn update_rgba(&mut self) {
        let pal = self.machine.machine_id().palette();
        let (w, h) = self.machine.frame_dims();
        self.rgba.resize(w * h * 4, 0);
        for (dst, pen) in self
            .rgba
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(self.machine.frame())
        {
            let (r, g, b) = pal[*pen as usize];
            dst[0] = r;
            dst[1] = g;
            dst[2] = b;
            dst[3] = 0xFF;
        }
    }

    /// Pointer + length of the RGBA buffer (read via wasm memory).
    pub fn rgba_ptr(&self) -> *const u8 {
        self.rgba.as_ptr()
    }

    pub fn rgba_len(&self) -> usize {
        self.rgba.len()
    }
}

/// The wasm linear memory (the page wraps it in a typed array around
/// [`Emu::rgba_ptr`]; re-fetch every frame — growth detaches buffers).
#[wasm_bindgen]
pub fn wasm_memory() -> JsValue {
    wasm_bindgen::memory()
}

/// Plain-Rust driver lookup behind [`Emu::new`] (host-testable: the
/// `JsValue` error construction is a wasm-only stub off-target).
fn lookup_machine(name: &str) -> Option<MachineId> {
    MachineId::all()
        .into_iter()
        .find(|m| m.driver_name() == name)
}

/// Machine driver names for the page's picker.
#[wasm_bindgen]
pub fn machine_list() -> Vec<String> {
    MachineId::all()
        .iter()
        .map(|m| m.driver_name().to_string())
        .collect()
}

/// Friendly labels parallel to [`machine_list`] (original A7800 `full_name`:
/// `Atari 7800 PAL Warm`, …) so the picker shows the region.
#[wasm_bindgen]
pub fn machine_labels() -> Vec<String> {
    MachineId::all()
        .iter()
        .map(|m| m.spec().full_name.to_string())
        .collect()
}

/// Machine region by driver name (`undefined` for unknown names).
/// Lets the page name the mismatch without hardcoding regions.
#[wasm_bindgen]
pub fn machine_is_pal(name: &str) -> Option<bool> {
    lookup_machine(name).map(|m| m.spec().is_pal)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The web API logic is exercised on the host (no JS imports here),
    /// so this test really runs the code the page calls.
    #[test]
    fn emu_boots_and_renders() {
        let mut img = vec![0xFFu8; 0x1000];
        img[0] = 0x4C;
        img[1] = 0x00;
        img[2] = 0xF0; // JMP $F000
        img[0xFFA] = 0x00;
        img[0xFFB] = 0xF0;
        img[0xFFC] = 0x00;
        img[0xFFD] = 0xF0;
        img[0xFFE] = 0x00;
        img[0xFFF] = 0xF0;
        let mut e = Emu::new("a7800").unwrap();
        assert_eq!(e.load(&img).unwrap(), "a78_rom");
        e.step_frames(2);
        assert_eq!(e.pc(), 0xF000);
        assert_eq!((e.width(), e.height()), (320, 263));
        e.update_rgba();
        assert_eq!(e.rgba_len(), 320 * 263 * 4);
        assert!(!e.rgba_ptr().is_null());
        assert!(machine_list().contains(&"a7800p".to_string()));
        assert!(lookup_machine("bogus").is_none());
        assert_eq!(machine_list().len(), machine_labels().len());
        assert!(machine_labels().iter().any(|l| l.contains("PAL")));
        assert_eq!(machine_is_pal("a7800p"), Some(true));
        assert_eq!(machine_is_pal("a7800"), Some(false));
        assert_eq!(machine_is_pal("bogus"), None);
        // Headerless image claims no region; headers report theirs.
        assert_eq!(e.rom_is_pal(), None);
        assert!(!e.region_mismatch());
        assert_eq!(lookup_machine("a7800p"), Some(MachineId::Pal));
        // Input plumbing: Proline idle byte is 0xAF; pressing up clears bit 0.
        assert_eq!(e.joy_p1(), 0xAF);
        e.set_p1(true, false, false, false, false, false);
        assert_eq!(e.joy_p1(), 0xAE);
        e.set_p1(false, false, false, false, true, true);
        assert_eq!(e.joy_p1(), 0xFF);
        e.set_console(true, false, false);
        assert_eq!(e.console_byte() & 0x01, 0);
        // Audio plumbing: rate is the TIA clock; a stepped frame yields
        // samples (silence until registers are programmed, but nonzero
        // in count), and take drains the queue.
        assert_eq!(e.audio_rate(), 31_400);
        e.take_audio(); // drain the two boot frames stepped above
        e.step_frames(1);
        let n = e.take_audio().len();
        assert!(n > 400 && n < 700, "samples/frame {n}");
        assert!(e.take_audio().is_empty());
    }
}

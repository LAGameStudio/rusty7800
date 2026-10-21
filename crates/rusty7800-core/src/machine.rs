//! Rusty7800 `machine` — the executable 7800.
//!
//! Wires Sally + Maria + TIA + RIOT + cartridge into one frame-stepping
//! machine against `a7800-5.2/src/mame/drivers/a7800.cpp` (BSD-3-Clause):
//! the static `a7800_mem` map (TIA `0x0000–0x001F`, MARIA
//! `0x0020–0x003F`, mirrors, RIOT, RIOT RAM, SRAM, cart `40xx`, BIOS-or-cart
//! `C000+`), the `machine_start` dynamic cart overlays (already encoded in
//! `cart::overlays_for_type`), `machine_reset` BIOS detection, the
//! per-scanline `interrupt` → `maria_startdma` sequence, `bios_or_cart_r`,
//! and the RIOT/TIA port callbacks.
//!
//! Timing model: one scanline is exactly [`CPU_HALVES_PER_LINE`] CPU
//! half-cycles (454 pixel clocks, NTSC and PAL). Each line runs MARIA DMA
//! first (SALLY frozen `(dma_cycles+3)/4` cycles, like MAME's
//! `spin_until_time`), then SALLY instructions until the budget is spent.
//! A DMALIMIT halt or a latched WSYNC freezes the CPU to HSYNC (RDY-style:
//! cycles burn, no bus activity), matching MAME's `spin_until_trigger`.
//! TIA-register and RIOT-RAM accesses cost 3 halves (the driver's "1.19
//! MHz" slowdown); everything else costs 2. Slow extras are reconciled
//! into `Sally::cycles` at line end so the CPU counter stays honest.
//!
//! Not yet modeled: POKEY/YM2151 chips (cart `CartBus` reads `0xFF`,
//! writes ignored — both are still Phase-0 stubs), and the lightgun
//! 0.5-cycle stall gate beyond bus-half accounting (paddle/beam timing
//! uses the exact bus clock, so only sub-instruction CPU phasing drifts
//! while a gun is plugged).

use crate::bus::{self, Memory, Region};
use crate::cart::{self, Cart, CartBus, LoadedCart, Overlay};
use crate::ctrl::{console, ConsoleState, ControlPort};
use crate::maria::{Maria, MariaBus, LINE_W};
use crate::riot::Riot;
use crate::sally::{CpuBus, Sally};
use crate::tia::TiaAudio;
use crate::tia_io::{BeamState, TiaIo, TiaPort};
use crate::state::{Snapshot, SnapshotError, SavedCart};
use crate::timing::{self, BiosImage, MachineId, ScreenGeometry};

/// TIA audio chip clock (`MCFG_SOUND_TIA_ADD("tia", 31400)`); also the
/// default sample rate (driver-identical, the shell resamples).
pub const TIA_AUDIO_CLOCK: u32 = 31_400;
/// CPU half-cycles per scanline: 454 pixel clocks = 227 halves, exact on
/// every line, NTSC and PAL (shared `HTOTAL`).
pub const CPU_HALVES_PER_LINE: u64 = 227;
/// Framebuffer width in half-pixels (160 MARIA cells × 2).
pub const FRAME_W: usize = 320;

/// The executable machine: devices, bus state, raster position, and the
/// current frame (256-pen indices, one byte per half-pixel) plus the
/// current frame's mono audio samples.
pub struct Machine {
    id: MachineId,
    /// SALLY clock; also drives paddle/beam timing via the bus clock.
    pub sally: Sally,
    pub maria: Maria,
    pub tia_io: TiaIo,
    pub audio: TiaAudio,
    pub riot: Riot,
    pub mem: Memory,
    pub bios: BiosImage,
    pub port1: ControlPort,
    pub port2: ControlPort,
    pub console: ConsoleState,
    cart: Option<Box<dyn Cart>>,
    overlays: Vec<Overlay>,
    slot_key: Option<&'static str>,
    pcb_id: Option<i32>,
    /// TV region declared by the loaded `.a78` header (byte 57):
    /// `Some(true)` = PAL, `Some(false)` = NTSC, `None` = headerless
    /// image (no region claim). The shells surface a mismatch warning
    /// when this disagrees with the machine: PAL-timed games re-init
    /// every frame on a 263-line machine (verified: Fatal Run PAL sits
    /// at its boot loop on `a7800`, runs on `a7800p`).
    rom_tv_pal: Option<bool>,
    vpos: u16,
    lines: u16,
    is_pal: bool,
    frame: Vec<u8>,
    samples: Vec<i16>,
    sample_rate: u32,
    samples_per_line: f64,
    audio_frac: f64,
    /// Absolute bus half-cycles since reset (paddle/beam epoch).
    clock_halves: u64,
    /// In-progress frame's per-line DMA cycles (exact mid-frame resume).
    dma_accum: Vec<i32>,
    /// Last completed frame's per-line DMA cycles (`screen.frame` source).
    dma_frame: Vec<i32>,
}

impl Machine {
    pub fn new(id: MachineId) -> Self {
        Self::with_sample_rate(id, TIA_AUDIO_CLOCK)
    }

    pub fn with_sample_rate(id: MachineId, sample_rate: u32) -> Self {
        let spec = id.spec();
        let lines = spec.lines as u16;
        let geo = ScreenGeometry::for_machine(id);
        Machine {
            id,
            sally: Sally::new(),
            maria: Maria::new(),
            tia_io: TiaIo::new(),
            audio: TiaAudio::new(TIA_AUDIO_CLOCK, sample_rate),
            riot: Riot::new(),
            mem: Memory::new(),
            bios: BiosImage::new(),
            port1: ControlPort::new(),
            port2: ControlPort::new(),
            console: ConsoleState::new(),
            cart: None,
            overlays: Vec::new(),
            slot_key: None,
            vpos: 0,
            lines,
            is_pal: spec.is_pal,
            frame: vec![0; FRAME_W * lines as usize],
            samples: Vec::new(),
            sample_rate,
            samples_per_line: sample_rate as f64
                / geo.frame_rate_hz(id.clock_hz())
                / lines as f64,
            audio_frac: 0.0,
            clock_halves: 0,
            dma_accum: Vec::new(),
            dma_frame: Vec::new(),
            pcb_id: None,
            rom_tv_pal: None,
        }
    }

    pub fn machine_id(&self) -> MachineId {
        self.id
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Install a loaded image (header or headerless); overlays follow the
    /// `machine_start` switch via `cart::overlays_for_type`.
    pub fn load_cart(&mut self, loaded: LoadedCart) {
        let LoadedCart { cart, pcb_id, slot_key, header, .. } = loaded;
        self.overlays = cart::overlays_for_type(pcb_id);
        self.slot_key = Some(slot_key);
        self.pcb_id = Some(pcb_id);
        self.rom_tv_pal = header.map(|h| h.is_pal);
        self.cart = Some(cart);
    }

    /// Place a BIOS file at its `ROM_LOAD` offset (`7800.u7`: 4K at
    /// `0x3000`; full 16K images at `0x0000`).
    pub fn load_bios(&mut self, file: &[u8], offset: usize) {
        self.bios.place(file, offset);
    }

    pub fn has_cart(&self) -> bool {
        self.cart.is_some()
    }

    pub fn slot_key(&self) -> Option<&'static str> {
        self.slot_key
    }

    /// TV region declared by the loaded ROM header (`None` = headerless,
    /// no claim). Compare against `machine_id().spec().is_pal` to
    /// detect a region mismatch.
    pub fn rom_is_pal(&self) -> Option<bool> {
        self.rom_tv_pal
    }

    /// True when the loaded ROM declares a TV region and it disagrees
    /// with the machine (PAL game on a 263-line machine or vice versa).
    /// Mismatched games misbehave badly (Fatal Run PAL re-runs its boot
    /// loop forever on `a7800`); the shells warn instead of guessing.
    pub fn region_mismatch(&self) -> bool {
        matches!(self.rom_tv_pal, Some(pal) if pal != self.is_pal)
    }

    /// Silent-boot mute: the audio backend mutes unless a cart or BIOS is
    /// present (`timing::audio_muted`).
    pub fn muted(&self) -> bool {
        timing::audio_muted(self.has_cart(), self.bios.present)
    }

    pub fn ctrl_reg(&self) -> u8 {
        self.tia_io.ctrl_reg
    }

    pub fn vpos(&self) -> u16 {
        self.vpos
    }

    /// Soft reset (`machine_reset`): control register zeroed, then flipped
    /// to cart when the boot vectors are blank; SALLY reset-vector fetch
    /// runs through the bus (BIOS or cart). RAM and RIOT survive (MAME
    /// `machine_reset` does not touch devices); `new()` is power-on.
    pub fn reset(&mut self) {
        self.tia_io.reset();
        self.maria.reset();
        self.maria.devmode_flag = self.id.spec().devmode;
        if let Some(cart) = self.cart.as_mut() {
            cart.reset();
        }
        self.tia_io.ctrl_reg = 0;
        let mut bus = MachineBus {
            maria: Some(&mut self.maria),
            mstat: 0,
            mem: &mut self.mem,
            tia: &mut self.tia_io,
            audio: &mut self.audio,
            riot: &mut self.riot,
            cart: self.cart.as_mut().map(|c| c as &mut Box<dyn Cart>),
            overlays: &self.overlays,
            bios: &self.bios,
            port1: &mut self.port1,
            port2: &mut self.port2,
            console: &self.console,
            beam: BeamState { hpos: 0, vpos: 0, lines: self.lines, is_pal: self.is_pal },
            base_halves: 0,
            halves: 0,
            accesses: 0,
        };
        let top_fffe = bus.read_sally(0xFFFE);
        let top_ffff = bus.read_sally(0xFFFF);
        bus.tia.ctrl_reg = timing::reset_ctrl_reg(top_fffe, top_ffff);
        self.sally.reset(&mut bus);
        self.sally.set_irq(false);
        self.vpos = 0;
        self.frame.fill(0);
        self.samples.clear();
        self.audio_frac = 0.0;
        self.clock_halves = self.sally.cycles;
        self.dma_accum.clear();
        self.dma_frame.clear();
    }

    /// Run one scanline: `interrupt` housekeeping, MARIA DMA (SALLY
    /// frozen), SALLY to the half-cycle budget (WSYNC/limit freezes burn
    /// to HSYNC), RIOT tick, pixel render, per-line audio.
    pub fn step_scanline(&mut self) {
        let lines = self.lines;
        let vpos = self.vpos;
        self.maria.interrupt(vpos, lines);
        let mut bus = MachineBus {
            maria: Some(&mut self.maria),
            mstat: 0,
            mem: &mut self.mem,
            tia: &mut self.tia_io,
            audio: &mut self.audio,
            riot: &mut self.riot,
            cart: self.cart.as_mut().map(|c| c as &mut Box<dyn Cart>),
            overlays: &self.overlays,
            bios: &self.bios,
            port1: &mut self.port1,
            port2: &mut self.port2,
            console: &self.console,
            beam: BeamState { hpos: 0, vpos, lines, is_pal: self.is_pal },
            base_halves: self.clock_halves,
            halves: 0,
            accesses: 0,
        };
        bus.mstat = bus.maria.as_deref().unwrap().vblank();
        let maria = bus.maria.take().unwrap();
        let (dma_cycles, halt, nmi) = maria.startdma(&mut bus, vpos, lines);
        // DMA done: hand MARIA back so SALLY register access works.
        bus.maria = Some(maria);
        self.dma_accum.push(dma_cycles);
        if nmi {
            self.sally.set_nmi();
        }
        // DMA owns the bus: SALLY burns `(cycles+3)/4`, like MAME's
        // `spin_until_time`.
        let stall_halves = (dma_cycles.max(0) as u64).div_ceil(4) * 2;
        bus.halves += stall_halves;
        let mut frozen_added: u64 = 0;
        let mut jam_ticks: u64 = 0;
        if halt {
            frozen_added = CPU_HALVES_PER_LINE.saturating_sub(bus.halves);
        } else {
            loop {
                if bus.maria.as_deref().unwrap().wsync_latched() {
                    frozen_added = CPU_HALVES_PER_LINE.saturating_sub(bus.halves);
                    break;
                }
                let h0 = bus.halves;
                self.sally.step(&mut bus);
                if bus.halves == h0 {
                    // KIL-jammed (or otherwise access-free): burn the
                    // instruction's cycle on the bus clock too.
                    bus.halves += 2;
                    jam_ticks += 1;
                }
                self.sally.set_irq(bus.riot.irq_asserted);
                if bus.halves >= CPU_HALVES_PER_LINE {
                    break;
                }
            }
        }
        bus.halves += frozen_added;
        self.sally.cycles += frozen_added / 2;
        // Reconcile the 1.19 MHz slow-access extras (TIA regs, RIOT RAM)
        // into SALLY's counter; DMA/freeze/jam halves are already exact.
        let cpu_halves = bus.halves.saturating_sub(stall_halves + frozen_added);
        let slow_extra = cpu_halves
            .saturating_sub(2 * bus.accesses + 2 * jam_ticks);
        self.sally.cycles += slow_extra / 2;
        bus.riot.tick(bus.halves / 2);
        // Blank scanlines outside the display window read background
        // (maria.cpp: "Blank scanlines are drawn when DMA is off").
        let maria = bus.maria.take().unwrap();
        if !(vpos > 15 && vpos < lines - 5) {
            maria.blank_displayed_line();
        }
        let row_base = vpos as usize * FRAME_W;
        for i in 0..LINE_W {
            let px = maria.render_pixel(i);
            self.frame[row_base + 2 * i] = px.left;
            self.frame[row_base + 2 * i + 1] = px.right;
        }
        bus.maria = Some(maria);
        // Per-line audio slice (chip state persists across lines, so
        // mid-frame register writes take effect mid-frame).
        self.audio_frac += self.samples_per_line;
        let mut n = self.audio_frac as usize;
        self.audio_frac -= n as f64;
        n = n.min(8);
        let mut tmp = [0i16; 8];
        bus.audio.render(&mut tmp[..n]);
        self.samples.extend_from_slice(&tmp[..n]);
        self.clock_halves += bus.halves;
        self.vpos = (vpos + 1) % lines;
        if self.vpos == 0 {
            self.dma_frame = std::mem::take(&mut self.dma_accum);
        }
    }

    /// Run a full frame (`lines` scanlines); `frame()` / `take_audio()`
    /// collect the output.
    pub fn step_frame(&mut self) {
        for _ in 0..self.lines {
            self.step_scanline();
        }
    }

    /// Current framebuffer: `lines` rows of 320 256-pen indices.
    pub fn frame(&self) -> &[u8] {
        &self.frame
    }

    pub fn frame_dims(&self) -> (usize, usize) {
        (FRAME_W, self.lines as usize)
    }

    /// Drain the current frame's mono audio samples.
    pub fn take_audio(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.samples)
    }
}

/// The CPU/DMA bus: one struct serves SALLY (`CpuBus`), MARIA DMA
/// (`MariaBus`), and the cart POKEY/YM hooks (`CartBus`). `maria` is
/// `take()`n out during `startdma` (DMA register reads use the `mstat`
/// snapshot, exactly what `Maria::read` would return); `cart` is
/// `take()`n per access so the cart and the bus never alias.
struct MachineBus<'a> {
    maria: Option<&'a mut Maria>,
    mstat: u8,
    mem: &'a mut Memory,
    tia: &'a mut TiaIo,
    audio: &'a mut TiaAudio,
    riot: &'a mut Riot,
    cart: Option<&'a mut Box<dyn Cart>>,
    overlays: &'a [Overlay],
    bios: &'a BiosImage,
    port1: &'a mut ControlPort,
    port2: &'a mut ControlPort,
    console: &'a ConsoleState,
    beam: BeamState,
    /// Absolute halves at line start (paddle/beam epoch).
    base_halves: u64,
    halves: u64,
    accesses: u64,
}

impl MachineBus<'_> {
    /// Absolute CPU cycles on the bus clock (slow-access exact).
    fn cpu_cycles(&self) -> u64 {
        (self.base_halves + self.halves) / 2
    }

    /// Static-map mirrors: TIA/MARIA `0x300` mirrors, the RIOT `0x160`
    /// aliases, and the RIOT-RAM `0x100` mirror (`a7800_mem`).
    fn canonical(addr: u16) -> u16 {
        match addr {
            0x0300..=0x033F => addr - 0x0300,
            0x02A0..=0x02BF
            | 0x02C0..=0x02DF
            | 0x02E0..=0x02FF
            | 0x0380..=0x039F
            | 0x03A0..=0x03BF
            | 0x03C0..=0x03DF
            | 0x03E0..=0x03FF => 0x0280 + (addr & 0x1F),
            0x0580..=0x05FF => addr - 0x0100,
            _ => addr,
        }
    }

    fn overlay_at(&self, addr: u16) -> Option<Overlay> {
        self.overlays.iter().find(|o| addr >= o.start && addr <= o.end).copied()
    }

    /// Run `op` against the installed cart, restoring it after (works
    /// with no cart: returns `default`).
    fn cart_op<R>(
        &mut self,
        default: R,
        op: impl FnOnce(&mut Box<dyn Cart>, &mut Self) -> R,
    ) -> R {
        let mut slot = self.cart.take();
        let v = match slot.as_mut() {
            Some(k) => op(k, self),
            None => default,
        };
        self.cart = slot;
        v
    }

    fn cart_40xx_read(&mut self, addr: u16, maria: bool) -> u8 {
        self.cart_op(0xFF, |k, b| k.read_40xx(addr, maria, b))
    }

    fn overlay_read(&mut self, addr: u16, window: u8, maria: bool) -> u8 {
        match window {
            0x04 => self.cart_op(0xFF, |k, b| k.read_04xx(addr, b)),
            0x08 => self.cart_op(0xFF, |k, b| k.read_08xx(addr, b)),
            0x10 => self.cart_op(0xFF, |k, b| k.read_10xx(addr, b)),
            0x30 => self.cart_op(0xFF, |k, b| k.read_30xx(addr, maria, b)),
            _ => self.cart_40xx_read(addr, maria),
        }
    }

    fn overlay_write(&mut self, addr: u16, window: u8, data: u8) {
        match window {
            0x04 => self.cart_op((), |k, b| k.write_04xx(addr, data, b)),
            0x08 => self.cart_op((), |k, b| k.write_08xx(addr, data, b)),
            0x10 => self.cart_op((), |k, b| k.write_10xx(addr, data, b)),
            0x30 => self.cart_op((), |k, b| k.write_30xx(addr, data, b)),
            _ => self.cart_op((), |k, b| k.write_40xx(addr, data, b)),
        }
    }

    /// Drive RIOT inputs from the ports + console byte, then read.
    /// Port input levels come from `pin_levels` (plain devices derive
    /// them from `joy_r`; AtariVox/SaveKey drive READY/SDA there), so
    /// the RIOT DDR mask picks latch-vs-device per bit like hardware.
    fn riot_read(&mut self, addr: u16) -> u8 {
        let now = self.cpu_cycles();
        let pa = console::riot_joystick_byte(
            self.port1.pin_levels(now),
            self.port2.pin_levels(now),
        );
        self.riot.set_pa_in(pa);
        self.riot.set_pb_in(self.console.byte());
        self.riot.read(addr)
    }

    /// Drive RIOT inputs, write, then run the port callbacks
    /// (`riot_joystick_w` row select, `riot_button_pullup_w` with its
    /// `$283` DDR gate). SWCHA data writes and PA DDR writes also push
    /// the driven pin state into the ports (`pin_write`) so bit-banged
    /// peripherals (AtariVox/SaveKey) see every edge, including
    /// DDR-direction flips (I2C SDA release = STOP).
    fn riot_write(&mut self, addr: u16, data: u8) {
        let now = self.cpu_cycles();
        let pa = console::riot_joystick_byte(
            self.port1.pin_levels(now),
            self.port2.pin_levels(now),
        );
        self.riot.set_pa_in(pa);
        self.riot.set_pb_in(self.console.byte());
        self.riot.write(addr, data);
        let off = (addr & 0x1F) as u8;
        if off & 0x17 == 0x00 {
            let (d1, d2) = console::riot_joystick_split(data);
            self.port1.joy_w(d1);
            self.port2.joy_w(d2);
        } else if off & 0x17 == 0x02 {
            let ddr_283 = self.riot.reg_read(0x03);
            console::pullup_write(
                ddr_283,
                data,
                &mut self.tia.p1_one_button,
                &mut self.tia.p2_one_button,
            );
        }
        // SWCHA data regs ($280/$288 mirrors): `off & 0x17 == 0x00`.
        // PA DDR regs ($281/$289/$291/$299): `off & 0x07 == 0x01`, which
        // is exactly the `reg_write` PA-DDR branch (bit 0 set, bit 1
        // clear, never a timer/edge offset since those need bit 2).
        if off & 0x17 == 0x00 || off & 0x07 == 0x01 {
            let ddr = self.riot.pa_ddr();
            let driven = self.riot.pa_out & ddr;
            // PA high nibble = port 1, low nibble = port 2.
            self.port1.pin_write(driven >> 4, (ddr >> 4) & 0x0F, now);
            self.port2.pin_write(driven & 0x0F, ddr & 0x0F, now);
        }
    }

    /// SALLY-view read (full map: TIA/MARIA mirrors, RIOT, SRAM,
    /// overlays, cart, BIOS-or-cart).
    fn read_sally(&mut self, addr: u16) -> u8 {
        let c = Self::canonical(addr);
        // Overlays first: a POK450 cart's `0x0400–0x04FF` install shadows
        // RIOT RAM, exactly like the MAME dynamic handler install.
        if let Some(o) = self.overlay_at(c) {
            self.accesses += 1;
            self.halves += 2;
            return self.overlay_read(c, o.window, false);
        }
        match bus::decode(c) {
            Region::Tia => {
                self.accesses += 1;
                self.halves += 3;
                let cyc = self.cpu_cycles();
                self.beam.hpos = (cyc * 4) as u16;
                let (v, stall) =
                    self.tia.read(c, cyc, self.beam, &*self.port1, &*self.port2);
                self.halves += stall * 2;
                v
            }
            Region::Maria => {
                self.accesses += 1;
                self.halves += 2;
                self.maria.as_deref().unwrap().read((c - 0x20) as u8)
            }
            Region::ZpMirror | Region::SpMirror | Region::Sram1 | Region::Sram2 => {
                self.accesses += 1;
                self.halves += 2;
                self.mem.read_raw(c)
            }
            Region::RiotRam => {
                self.accesses += 1;
                self.halves += 3;
                self.mem.read_raw(c)
            }
            Region::Riot => {
                self.accesses += 1;
                self.halves += 2;
                self.riot_read(c)
            }
            Region::Open => {
                self.accesses += 1;
                self.halves += 2;
                0xFF
            }
            Region::Cart => {
                self.accesses += 1;
                self.halves += 2;
                if c >= 0xC000 {
                    let cart_byte = self.cart_40xx_read(c, false);
                    self.bios.read(c, self.tia.ctrl_reg, cart_byte)
                } else {
                    self.cart_40xx_read(c, false)
                }
            }
        }
    }

    /// SALLY-view write.
    fn write_sally(&mut self, addr: u16, val: u8) {
        let c = Self::canonical(addr);
        if let Some(o) = self.overlay_at(c) {
            self.accesses += 1;
            self.halves += 2;
            if o.writable {
                self.overlay_write(c, o.window, val);
            }
            return;
        }
        match bus::decode(c) {
            Region::Tia => {
                self.accesses += 1;
                self.halves += 3;
                let cyc = self.cpu_cycles();
                let p1_gun = self.port1.is_lightgun();
                let p2_gun = self.port2.is_lightgun();
                let stall =
                    self.tia.write(c, val, cyc, p1_gun, p2_gun, &mut *self.audio);
                self.halves += stall * 2;
            }
            Region::Maria => {
                self.accesses += 1;
                self.halves += 2;
                self.maria.as_deref_mut().unwrap().write((c - 0x20) as u8, val);
            }
            Region::ZpMirror | Region::SpMirror | Region::Sram1 | Region::Sram2 => {
                self.accesses += 1;
                self.halves += 2;
                self.mem.write_raw(c, val);
            }
            Region::RiotRam => {
                self.accesses += 1;
                self.halves += 3;
                self.mem.write_raw(c, val);
            }
            Region::Riot => {
                self.accesses += 1;
                self.halves += 2;
                self.riot_write(c, val);
            }
            Region::Open => {
                self.accesses += 1;
                self.halves += 2;
            }
            Region::Cart => {
                self.accesses += 1;
                self.halves += 2;
                // `a7800_mem`: writes hit the cart across the whole
                // `0x4000–0xFFFF` window, even BIOS-mapped `C000+`.
                self.cart_op((), |k, b| k.write_40xx(c, val, b));
            }
        }
    }

    /// MARIA-DMA view: same map, no clock accounting (DMA time is owned
    /// by `draw_scanline`), cart windows in MARIA mode.
    fn read_maria(&mut self, addr: u16) -> u8 {
        let c = Self::canonical(addr);
        if let Some(o) = self.overlay_at(c) {
            return self.overlay_read(c, o.window, true);
        }
        match bus::decode(c) {
            // Unreachable via `Maria::read_mem` (< 0x20 reads 0) except
            // through the TIA mirror: no side effects, like open bus.
            Region::Tia => 0xFF,
            // The taken-out MARIA answers via snapshot (== `Maria::read`).
            Region::Maria => {
                if (c - 0x20) as u8 == 0x08 {
                    self.mstat
                } else {
                    0x00
                }
            }
            Region::ZpMirror
            | Region::SpMirror
            | Region::Sram1
            | Region::Sram2
            | Region::RiotRam => self.mem.read_raw(c),
            // MAME-faithful: DMA reads travel the same map, side effects
            // included (a DL over RIOT space observes the timer).
            Region::Riot => self.riot_read(c),
            Region::Open => 0xFF,
            Region::Cart => {
                if c >= 0xC000 {
                    let cart_byte = self.cart_40xx_read(c, true);
                    self.bios.read(c, self.tia.ctrl_reg, cart_byte)
                } else {
                    self.cart_40xx_read(c, true)
                }
            }
        }
    }
}

impl CpuBus for MachineBus<'_> {
    fn read(&mut self, addr: u16) -> u8 {
        self.read_sally(addr)
    }

    fn write(&mut self, addr: u16, val: u8) {
        self.write_sally(addr, val);
    }
}

impl MariaBus for MachineBus<'_> {
    fn read(&mut self, addr: u16) -> u8 {
        self.read_maria(addr)
    }
}

/// Whole-machine save-state (Phase 8, serde via [`Snapshot`](crate::state::Snapshot)).
/// ROM bytes are immutable and travel with the loaded image: loading
/// requires the same slot key + PCB id (checked, never assumed).
impl Machine {
    /// Last completed frame's per-line DMA cycles (feeds `screen.frame`).
    pub fn dma_per_line(&self) -> &[i32] {
        &self.dma_frame
    }

    /// Bank/control registers of the live cart (socket `state.cart`).
    pub fn cart_regs(&self) -> Vec<(&'static str, i64)> {
        self.cart.as_ref().map(|c| c.debug_regs()).unwrap_or_default()
    }

    /// POKEY bases the live cart decodes (socket `state.cart`).
    pub fn cart_pokey_bases(&self) -> Vec<u16> {
        self.cart.as_ref().map(|c| c.pokey_bases()).unwrap_or_default()
    }

    /// Whether the live cart decodes the XM YM2151 window.
    pub fn cart_ym_present(&self) -> bool {
        self.cart.as_ref().map(|c| c.ym_present()).unwrap_or(false)
    }

    /// Audio samples rendered but not yet drained (`perf.sample`).
    pub fn pending_samples(&self) -> usize {
        self.samples.len()
    }

    /// Live battery contents for NVRAM persistence; `None` = no battery.
    pub fn nvram_image(&self) -> Option<Vec<u8>> {
        self.cart.as_ref().and_then(|c| c.nvram_image())
    }

    /// Restore battery contents; returns false when the cart has no battery.
    pub fn load_nvram_image(&mut self, image: &[u8]) -> bool {
        match self.cart.as_mut() {
            Some(c) if c.nvram_image().is_some() => {
                c.load_nvram(image);
                true
            }
            _ => false,
        }
    }

    /// Debugger/harness poke: one SALLY-view bus write with true cycle
    /// accounting (drives RIOT pin notification, TIA stalls, etc.).
    pub fn poke(&mut self, addr: u16, val: u8) {
        let h = {
            let mut bus = self.harness_bus();
            bus.write_sally(addr, val);
            bus.halves
        };
        self.clock_halves += h;
        self.sally.cycles += h / 2;
    }

    /// Debugger/harness peek: one SALLY-view bus read.
    pub fn peek(&mut self, addr: u16) -> u8 {
        let (v, h) = {
            let mut bus = self.harness_bus();
            let v = bus.read_sally(addr);
            (v, bus.halves)
        };
        self.clock_halves += h;
        self.sally.cycles += h / 2;
        v
    }

    /// One-shot bus for harness access (beam parked at line start).
    fn harness_bus(&mut self) -> MachineBus<'_> {
        MachineBus {
            maria: Some(&mut self.maria),
            mstat: 0,
            mem: &mut self.mem,
            tia: &mut self.tia_io,
            audio: &mut self.audio,
            riot: &mut self.riot,
            cart: self.cart.as_mut().map(|c| c as &mut Box<dyn Cart>),
            overlays: &self.overlays,
            bios: &self.bios,
            port1: &mut self.port1,
            port2: &mut self.port2,
            console: &self.console,
            beam: BeamState { hpos: 0, vpos: 0, lines: self.lines, is_pal: self.is_pal },
            base_halves: self.clock_halves,
            halves: 0,
            accesses: 0,
        }
    }

    /// Live AtariVox/SaveKey EEPROM of a control port (1 or 2);
    /// `None` = no such device plugged.
    pub fn port_nvram_image(&self, port: u8) -> Option<Vec<u8>> {
        use crate::ctrl::PortDevice;
        let p = if port == 1 { &self.port1 } else { &self.port2 };
        match p.device() {
            Some(PortDevice::AtariVox(d)) | Some(PortDevice::SaveKey(d)) => {
                Some(d.nvram_image())
            }
            _ => None,
        }
    }

    /// Restore an AtariVox/SaveKey EEPROM; false when not plugged.
    pub fn load_port_nvram(&mut self, port: u8, image: &[u8]) -> bool {
        use crate::ctrl::PortDevice;
        let p = if port == 1 { &mut self.port1 } else { &mut self.port2 };
        match p.device_mut() {
            Some(PortDevice::AtariVox(d)) | Some(PortDevice::SaveKey(d)) => {
                d.load_nvram(image);
                true
            }
            _ => false,
        }
    }

    /// Drain a port's received SpeakJet phrase log; `None` = no speech
    /// hardware on that port (empty, SaveKey, or plain controller).
    pub fn take_port_speech(&mut self, port: u8) -> Option<Vec<u8>> {
        use crate::ctrl::PortDevice;
        let p = if port == 1 { &mut self.port1 } else { &mut self.port2 };
        match p.device_mut() {
            Some(PortDevice::AtariVox(d)) if d.speech_fitted() => Some(d.take_speech()),
            _ => None,
        }
    }

    /// Capture a whole-machine snapshot (mid-frame exact).
    pub fn save_snapshot(&self) -> Snapshot {
        Snapshot {
            version: crate::state::SNAPSHOT_VERSION,
            machine: self.id.driver_name().to_string(),
            sally: self.sally.clone(),
            maria: self.maria.save(),
            audio: self.audio.save(),
            tia: self.tia_io.clone(),
            riot: self.riot.clone(),
            sram1: self.mem.sram1.to_vec(),
            sram2: self.mem.sram2.to_vec(),
            riot_ram: self.mem.riot_ram.to_vec(),
            bios_present: self.bios.present,
            bios: self.bios.data.to_vec(),
            port1: self.port1.clone(),
            port2: self.port2.clone(),
            console: self.console.clone(),
            cart: self.cart.as_ref().map(|c| SavedCart {
                slot_key: self.slot_key.unwrap_or("?").to_string(),
                pcb_id: self.pcb_id.unwrap_or(-99),
                state: c.save_state(),
            }),
            vpos: self.vpos,
            frame: self.frame.clone(),
            samples: self.samples.clone(),
            clock_halves: self.clock_halves,
            audio_frac: self.audio_frac,
            dma_frame: self.dma_frame.clone(),
            dma_accum: self.dma_accum.clone(),
        }
    }

    /// Restore a snapshot taken by [`save_snapshot`](Self::save_snapshot).
    pub fn load_snapshot(&mut self, s: &Snapshot) -> Result<(), SnapshotError> {
        if s.version != crate::state::SNAPSHOT_VERSION {
            return Err(SnapshotError::Version(s.version));
        }
        if s.machine != self.id.driver_name() {
            return Err(SnapshotError::Machine {
                expected: self.id.driver_name().to_string(),
                found: s.machine.clone(),
            });
        }
        let live_cart = self.slot_key.map(|k| (k.to_string(), self.pcb_id.unwrap_or(-99)));
        let file_cart = s.cart.as_ref().map(|c| (c.slot_key.clone(), c.pcb_id));
        if live_cart != file_cart {
            return Err(SnapshotError::Cart {
                expected: live_cart.map(|c| c.0),
                found: file_cart.map(|c| c.0),
            });
        }
        let shape = |field: &'static str, len: usize, want: usize| {
            (len == want).then_some(()).ok_or(SnapshotError::Shape { field, len })
        };
        shape("sram1", s.sram1.len(), 2048)?;
        shape("sram2", s.sram2.len(), 4096)?;
        shape("riot_ram", s.riot_ram.len(), 128)?;
        shape("bios", s.bios.len(), 0x4000)?;
        shape("frame", s.frame.len(), FRAME_W * self.lines as usize)?;
        self.sally = s.sally.clone();
        self.maria.load(&s.maria);
        self.audio.load(&s.audio);
        self.tia_io = s.tia.clone();
        self.riot = s.riot.clone();
        self.mem.sram1.copy_from_slice(&s.sram1);
        self.mem.sram2.copy_from_slice(&s.sram2);
        self.mem.riot_ram.copy_from_slice(&s.riot_ram);
        self.bios.present = s.bios_present;
        self.bios.data.copy_from_slice(&s.bios);
        self.port1 = s.port1.clone();
        self.port2 = s.port2.clone();
        self.console = s.console.clone();
        if let (Some(cart), Some(saved)) = (self.cart.as_mut(), s.cart.as_ref()) {
            cart.load_state(&saved.state);
        }
        self.vpos = s.vpos;
        self.frame.copy_from_slice(&s.frame);
        self.samples = s.samples.clone();
        self.clock_halves = s.clock_halves;
        self.audio_frac = s.audio_frac;
        self.dma_frame = s.dma_frame.clone();
        self.dma_accum = s.dma_accum.clone();
        self.sally.set_irq(self.riot.irq_asserted);
        Ok(())
    }
}

/// POKEY/YM2151 are Phase-0 stubs: cart sound reads return `0xFF`,
/// writes are ignored. Slot-window routing (which cart decodes POKEY
/// where) is real and covered by the corpus + machine tests.
impl CartBus for MachineBus<'_> {
    fn pokey_read(&mut self, _base: u16, _reg: u8) -> u8 {
        0xFF
    }

    fn pokey_write(&mut self, _base: u16, _reg: u8, _data: u8) {}

    fn ym_read(&mut self, _reg: u8) -> u8 {
        0xFF
    }

    fn ym_write(&mut self, _reg: u8, _data: u8) {}
}

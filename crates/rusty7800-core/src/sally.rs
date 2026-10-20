//! Rusty7800 `sally` — NMOS 6502 CPU (Atari SALLY).
//!
//! Phase 1 hand-port of `a7800-5.2/src/devices/cpu/m6502/` (M6502,
//! BSD-3-Clause, Olivier Galibert). ALU semantics (`do_adc_d`, `do_sbc_d`,
//! `do_arr_*`, `do_cmp`, `do_bit`, shifts) are transcribed from
//! `m6502.cpp`; cycle behavior (page-cross penalties, branch costs,
//! RMW dummy accesses, interrupt sequences, JMP-indirect page-wrap bug)
//! matches the MAME executor. Unofficial opcodes follow the header's
//! documented policy: reliable ones fully, unreliable ones per visual6502
//! stable formulas (see `MAGIC` notes). SALLY has no functional difference
//! from NMOS 6502 at this level (halt-on-DMA is modeled machine-side).

/// Bus interface: every `read`/`write` costs one CPU cycle.
pub trait CpuBus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, val: u8);
}

pub const F_N: u8 = 0x80;
pub const F_V: u8 = 0x40;
pub const F_B: u8 = 0x10;
pub const F_D: u8 = 0x08;
pub const F_I: u8 = 0x04;
pub const F_Z: u8 = 0x02;
pub const F_C: u8 = 0x01;

/// Stable high-bits constant assumed for LXA/ANE (visual6502 NMOS value).
const MAGIC: u8 = 0xEE;

const VEC_NMI: u16 = 0xFFFA;
const VEC_RESET: u16 = 0xFFFC;
const VEC_IRQ: u16 = 0xFFFE;

/// NMOS 6502 CPU state.
#[derive(Clone, Debug)]
pub struct Sally {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    /// Stack pointer low byte (stack page is always 0x01xx).
    pub sp: u8,
    pub pc: u16,
    pub p: u8,
    /// Total cycles executed.
    pub cycles: u64,
    /// Level-sensitive IRQ line.
    irq: bool,
    /// Edge-latched NMI.
    nmi: bool,
    /// KIL ($x2) jam state; cleared by reset.
    pub jammed: bool,
    /// MAME `inhibit_interrupts`: IRQ/NMI not serviced on the next step
    /// (set by CLI/SEI/BRK/RTI semantics).
    inhibit: bool,
}

impl Default for Sally {
    fn default() -> Self {
        Self::new()
    }
}

impl Sally {
    pub fn new() -> Self {
        Sally {
            // Power-on values per m6502.cpp init().
            a: 0x00,
            x: 0x80,
            y: 0x00,
            sp: 0xBD,
            pc: 0,
            p: 0x36,
            cycles: 0,
            irq: false,
            nmi: false,
            jammed: false,
            inhibit: false,
        }
    }

    /// Reset sequence: 6 cycles + vector fetch (total 7 like MAME STATE_RESET).
    pub fn reset<B: CpuBus>(&mut self, bus: &mut B) {
        self.jammed = false;
        self.inhibit = true;
        self.irq = false;
        self.nmi = false;
        self.sp = self.sp.wrapping_sub(3);
        let lo = bus.read(VEC_RESET) as u16;
        let hi = bus.read(VEC_RESET + 1) as u16;
        self.pc = lo | (hi << 8);
        self.p |= F_I;
        self.cycles += 7;
    }

    pub fn set_irq(&mut self, asserted: bool) {
        self.irq = asserted;
    }

    /// Latch an NMI rising edge.
    pub fn set_nmi(&mut self) {
        self.nmi = true;
    }

    fn tick(&mut self, n: u64) {
        self.cycles += n;
    }

    fn read<B: CpuBus>(&mut self, bus: &mut B, addr: u16) -> u8 {
        self.tick(1);
        bus.read(addr)
    }

    fn write<B: CpuBus>(&mut self, bus: &mut B, addr: u16, val: u8) {
        self.tick(1);
        bus.write(addr, val);
    }

    fn fetch<B: CpuBus>(&mut self, bus: &mut B) -> u8 {
        let v = self.read(bus, self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    fn push<B: CpuBus>(&mut self, bus: &mut B, v: u8) {
        self.write(bus, 0x100 | self.sp as u16, v);
        self.sp = self.sp.wrapping_sub(1);
    }

    fn pull<B: CpuBus>(&mut self, bus: &mut B) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.read(bus, 0x100 | self.sp as u16)
    }

    fn set_nz(&mut self, v: u8) {
        self.p &= !(F_N | F_Z);
        if v == 0 {
            self.p |= F_Z;
        } else if v & 0x80 != 0 {
            self.p |= F_N;
        }
    }

    // -- ALU (transcribed from m6502.cpp) ---------------------------------

    fn adc(&mut self, v: u8) {
        if self.p & F_D != 0 {
            self.adc_d(v);
        } else {
            self.adc_nd(v);
        }
    }

    fn adc_d(&mut self, val: u8) {
        let c = u8::from(self.p & F_C != 0);
        self.p &= !(F_N | F_V | F_Z | F_C);
        let al = (self.a & 15) + (val & 15) + c;
        let al = if al > 9 { al + 6 } else { al };
        let ah = (self.a >> 4) + (val >> 4) + u8::from(al > 15);
        if self.a.wrapping_add(val).wrapping_add(c) == 0 {
            self.p |= F_Z;
        } else if ah & 8 != 0 {
            self.p |= F_N;
        }
        if (!(self.a ^ val) & (self.a ^ (ah << 4)) & 0x80) != 0 {
            self.p |= F_V;
        }
        let ah = if ah > 9 { ah + 6 } else { ah };
        if ah > 15 {
            self.p |= F_C;
        }
        self.a = (ah << 4) | (al & 15);
    }

    fn adc_nd(&mut self, val: u8) {
        let sum = self.a as u16 + val as u16 + u16::from(self.p & F_C != 0);
        self.p &= !(F_N | F_V | F_Z | F_C);
        let r = sum as u8;
        if r == 0 {
            self.p |= F_Z;
        } else if r & 0x80 != 0 {
            self.p |= F_N;
        }
        if (!(self.a ^ val) & (self.a ^ r) & 0x80) != 0 {
            self.p |= F_V;
        }
        if sum & 0xff00 != 0 {
            self.p |= F_C;
        }
        self.a = r;
    }

    fn sbc(&mut self, v: u8) {
        if self.p & F_D != 0 {
            self.sbc_d(v);
        } else {
            self.sbc_nd(v);
        }
    }

    fn sbc_d(&mut self, val: u8) {
        let c = u8::from(self.p & F_C == 0);
        self.p &= !(F_N | F_V | F_Z | F_C);
        let diff = self.a as u16 as i32 - val as i32 - c as i32;
        let al = (self.a & 15) as i8 as i32 - (val & 15) as i32 - c as i32;
        let al = if al < 0 { al - 6 } else { al };
        let ah = (self.a >> 4) as i32 - (val >> 4) as i32 - i32::from(al < 0);
        let du = diff as u16;
        if du as u8 == 0 {
            self.p |= F_Z;
        } else if du & 0x80 != 0 {
            self.p |= F_N;
        }
        if ((self.a ^ val) & (self.a ^ du as u8) & 0x80) != 0 {
            self.p |= F_V;
        }
        if du & 0xff00 == 0 {
            self.p |= F_C;
        }
        let ah = if ah < 0 { ah - 6 } else { ah };
        self.a = ((ah << 4) | (al & 15)) as u8;
    }

    fn sbc_nd(&mut self, val: u8) {
        let diff = self.a as u16 as i32 - val as i32 - i32::from(self.p & F_C == 0);
        let du = diff as u16;
        self.p &= !(F_N | F_V | F_Z | F_C);
        if du as u8 == 0 {
            self.p |= F_Z;
        } else if du & 0x80 != 0 {
            self.p |= F_N;
        }
        if ((self.a ^ val) & (self.a ^ du as u8) & 0x80) != 0 {
            self.p |= F_V;
        }
        if du & 0xff00 == 0 {
            self.p |= F_C;
        }
        self.a = du as u8;
    }

    fn arr(&mut self) {
        if self.p & F_D != 0 {
            self.arr_d();
        } else {
            self.arr_nd();
        }
    }

    fn arr_nd(&mut self) {
        let c = self.p & F_C != 0;
        self.p &= !(F_N | F_Z | F_C | F_V);
        self.a >>= 1;
        if c {
            self.a |= 0x80;
        }
        if self.a == 0 {
            self.p |= F_Z;
        } else if self.a & 0x80 != 0 {
            self.p |= F_N;
        }
        if self.a & 0x40 != 0 {
            self.p |= F_V | F_C;
        }
        if self.a & 0x20 != 0 {
            self.p ^= F_V;
        }
    }

    fn arr_d(&mut self) {
        let c = self.p & F_C != 0;
        self.p &= !(F_N | F_Z | F_C | F_V);
        let old = self.a;
        let mut a = self.a >> 1;
        if c {
            a |= 0x80;
        }
        if a == 0 {
            self.p |= F_Z;
        } else if a & 0x80 != 0 {
            self.p |= F_N;
        }
        if (a ^ old) & 0x40 != 0 {
            self.p |= F_V;
        }
        if (old & 0x0f) >= 0x05 {
            a = (a.wrapping_add(6) & 0x0f) | (a & 0xf0);
        }
        if (old & 0xf0) >= 0x50 {
            a = a.wrapping_add(0x60);
            self.p |= F_C;
        }
        self.a = a;
    }

    fn cmp(&mut self, r1: u8, r2: u8) {
        self.p &= !(F_N | F_Z | F_C);
        let r = r1 as u16 as i32 - r2 as i32;
        if r == 0 {
            self.p |= F_Z;
        } else if r & 0x80 != 0 {
            self.p |= F_N;
        }
        if r >= 0 {
            self.p |= F_C;
        }
    }

    fn bit(&mut self, v: u8) {
        self.p &= !(F_N | F_Z | F_V);
        if self.a & v == 0 {
            self.p |= F_Z;
        }
        if v & 0x80 != 0 {
            self.p |= F_N;
        }
        if v & 0x40 != 0 {
            self.p |= F_V;
        }
    }

    fn asl(&mut self, v: u8) -> u8 {
        self.p &= !(F_N | F_Z | F_C);
        let r = v << 1;
        if r == 0 {
            self.p |= F_Z;
        } else if r & 0x80 != 0 {
            self.p |= F_N;
        }
        if v & 0x80 != 0 {
            self.p |= F_C;
        }
        r
    }

    fn lsr(&mut self, v: u8) -> u8 {
        self.p &= !(F_N | F_Z | F_C);
        if v & 1 != 0 {
            self.p |= F_C;
        }
        let r = v >> 1;
        if r == 0 {
            self.p |= F_Z;
        }
        r
    }

    fn ror(&mut self, v: u8) -> u8 {
        let c = self.p & F_C != 0;
        self.p &= !(F_N | F_Z | F_C);
        if v & 1 != 0 {
            self.p |= F_C;
        }
        let mut r = v >> 1;
        if c {
            r |= 0x80;
        }
        if r == 0 {
            self.p |= F_Z;
        } else if r & 0x80 != 0 {
            self.p |= F_N;
        }
        r
    }

    fn rol(&mut self, v: u8) -> u8 {
        let c = self.p & F_C != 0;
        self.p &= !(F_N | F_Z | F_C);
        if v & 0x80 != 0 {
            self.p |= F_C;
        }
        let mut r = v << 1;
        if c {
            r |= 0x01;
        }
        if r == 0 {
            self.p |= F_Z;
        } else if r & 0x80 != 0 {
            self.p |= F_N;
        }
        r
    }

    // -- addressing helpers (each bus access ticked; return addr) ---------

    fn imm<B: CpuBus>(&mut self, bus: &mut B) -> u8 {
        self.fetch(bus)
    }

    fn zp<B: CpuBus>(&mut self, bus: &mut B) -> u16 {
        self.fetch(bus) as u16
    }

    fn zpx<B: CpuBus>(&mut self, bus: &mut B) -> u16 {
        let b = self.fetch(bus);
        self.tick(1); // dummy read cycle
        b.wrapping_add(self.x) as u16
    }

    fn zpy<B: CpuBus>(&mut self, bus: &mut B) -> u16 {
        let b = self.fetch(bus);
        self.tick(1);
        b.wrapping_add(self.y) as u16
    }

    fn abs<B: CpuBus>(&mut self, bus: &mut B) -> u16 {
        let lo = self.fetch(bus) as u16;
        let hi = self.fetch(bus) as u16;
        lo | (hi << 8)
    }

    /// Absolute+X: `write` callers ignore the cross flag (RMW/stores always
    /// pay the fixup cycle); readers add it.
    fn abx<B: CpuBus>(&mut self, bus: &mut B, write: bool) -> (u16, bool) {
        let base = self.abs(bus);
        let addr = base.wrapping_add(self.x as u16);
        let cross = (base & 0xff00) != (addr & 0xff00);
        if cross || write {
            self.tick(1); // fixup/dummy cycle
        }
        (addr, cross)
    }

    fn aby<B: CpuBus>(&mut self, bus: &mut B) -> (u16, bool) {
        let base = self.abs(bus);
        let addr = base.wrapping_add(self.y as u16);
        let cross = (base & 0xff00) != (addr & 0xff00);
        if cross {
            self.tick(1);
        }
        (addr, cross)
    }

    fn idx<B: CpuBus>(&mut self, bus: &mut B) -> u16 {
        let b = self.fetch(bus);
        self.tick(1);
        let ptr = b.wrapping_add(self.x);
        let lo = self.read(bus, ptr as u16) as u16;
        let hi = self.read(bus, ptr.wrapping_add(1) as u16) as u16;
        lo | (hi << 8)
    }

    fn idy<B: CpuBus>(&mut self, bus: &mut B, write: bool) -> (u16, bool) {
        let b = self.fetch(bus);
        let lo = self.read(bus, b as u16) as u16;
        let hi = self.read(bus, b.wrapping_add(1) as u16) as u16;
        let base = lo | (hi << 8);
        let addr = base.wrapping_add(self.y as u16);
        let cross = (base & 0xff00) != (addr & 0xff00);
        if cross || write {
            self.tick(1);
        }
        (addr, cross)
    }

    fn branch<B: CpuBus>(&mut self, bus: &mut B, cond: bool) {
        let off = self.fetch(bus) as i8;
        if !cond {
            return;
        }
        self.tick(1);
        let target = self.pc.wrapping_add(off as i16 as u16);
        if (self.pc & 0xff00) != (target & 0xff00) {
            self.tick(1);
        }
        self.pc = target;
    }

    /// RMW helper: read, dummy write-back, apply, write result.
    fn rmw<B: CpuBus, F>(&mut self, bus: &mut B, addr: u16, f: F)
    where
        F: FnOnce(&mut Self, u8) -> u8,
    {
        let v = self.read(bus, addr);
        self.write(bus, addr, v); // dummy write-back
        let r = f(self, v);
        self.write(bus, addr, r);
    }

    fn do_interrupt<B: CpuBus>(&mut self, bus: &mut B, vector: u16, brk: bool) {
        let pc = self.pc;
        let mut p = self.p | 0x20;
        if brk {
            p |= F_B;
        } else {
            // Hardware IRQ/NMI: 2 internal cycles (BRK already paid 2 fetches).
            self.tick(2);
        }
        self.push(bus, (pc >> 8) as u8);
        self.push(bus, pc as u8);
        self.push(bus, p);
        self.p |= F_I;
        let lo = self.read(bus, vector) as u16;
        let hi = self.read(bus, vector + 1) as u16;
        self.pc = lo | (hi << 8);
    }

    /// Execute one instruction; returns cycles consumed.
    pub fn step<B: CpuBus>(&mut self, bus: &mut B) -> u64 {
        if self.jammed {
            self.tick(1);
            return 1;
        }
        let start = self.cycles;
        // Interrupt dispatch (MAME inhibit = 1-instruction delay after CLI/SEI).
        if !self.inhibit {
            if self.nmi {
                self.nmi = false;
                self.do_interrupt(bus, VEC_NMI, false);
                return self.cycles - start;
            }
            if self.irq && self.p & F_I == 0 {
                self.do_interrupt(bus, VEC_IRQ, false);
                return self.cycles - start;
            }
        }
        self.inhibit = false;
        let op = self.fetch(bus);
        self.exec::<B>(bus, op);
        self.cycles - start
    }

    #[allow(clippy::too_many_lines)]
    fn exec<B: CpuBus>(&mut self, bus: &mut B, op: u8) {
        match op {
            // LDA
            0xA9 => {
                let v = self.imm(bus);
                self.a = v;
                self.set_nz(v);
            }
            0xA5 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                self.a = v;
                self.set_nz(v);
            }
            0xB5 => {
                let a = self.zpx(bus);
                let v = self.read(bus, a);
                self.a = v;
                self.set_nz(v);
            }
            0xAD => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                self.a = v;
                self.set_nz(v);
            }
            0xBD => {
                let (a, c) = self.abx(bus, false);
                if c {
                    // extra cycle already ticked in abx
                }
                let v = self.read(bus, a);
                self.a = v;
                self.set_nz(v);
            }
            0xB9 => {
                let (a, _) = self.aby(bus);
                let v = self.read(bus, a);
                self.a = v;
                self.set_nz(v);
            }
            0xA1 => {
                let a = self.idx(bus);
                let v = self.read(bus, a);
                self.a = v;
                self.set_nz(v);
            }
            0xB1 => {
                let (a, _) = self.idy(bus, false);
                let v = self.read(bus, a);
                self.a = v;
                self.set_nz(v);
            }
            // LDX
            0xA2 => {
                let v = self.imm(bus);
                self.x = v;
                self.set_nz(v);
            }
            0xA6 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                self.x = v;
                self.set_nz(v);
            }
            0xB6 => {
                let a = self.zpy(bus);
                let v = self.read(bus, a);
                self.x = v;
                self.set_nz(v);
            }
            0xAE => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                self.x = v;
                self.set_nz(v);
            }
            0xBE => {
                let (a, _) = self.aby(bus);
                let v = self.read(bus, a);
                self.x = v;
                self.set_nz(v);
            }
            // LDY
            0xA0 => {
                let v = self.imm(bus);
                self.y = v;
                self.set_nz(v);
            }
            0xA4 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                self.y = v;
                self.set_nz(v);
            }
            0xB4 => {
                let a = self.zpx(bus);
                let v = self.read(bus, a);
                self.y = v;
                self.set_nz(v);
            }
            0xAC => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                self.y = v;
                self.set_nz(v);
            }
            0xBC => {
                let (a, _) = self.abx(bus, false);
                let v = self.read(bus, a);
                self.y = v;
                self.set_nz(v);
            }
            // STA
            0x85 => {
                let a = self.zp(bus);
                let v = self.a;
                self.write(bus, a, v);
            }
            0x95 => {
                let a = self.zpx(bus);
                let v = self.a;
                self.write(bus, a, v);
            }
            0x8D => {
                let a = self.abs(bus);
                let v = self.a;
                self.write(bus, a, v);
            }
            0x9D => {
                let (a, _) = self.abx(bus, true);
                let v = self.a;
                self.write(bus, a, v);
            }
            0x99 => {
                let base = self.abs(bus);
                let addr = base.wrapping_add(self.y as u16);
                self.tick(1);
                let v = self.a;
                self.write(bus, addr, v);
            }
            0x81 => {
                let a = self.idx(bus);
                let v = self.a;
                self.write(bus, a, v);
            }
            0x91 => {
                let (a, _) = self.idy(bus, true);
                let v = self.a;
                self.write(bus, a, v);
            }
            // STX
            0x86 => {
                let a = self.zp(bus);
                let v = self.x;
                self.write(bus, a, v);
            }
            0x96 => {
                let a = self.zpy(bus);
                let v = self.x;
                self.write(bus, a, v);
            }
            0x8E => {
                let a = self.abs(bus);
                let v = self.x;
                self.write(bus, a, v);
            }
            // STY
            0x84 => {
                let a = self.zp(bus);
                let v = self.y;
                self.write(bus, a, v);
            }
            0x94 => {
                let a = self.zpx(bus);
                let v = self.y;
                self.write(bus, a, v);
            }
            0x8C => {
                let a = self.abs(bus);
                let v = self.y;
                self.write(bus, a, v);
            }
            // Transfers
            0xAA => {
                self.tick(1);
                self.x = self.a;
                self.set_nz(self.x);
            }
            0xA8 => {
                self.tick(1);
                self.y = self.a;
                self.set_nz(self.y);
            }
            0xBA => {
                self.tick(1);
                self.x = self.sp;
                self.set_nz(self.x);
            }
            0x8A => {
                self.tick(1);
                self.a = self.x;
                self.set_nz(self.a);
            }
            0x9A => {
                self.tick(1);
                self.sp = self.x;
            }
            0x98 => {
                self.tick(1);
                self.a = self.y;
                self.set_nz(self.a);
            }
            // Stack
            0x48 => {
                self.tick(1);
                let v = self.a;
                self.push(bus, v);
            }
            0x08 => {
                self.tick(1);
                let v = self.p | 0x30;
                self.push(bus, v);
            }
            0x68 => {
                self.tick(2);
                let v = self.pull(bus);
                self.a = v;
                self.set_nz(v);
            }
            0x28 => {
                self.tick(2);
                let v = self.pull(bus);
                self.p = (v & !F_B) | 0x20;
            }
            // ADC
            0x69 => {
                let v = self.imm(bus);
                self.adc(v);
            }
            0x65 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                self.adc(v);
            }
            0x75 => {
                let a = self.zpx(bus);
                let v = self.read(bus, a);
                self.adc(v);
            }
            0x6D => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                self.adc(v);
            }
            0x7D => {
                let (a, _) = self.abx(bus, false);
                let v = self.read(bus, a);
                self.adc(v);
            }
            0x79 => {
                let (a, _) = self.aby(bus);
                let v = self.read(bus, a);
                self.adc(v);
            }
            0x61 => {
                let a = self.idx(bus);
                let v = self.read(bus, a);
                self.adc(v);
            }
            0x71 => {
                let (a, _) = self.idy(bus, false);
                let v = self.read(bus, a);
                self.adc(v);
            }
            // SBC (0xEB is the unofficial imm duplicate)
            0xE9 | 0xEB => {
                let v = self.imm(bus);
                self.sbc(v);
            }
            0xE5 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                self.sbc(v);
            }
            0xF5 => {
                let a = self.zpx(bus);
                let v = self.read(bus, a);
                self.sbc(v);
            }
            0xED => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                self.sbc(v);
            }
            0xFD => {
                let (a, _) = self.abx(bus, false);
                let v = self.read(bus, a);
                self.sbc(v);
            }
            0xF9 => {
                let (a, _) = self.aby(bus);
                let v = self.read(bus, a);
                self.sbc(v);
            }
            0xE1 => {
                let a = self.idx(bus);
                let v = self.read(bus, a);
                self.sbc(v);
            }
            0xF1 => {
                let (a, _) = self.idy(bus, false);
                let v = self.read(bus, a);
                self.sbc(v);
            }
            // AND
            0x29 => {
                let v = self.imm(bus);
                self.a &= v;
                self.set_nz(self.a);
            }
            0x25 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                self.a &= v;
                self.set_nz(self.a);
            }
            0x35 => {
                let a = self.zpx(bus);
                let v = self.read(bus, a);
                self.a &= v;
                self.set_nz(self.a);
            }
            0x2D => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                self.a &= v;
                self.set_nz(self.a);
            }
            0x3D => {
                let (a, _) = self.abx(bus, false);
                let v = self.read(bus, a);
                self.a &= v;
                self.set_nz(self.a);
            }
            0x39 => {
                let (a, _) = self.aby(bus);
                let v = self.read(bus, a);
                self.a &= v;
                self.set_nz(self.a);
            }
            0x21 => {
                let a = self.idx(bus);
                let v = self.read(bus, a);
                self.a &= v;
                self.set_nz(self.a);
            }
            0x31 => {
                let (a, _) = self.idy(bus, false);
                let v = self.read(bus, a);
                self.a &= v;
                self.set_nz(self.a);
            }
            // EOR
            0x49 => {
                let v = self.imm(bus);
                self.a ^= v;
                self.set_nz(self.a);
            }
            0x45 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                self.a ^= v;
                self.set_nz(self.a);
            }
            0x55 => {
                let a = self.zpx(bus);
                let v = self.read(bus, a);
                self.a ^= v;
                self.set_nz(self.a);
            }
            0x4D => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                self.a ^= v;
                self.set_nz(self.a);
            }
            0x5D => {
                let (a, _) = self.abx(bus, false);
                let v = self.read(bus, a);
                self.a ^= v;
                self.set_nz(self.a);
            }
            0x59 => {
                let (a, _) = self.aby(bus);
                let v = self.read(bus, a);
                self.a ^= v;
                self.set_nz(self.a);
            }
            0x41 => {
                let a = self.idx(bus);
                let v = self.read(bus, a);
                self.a ^= v;
                self.set_nz(self.a);
            }
            0x51 => {
                let (a, _) = self.idy(bus, false);
                let v = self.read(bus, a);
                self.a ^= v;
                self.set_nz(self.a);
            }
            // ORA
            0x09 => {
                let v = self.imm(bus);
                self.a |= v;
                self.set_nz(self.a);
            }
            0x05 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                self.a |= v;
                self.set_nz(self.a);
            }
            0x15 => {
                let a = self.zpx(bus);
                let v = self.read(bus, a);
                self.a |= v;
                self.set_nz(self.a);
            }
            0x0D => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                self.a |= v;
                self.set_nz(self.a);
            }
            0x1D => {
                let (a, _) = self.abx(bus, false);
                let v = self.read(bus, a);
                self.a |= v;
                self.set_nz(self.a);
            }
            0x19 => {
                let (a, _) = self.aby(bus);
                let v = self.read(bus, a);
                self.a |= v;
                self.set_nz(self.a);
            }
            0x01 => {
                let a = self.idx(bus);
                let v = self.read(bus, a);
                self.a |= v;
                self.set_nz(self.a);
            }
            0x11 => {
                let (a, _) = self.idy(bus, false);
                let v = self.read(bus, a);
                self.a |= v;
                self.set_nz(self.a);
            }
            // CMP/CPX/CPY
            0xC9 => {
                let v = self.imm(bus);
                let a = self.a;
                self.cmp(a, v);
            }
            0xC5 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                let r = self.a;
                self.cmp(r, v);
            }
            0xD5 => {
                let a = self.zpx(bus);
                let v = self.read(bus, a);
                let r = self.a;
                self.cmp(r, v);
            }
            0xCD => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                let r = self.a;
                self.cmp(r, v);
            }
            0xDD => {
                let (a, _) = self.abx(bus, false);
                let v = self.read(bus, a);
                let r = self.a;
                self.cmp(r, v);
            }
            0xD9 => {
                let (a, _) = self.aby(bus);
                let v = self.read(bus, a);
                let r = self.a;
                self.cmp(r, v);
            }
            0xC1 => {
                let a = self.idx(bus);
                let v = self.read(bus, a);
                let r = self.a;
                self.cmp(r, v);
            }
            0xD1 => {
                let (a, _) = self.idy(bus, false);
                let v = self.read(bus, a);
                let r = self.a;
                self.cmp(r, v);
            }
            0xE0 => {
                let v = self.imm(bus);
                let r = self.x;
                self.cmp(r, v);
            }
            0xE4 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                let r = self.x;
                self.cmp(r, v);
            }
            0xEC => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                let r = self.x;
                self.cmp(r, v);
            }
            0xC0 => {
                let v = self.imm(bus);
                let r = self.y;
                self.cmp(r, v);
            }
            0xC4 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                let r = self.y;
                self.cmp(r, v);
            }
            0xCC => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                let r = self.y;
                self.cmp(r, v);
            }
            // BIT
            0x24 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                self.bit(v);
            }
            0x2C => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                self.bit(v);
            }
            // ASL
            0x0A => {
                self.tick(1);
                let v = self.a;
                self.a = self.asl(v);
            }
            0x06 => {
                let a = self.zp(bus);
                self.rmw(bus, a, Self::asl);
            }
            0x16 => {
                let a = self.zpx(bus);
                self.rmw(bus, a, Self::asl);
            }
            0x0E => {
                let a = self.abs(bus);
                self.rmw(bus, a, Self::asl);
            }
            0x1E => {
                let (a, _) = self.abx(bus, true);
                self.rmw(bus, a, Self::asl);
            }
            // LSR
            0x4A => {
                self.tick(1);
                let v = self.a;
                self.a = self.lsr(v);
            }
            0x46 => {
                let a = self.zp(bus);
                self.rmw(bus, a, Self::lsr);
            }
            0x56 => {
                let a = self.zpx(bus);
                self.rmw(bus, a, Self::lsr);
            }
            0x4E => {
                let a = self.abs(bus);
                self.rmw(bus, a, Self::lsr);
            }
            0x5E => {
                let (a, _) = self.abx(bus, true);
                self.rmw(bus, a, Self::lsr);
            }
            // ROL
            0x2A => {
                self.tick(1);
                let v = self.a;
                self.a = self.rol(v);
            }
            0x26 => {
                let a = self.zp(bus);
                self.rmw(bus, a, Self::rol);
            }
            0x36 => {
                let a = self.zpx(bus);
                self.rmw(bus, a, Self::rol);
            }
            0x2E => {
                let a = self.abs(bus);
                self.rmw(bus, a, Self::rol);
            }
            0x3E => {
                let (a, _) = self.abx(bus, true);
                self.rmw(bus, a, Self::rol);
            }
            // ROR
            0x6A => {
                self.tick(1);
                let v = self.a;
                self.a = self.ror(v);
            }
            0x66 => {
                let a = self.zp(bus);
                self.rmw(bus, a, Self::ror);
            }
            0x76 => {
                let a = self.zpx(bus);
                self.rmw(bus, a, Self::ror);
            }
            0x6E => {
                let a = self.abs(bus);
                self.rmw(bus, a, Self::ror);
            }
            0x7E => {
                let (a, _) = self.abx(bus, true);
                self.rmw(bus, a, Self::ror);
            }
            // INC/DEC
            0xE6 => {
                let a = self.zp(bus);
                self.rmw(bus, a, |s, v| {
                    let r = v.wrapping_add(1);
                    s.set_nz(r);
                    r
                });
            }
            0xF6 => {
                let a = self.zpx(bus);
                self.rmw(bus, a, |s, v| {
                    let r = v.wrapping_add(1);
                    s.set_nz(r);
                    r
                });
            }
            0xEE => {
                let a = self.abs(bus);
                self.rmw(bus, a, |s, v| {
                    let r = v.wrapping_add(1);
                    s.set_nz(r);
                    r
                });
            }
            0xFE => {
                let (a, _) = self.abx(bus, true);
                self.rmw(bus, a, |s, v| {
                    let r = v.wrapping_add(1);
                    s.set_nz(r);
                    r
                });
            }
            0xC6 => {
                let a = self.zp(bus);
                self.rmw(bus, a, |s, v| {
                    let r = v.wrapping_sub(1);
                    s.set_nz(r);
                    r
                });
            }
            0xD6 => {
                let a = self.zpx(bus);
                self.rmw(bus, a, |s, v| {
                    let r = v.wrapping_sub(1);
                    s.set_nz(r);
                    r
                });
            }
            0xCE => {
                let a = self.abs(bus);
                self.rmw(bus, a, |s, v| {
                    let r = v.wrapping_sub(1);
                    s.set_nz(r);
                    r
                });
            }
            0xDE => {
                let (a, _) = self.abx(bus, true);
                self.rmw(bus, a, |s, v| {
                    let r = v.wrapping_sub(1);
                    s.set_nz(r);
                    r
                });
            }
            0xE8 => {
                self.tick(1);
                self.x = self.x.wrapping_add(1);
                self.set_nz(self.x);
            }
            0xC8 => {
                self.tick(1);
                self.y = self.y.wrapping_add(1);
                self.set_nz(self.y);
            }
            0xCA => {
                self.tick(1);
                self.x = self.x.wrapping_sub(1);
                self.set_nz(self.x);
            }
            0x88 => {
                self.tick(1);
                self.y = self.y.wrapping_sub(1);
                self.set_nz(self.y);
            }
            // Branches
            0x90 => {
                let c = self.p & F_C == 0;
                self.branch(bus, c);
            }
            0xB0 => {
                let c = self.p & F_C != 0;
                self.branch(bus, c);
            }
            0xF0 => {
                let c = self.p & F_Z != 0;
                self.branch(bus, c);
            }
            0xD0 => {
                let c = self.p & F_Z == 0;
                self.branch(bus, c);
            }
            0x10 => {
                let c = self.p & F_N == 0;
                self.branch(bus, c);
            }
            0x30 => {
                let c = self.p & F_N != 0;
                self.branch(bus, c);
            }
            0x50 => {
                let c = self.p & F_V == 0;
                self.branch(bus, c);
            }
            0x70 => {
                let c = self.p & F_V != 0;
                self.branch(bus, c);
            }
            // Jumps
            0x4C => {
                let a = self.abs(bus);
                self.pc = a;
            }
            0x6C => {
                // NMOS page-wrap bug: vector high byte wraps within the page.
                let ptr = self.abs(bus);
                let lo = self.read(bus, ptr) as u16;
                let hi = self.read(bus, (ptr & 0xff00) | ((ptr + 1) & 0x00ff)) as u16;
                self.pc = lo | (hi << 8);
            }
            0x20 => {
                let lo = self.fetch(bus) as u16;
                self.tick(1); // internal
                let hi = self.fetch(bus) as u16;
                let ret = self.pc.wrapping_sub(1);
                self.push(bus, (ret >> 8) as u8);
                self.push(bus, ret as u8);
                self.pc = lo | (hi << 8);
            }
            0x60 => {
                self.tick(1); // internal
                let _ = self.read(bus, 0x100 | self.sp as u16); // dummy read, SP kept
                let lo = self.pull(bus) as u16;
                let hi = self.pull(bus) as u16;
                self.tick(1); // PC++ internal
                self.pc = (lo | (hi << 8)).wrapping_add(1);
            }
            0x40 => {
                self.tick(2);
                let p = self.pull(bus);
                let lo = self.pull(bus) as u16;
                let hi = self.pull(bus) as u16;
                self.p = (p & !F_B) | 0x20;
                self.pc = lo | (hi << 8);
            }
            0x00 => {
                self.fetch(bus); // padding byte
                self.inhibit = true;
                let pc = self.pc;
                self.do_interrupt(bus, VEC_IRQ, true);
                let _ = pc;
            }
            // Flags
            0x18 => {
                self.tick(1);
                self.p &= !F_C;
            }
            0x38 => {
                self.tick(1);
                self.p |= F_C;
            }
            0x58 => {
                self.tick(1);
                self.p &= !F_I;
                self.inhibit = true;
            }
            0x78 => {
                self.tick(1);
                self.p |= F_I;
                self.inhibit = true;
            }
            0xD8 => {
                self.tick(1);
                self.p &= !F_D;
            }
            0xF8 => {
                self.tick(1);
                self.p |= F_D;
            }
            0xB8 => {
                self.tick(1);
                self.p &= !F_V;
            }
            0xEA => {
                self.tick(1);
            }
            // ---- Undocumented: reliable ----
            // LAX (load A+X)
            0xA7 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                self.a = v;
                self.x = v;
                self.set_nz(v);
            }
            0xB7 => {
                let a = self.zpy(bus);
                let v = self.read(bus, a);
                self.a = v;
                self.x = v;
                self.set_nz(v);
            }
            0xAF => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                self.a = v;
                self.x = v;
                self.set_nz(v);
            }
            0xBF => {
                let (a, _) = self.aby(bus);
                let v = self.read(bus, a);
                self.a = v;
                self.x = v;
                self.set_nz(v);
            }
            0xA3 => {
                let a = self.idx(bus);
                let v = self.read(bus, a);
                self.a = v;
                self.x = v;
                self.set_nz(v);
            }
            0xB3 => {
                let (a, _) = self.idy(bus, false);
                let v = self.read(bus, a);
                self.a = v;
                self.x = v;
                self.set_nz(v);
            }
            // SAX (A&X -> mem)
            0x87 => {
                let a = self.zp(bus);
                let v = self.a & self.x;
                self.write(bus, a, v);
            }
            0x97 => {
                let a = self.zpy(bus);
                let v = self.a & self.x;
                self.write(bus, a, v);
            }
            0x8F => {
                let a = self.abs(bus);
                let v = self.a & self.x;
                self.write(bus, a, v);
            }
            0x83 => {
                let a = self.idx(bus);
                let v = self.a & self.x;
                self.write(bus, a, v);
            }
            // DCP (DEC + CMP)
            0xC7 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = v.wrapping_sub(1);
                self.write(bus, a, r);
                let x = self.a;
                self.cmp(x, r);
            }
            0xD7 => {
                let a = self.zpx(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = v.wrapping_sub(1);
                self.write(bus, a, r);
                let x = self.a;
                self.cmp(x, r);
            }
            0xCF => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = v.wrapping_sub(1);
                self.write(bus, a, r);
                let x = self.a;
                self.cmp(x, r);
            }
            0xDF => {
                let (a, _) = self.abx(bus, true);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = v.wrapping_sub(1);
                self.write(bus, a, r);
                let x = self.a;
                self.cmp(x, r);
            }
            0xDB => {
                let base = self.abs(bus);
                let addr = base.wrapping_add(self.y as u16);
                self.tick(1);
                let v = self.read(bus, addr);
                self.write(bus, addr, v);
                let r = v.wrapping_sub(1);
                self.write(bus, addr, r);
                let x = self.a;
                self.cmp(x, r);
            }
            0xC3 => {
                let a = self.idx(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = v.wrapping_sub(1);
                self.write(bus, a, r);
                let x = self.a;
                self.cmp(x, r);
            }
            0xD3 => {
                let (a, _) = self.idy(bus, true);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = v.wrapping_sub(1);
                self.write(bus, a, r);
                let x = self.a;
                self.cmp(x, r);
            }
            // ISB (INC + SBC)
            0xE7 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = v.wrapping_add(1);
                self.write(bus, a, r);
                self.sbc(r);
            }
            0xF7 => {
                let a = self.zpx(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = v.wrapping_add(1);
                self.write(bus, a, r);
                self.sbc(r);
            }
            0xEF => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = v.wrapping_add(1);
                self.write(bus, a, r);
                self.sbc(r);
            }
            0xFF => {
                let (a, _) = self.abx(bus, true);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = v.wrapping_add(1);
                self.write(bus, a, r);
                self.sbc(r);
            }
            0xFB => {
                let base = self.abs(bus);
                let addr = base.wrapping_add(self.y as u16);
                self.tick(1);
                let v = self.read(bus, addr);
                self.write(bus, addr, v);
                let r = v.wrapping_add(1);
                self.write(bus, addr, r);
                self.sbc(r);
            }
            0xE3 => {
                let a = self.idx(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = v.wrapping_add(1);
                self.write(bus, a, r);
                self.sbc(r);
            }
            0xF3 => {
                let (a, _) = self.idy(bus, true);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = v.wrapping_add(1);
                self.write(bus, a, r);
                self.sbc(r);
            }
            // SLO (ASL + ORA)
            0x07 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.asl(v);
                self.write(bus, a, r);
                self.a |= r;
                self.set_nz(self.a);
            }
            0x17 => {
                let a = self.zpx(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.asl(v);
                self.write(bus, a, r);
                self.a |= r;
                self.set_nz(self.a);
            }
            0x0F => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.asl(v);
                self.write(bus, a, r);
                self.a |= r;
                self.set_nz(self.a);
            }
            0x1F => {
                let (a, _) = self.abx(bus, true);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.asl(v);
                self.write(bus, a, r);
                self.a |= r;
                self.set_nz(self.a);
            }
            0x1B => {
                let base = self.abs(bus);
                let addr = base.wrapping_add(self.y as u16);
                self.tick(1);
                let v = self.read(bus, addr);
                self.write(bus, addr, v);
                let r = self.asl(v);
                self.write(bus, addr, r);
                self.a |= r;
                self.set_nz(self.a);
            }
            0x03 => {
                let a = self.idx(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.asl(v);
                self.write(bus, a, r);
                self.a |= r;
                self.set_nz(self.a);
            }
            0x13 => {
                let (a, _) = self.idy(bus, true);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.asl(v);
                self.write(bus, a, r);
                self.a |= r;
                self.set_nz(self.a);
            }
            // SRE (LSR + EOR)
            0x47 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.lsr(v);
                self.write(bus, a, r);
                self.a ^= r;
                self.set_nz(self.a);
            }
            0x57 => {
                let a = self.zpx(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.lsr(v);
                self.write(bus, a, r);
                self.a ^= r;
                self.set_nz(self.a);
            }
            0x4F => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.lsr(v);
                self.write(bus, a, r);
                self.a ^= r;
                self.set_nz(self.a);
            }
            0x5F => {
                let (a, _) = self.abx(bus, true);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.lsr(v);
                self.write(bus, a, r);
                self.a ^= r;
                self.set_nz(self.a);
            }
            0x5B => {
                let base = self.abs(bus);
                let addr = base.wrapping_add(self.y as u16);
                self.tick(1);
                let v = self.read(bus, addr);
                self.write(bus, addr, v);
                let r = self.lsr(v);
                self.write(bus, addr, r);
                self.a ^= r;
                self.set_nz(self.a);
            }
            0x43 => {
                let a = self.idx(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.lsr(v);
                self.write(bus, a, r);
                self.a ^= r;
                self.set_nz(self.a);
            }
            0x53 => {
                let (a, _) = self.idy(bus, true);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.lsr(v);
                self.write(bus, a, r);
                self.a ^= r;
                self.set_nz(self.a);
            }
            // RLA (ROL + AND)
            0x27 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.rol(v);
                self.write(bus, a, r);
                self.a &= r;
                self.set_nz(self.a);
            }
            0x37 => {
                let a = self.zpx(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.rol(v);
                self.write(bus, a, r);
                self.a &= r;
                self.set_nz(self.a);
            }
            0x2F => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.rol(v);
                self.write(bus, a, r);
                self.a &= r;
                self.set_nz(self.a);
            }
            0x3F => {
                let (a, _) = self.abx(bus, true);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.rol(v);
                self.write(bus, a, r);
                self.a &= r;
                self.set_nz(self.a);
            }
            0x3B => {
                let base = self.abs(bus);
                let addr = base.wrapping_add(self.y as u16);
                self.tick(1);
                let v = self.read(bus, addr);
                self.write(bus, addr, v);
                let r = self.rol(v);
                self.write(bus, addr, r);
                self.a &= r;
                self.set_nz(self.a);
            }
            0x23 => {
                let a = self.idx(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.rol(v);
                self.write(bus, a, r);
                self.a &= r;
                self.set_nz(self.a);
            }
            0x33 => {
                let (a, _) = self.idy(bus, true);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.rol(v);
                self.write(bus, a, r);
                self.a &= r;
                self.set_nz(self.a);
            }
            // RRA (ROR + ADC)
            0x67 => {
                let a = self.zp(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.ror(v);
                self.write(bus, a, r);
                self.adc(r);
            }
            0x77 => {
                let a = self.zpx(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.ror(v);
                self.write(bus, a, r);
                self.adc(r);
            }
            0x6F => {
                let a = self.abs(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.ror(v);
                self.write(bus, a, r);
                self.adc(r);
            }
            0x7F => {
                let (a, _) = self.abx(bus, true);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.ror(v);
                self.write(bus, a, r);
                self.adc(r);
            }
            0x7B => {
                let base = self.abs(bus);
                let addr = base.wrapping_add(self.y as u16);
                self.tick(1);
                let v = self.read(bus, addr);
                self.write(bus, addr, v);
                let r = self.ror(v);
                self.write(bus, addr, r);
                self.adc(r);
            }
            0x63 => {
                let a = self.idx(bus);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.ror(v);
                self.write(bus, a, r);
                self.adc(r);
            }
            0x73 => {
                let (a, _) = self.idy(bus, true);
                let v = self.read(bus, a);
                self.write(bus, a, v);
                let r = self.ror(v);
                self.write(bus, a, r);
                self.adc(r);
            }
            // SBX (AXS): X = (A&X) - imm
            0xCB => {
                let v = self.imm(bus);
                let t = (self.a & self.x) as u16 as i32 - v as i32;
                self.p &= !(F_N | F_Z | F_C);
                if t as u8 == 0 {
                    self.p |= F_Z;
                } else if t & 0x80 != 0 {
                    self.p |= F_N;
                }
                if t >= 0 {
                    self.p |= F_C;
                }
                self.x = t as u8;
            }
            // SHA/SHX/SHY/SHS (stable addressing; value may drop on real HW
            // with certain bus capacitance — games never rely on the drop).
            0x9F => {
                let base = self.abs(bus);
                let addr = base.wrapping_add(self.y as u16);
                self.tick(1);
                self.write(bus, addr, self.a & self.x & ((base >> 8) as u8).wrapping_add(1));
            }
            0x93 => {
                let (a, _) = self.idy(bus, true);
                let hb = (a >> 8) as u8;
                self.write(bus, a, self.a & self.x & hb.wrapping_add(1));
            }
            0x9E => {
                let base = self.abs(bus);
                let addr = base.wrapping_add(self.y as u16);
                self.tick(1);
                self.write(bus, addr, self.x & ((base >> 8) as u8).wrapping_add(1));
            }
            0x9C => {
                let base = self.abs(bus);
                let addr = base.wrapping_add(self.x as u16);
                self.tick(1);
                self.write(bus, addr, self.y & ((base >> 8) as u8).wrapping_add(1));
            }
            0x9B => {
                let base = self.abs(bus);
                let addr = base.wrapping_add(self.y as u16);
                self.tick(1);
                self.sp = self.a & self.x;
                self.write(bus, addr, self.sp & ((base >> 8) as u8).wrapping_add(1));
            }
            // ---- Undocumented: visual6502-stable ----
            0x0B | 0x2B => {
                let v = self.imm(bus);
                self.a &= v;
                self.p &= !(F_N | F_Z | F_C);
                if self.a == 0 {
                    self.p |= F_Z;
                } else if self.a & 0x80 != 0 {
                    self.p |= F_N;
                }
                if self.a & 0x80 != 0 {
                    self.p |= F_C;
                }
            }
            0x4B => {
                let v = self.imm(bus);
                self.a &= v;
                let c = self.a & 1 != 0;
                self.a >>= 1;
                self.p &= !(F_N | F_Z | F_C);
                if c {
                    self.p |= F_C;
                }
                if self.a == 0 {
                    self.p |= F_Z;
                }
            }
            0x6B => {
                let v = self.imm(bus);
                self.a &= v;
                self.arr();
            }
            0xAB => {
                let v = self.imm(bus);
                let r = (self.a | MAGIC) & v;
                self.a = r;
                self.x = r;
                self.set_nz(r);
            }
            0x8B => {
                let v = self.imm(bus);
                self.a = (self.a | MAGIC) & self.x & v;
                self.set_nz(self.a);
            }
            0xBB => {
                let (a, _) = self.aby(bus);
                let v = self.read(bus, a);
                let r = self.sp & v;
                self.a = r;
                self.x = r;
                self.sp = r;
                self.set_nz(r);
            }
            // NOP variants
            0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => {
                self.tick(1);
            }
            0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 => {
                self.fetch(bus);
            }
            0x04 | 0x44 | 0x64 => {
                self.fetch(bus);
                self.tick(1);
            }
            0x14 | 0x34 | 0x54 | 0x74 | 0xD4 | 0xF4 => {
                self.fetch(bus);
                self.tick(2);
            }
            0x0C => {
                self.fetch(bus);
                self.fetch(bus);
                self.tick(1);
            }
            0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => {
                let (a, c) = self.abx(bus, false);
                if c {
                    // fixup already ticked
                }
                let _ = self.read(bus, a); // dummy read keeps timing exact
            }
            // KIL: bus jam until reset.
            0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 | 0x62 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2 => {
                self.jammed = true;
            }
        }
    }
}

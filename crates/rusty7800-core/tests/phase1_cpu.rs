//! Phase 1 acceptance: SALLY (NMOS 6502) functional + cycle tests.
//!
//! Hand-assembled programs executed against a flat 64 KiB RAM bus.
//! Covers all addressing modes, BCD, branches/penalties, stack and
//! interrupts, RMW, unofficial ops, and the JMP-indirect page-wrap bug.

use rusty7800_core::sally::{CpuBus, Sally, F_C, F_D, F_I, F_N, F_V, F_Z};

struct Ram([u8; 65536]);

impl Ram {
    fn new() -> Self {
        Ram([0; 65536])
    }
}

impl CpuBus for Ram {
    fn read(&mut self, addr: u16) -> u8 {
        self.0[addr as usize]
    }

    fn write(&mut self, addr: u16, val: u8) {
        self.0[addr as usize] = val;
    }
}

struct Fixture {
    cpu: Sally,
    bus: Ram,
}

impl Fixture {
    fn new() -> Self {
        Fixture {
            cpu: Sally::new(),
            bus: Ram::new(),
        }
    }

    fn load(&mut self, addr: u16, bytes: &[u8]) {
        for (i, b) in bytes.iter().enumerate() {
            self.bus.0[addr as usize + i] = *b;
        }
    }

    /// Run `n` instructions from `pc`; returns total cycles.
    fn run(&mut self, pc: u16, n: usize) -> u64 {
        self.cpu.pc = pc;
        let start = self.cpu.cycles;
        for _ in 0..n {
            self.cpu.step(&mut self.bus);
        }
        self.cpu.cycles - start
    }
}

#[test]
fn lda_sta_all_modes_and_cycles() {
    let mut f = Fixture::new();
    // zp + data
    f.bus.0[0x0010] = 0x42;
    f.bus.0[0x0020] = 0x11; // for zpx
    f.bus.0[0x1234] = 0x77;
    f.bus.0[0x1244] = 0x88; // abx no-cross target (0x1234+0x10)
    f.bus.0[0x1310] = 0x99; // abx cross target (0x12FF+0x11)
    f.bus.0[0x0034] = 0x34;
    f.bus.0[0x0035] = 0x12; // idx ptr (X=4 wraps 0x30 -> 0x34)
    f.bus.0[0x1234] = 0x55; // idx target
    f.bus.0[0x0040] = 0x00;
    f.bus.0[0x0041] = 0x20; // idy base 0x2000
    f.bus.0[0x2005] = 0x66; // idy target (Y=5, no cross)
    f.bus.0[0x2102] = 0x77; // idy cross target (base 0x20FD + Y=5)
    f.bus.0[0x0042] = 0xFD;
    f.bus.0[0x0043] = 0x20;

    // LDA imm
    f.load(0x8000, &[0xA9, 0x44]);
    assert_eq!(f.run(0x8000, 1), 2);
    assert_eq!(f.cpu.a, 0x44);

    // LDA zp / zpx / abs
    f.cpu.x = 0x10;
    f.load(0x8000, &[0xA5, 0x10, 0xB5, 0x10, 0xAD, 0x34, 0x12]);
    assert_eq!(f.run(0x8000, 3), 3 + 4 + 4);
    assert_eq!(f.cpu.a, 0x55); // last: abs loaded 0x55 (overwritten below? no)

    // LDA abx no-cross (4) then cross (5)
    f.cpu.x = 0x10;
    f.load(0x8000, &[0xBD, 0x34, 0x12]);
    assert_eq!(f.run(0x8000, 1), 4);
    assert_eq!(f.cpu.a, 0x88);
    f.cpu.x = 0x11;
    f.load(0x8000, &[0xBD, 0xFF, 0x12]);
    assert_eq!(f.run(0x8000, 1), 5);
    assert_eq!(f.cpu.a, 0x99);

    // LDA (idx)
    f.cpu.x = 0x04;
    f.load(0x8000, &[0xA1, 0x30]);
    assert_eq!(f.run(0x8000, 1), 6);
    assert_eq!(f.cpu.a, 0x55);

    // LDA (idy) no-cross (5) then cross (6)
    f.cpu.y = 0x05;
    f.load(0x8000, &[0xB1, 0x40]);
    assert_eq!(f.run(0x8000, 1), 5);
    assert_eq!(f.cpu.a, 0x66);
    f.load(0x8000, &[0xB1, 0x42]);
    assert_eq!(f.run(0x8000, 1), 6);
    assert_eq!(f.cpu.a, 0x77);

    // STA zp/zpx/abs/abx/aby/idx/idy cycle spot checks
    f.cpu.a = 0x5A;
    f.cpu.x = 0x01;
    f.cpu.y = 0x02;
    f.load(
        0x8000,
        &[
            0x85, 0x50, // 3
            0x95, 0x50, // 4
            0x8D, 0x00, 0x30, // 4
            0x9D, 0x00, 0x30, // 5
            0x99, 0x00, 0x30, // 5
            0x81, 0x60, // 6
            0x91, 0x62, // 6
        ],
    );
    assert_eq!(f.run(0x8000, 7), 3 + 4 + 4 + 5 + 5 + 6 + 6);
    assert_eq!(f.bus.0[0x0050], 0x5A);
    assert_eq!(f.bus.0[0x3000], 0x5A);
}

#[test]
fn adc_sbc_binary_flags() {
    let mut f = Fixture::new();
    // ADC: 0x10+0x20
    f.load(0x8000, &[0xA9, 0x10, 0x69, 0x20]);
    f.run(0x8000, 2);
    assert_eq!(f.cpu.a, 0x30);
    assert_eq!(f.cpu.p & (F_C | F_Z | F_N | F_V), 0);

    // ADC overflow: 0x50+0x50 = 0xA0, N+V
    f.load(0x8000, &[0xA9, 0x50, 0x69, 0x50]);
    f.run(0x8000, 2);
    assert_eq!(f.cpu.a, 0xA0);
    assert!(f.cpu.p & F_N != 0 && f.cpu.p & F_V != 0);

    // ADC carry+zero: 0xFF+0x01
    f.load(0x8000, &[0xA9, 0xFF, 0x69, 0x01]);
    f.run(0x8000, 2);
    assert_eq!(f.cpu.a, 0x00);
    assert!(f.cpu.p & F_Z != 0 && f.cpu.p & F_C != 0);

    // SBC: 0x30-0x10 (C set) = 0x20, C stays
    f.load(0x8000, &[0x38, 0xA9, 0x30, 0xE9, 0x10]);
    f.run(0x8000, 3);
    assert_eq!(f.cpu.a, 0x20);
    assert!(f.cpu.p & F_C != 0);

    // SBC borrow: 0x10-0x20 = 0xF0, C clear, N set
    f.load(0x8000, &[0x38, 0xA9, 0x10, 0xE9, 0x20]);
    f.run(0x8000, 3);
    assert_eq!(f.cpu.a, 0xF0);
    assert!(f.cpu.p & F_C == 0 && f.cpu.p & F_N != 0);
}

#[test]
fn adc_sbc_decimal_mode() {
    let mut f = Fixture::new();
    // 15+27=42 BCD
    f.load(0x8000, &[0xF8, 0xA9, 0x15, 0x69, 0x27]);
    f.run(0x8000, 3);
    assert_eq!(f.cpu.a, 0x42);
    assert!(f.cpu.p & F_C == 0);

    // 99+01=00 BCD carry (hardware/MAME quirk: Z reflects the binary sum
    // 0x9A, so Z is clear and N set on the 0x00 result).
    f.load(0x8000, &[0xA9, 0x99, 0x69, 0x01]);
    f.run(0x8000, 2);
    assert_eq!(f.cpu.a, 0x00);
    assert!(f.cpu.p & F_C != 0 && f.cpu.p & F_Z == 0);

    // 42-27=15 BCD
    f.load(0x8000, &[0x38, 0xA9, 0x42, 0xE9, 0x27]);
    f.run(0x8000, 3);
    assert_eq!(f.cpu.a, 0x15);
    assert!(f.cpu.p & F_C != 0);

    // 00-01=99 BCD borrow
    f.load(0x8000, &[0x38, 0xA9, 0x00, 0xE9, 0x01]);
    f.run(0x8000, 3);
    assert_eq!(f.cpu.a, 0x99);
    assert!(f.cpu.p & F_C == 0);
    f.load(0x8000, &[0xD8]);
    f.run(0x8000, 1);
    assert!(f.cpu.p & F_D == 0);
}

#[test]
fn branches_costs() {
    let mut f = Fixture::new();
    // BNE not taken (Z set): 2 cycles
    f.load(0x8000, &[0xA9, 0x00, 0xD0, 0x05]);
    assert_eq!(f.run(0x8000, 2), 2 + 2);
    assert_eq!(f.cpu.pc, 0x8004);
    // BNE taken, no cross: 3
    f.load(0x8000, &[0xA9, 0x01, 0xD0, 0x02, 0xEA, 0xEA, 0xEA]);
    assert_eq!(f.run(0x8000, 2), 2 + 3);
    assert_eq!(f.cpu.pc, 0x8006);
    // BNE taken with page cross: 4
    f.load(0x80F8, &[0xA9, 0x01, 0xD0, 0x05]);
    assert_eq!(f.run(0x80F8, 2), 2 + 4);
    assert_eq!(f.cpu.pc, 0x8101);
}

#[test]
fn stack_jsr_rts_rti_brk() {
    let mut f = Fixture::new();
    // JSR 6, RTS 6 round trip; A preserved via PHA/PLA (3/4)
    f.load(0x8000, &[0xA9, 0x7E, 0x48, 0xA9, 0x00, 0x68]);
    assert_eq!(f.run(0x8000, 4), 2 + 3 + 2 + 4);
    assert_eq!(f.cpu.a, 0x7E);

    f.load(0x8000, &[0x20, 0x05, 0x80, 0xEA, 0xEA, 0xA9, 0x33, 0x60]);
    assert_eq!(f.run(0x8000, 2), 6 + 2);
    assert_eq!(f.cpu.a, 0x33);
    // RTS was the 2nd... actually run 2 instrs = JSR + LDA; step RTS:
    assert_eq!(f.run(f.cpu.pc, 1), 6);
    assert_eq!(f.cpu.pc, 0x8003);

    // BRK = 7 cycles, pushes PC+2 with B set, I set
    f.bus.0[0xFFFE] = 0x00;
    f.bus.0[0xFFFF] = 0x90;
    f.load(0x8000, &[0x00]);
    assert_eq!(f.run(0x8000, 1), 7);
    assert_eq!(f.cpu.pc, 0x9000);
    assert!(f.cpu.p & F_I != 0);
    // stacked P has B set (live P was 0x34 after LDA #$01)
    assert_eq!(f.bus.0[0x01BB], 0x34);
}

#[test]
fn irq_and_nmi_sequences() {
    let mut f = Fixture::new();
    f.bus.0[0xFFFE] = 0x00;
    f.bus.0[0xFFFF] = 0x40;
    f.bus.0[0xFFFA] = 0x00;
    f.bus.0[0xFFFB] = 0x50;
    // CLI then IRQ serviced (7 cycles), inhibit delay honored
    f.load(0x8000, &[0x58, 0xEA]);
    f.cpu.set_irq(true);
    assert_eq!(f.run(0x8000, 1), 2); // CLI only; IRQ held one instr
    assert_eq!(f.run(f.cpu.pc, 1), 2); // NOP
    assert_eq!(f.run(f.cpu.pc, 1), 7); // IRQ sequence
    assert_eq!(f.cpu.pc, 0x4000);

    // NMI latches and wins immediately
    f.cpu.set_irq(false);
    f.load(0x8000, &[0xEA]);
    f.cpu.set_nmi();
    assert_eq!(f.run(0x8000, 1), 7);
    assert_eq!(f.cpu.pc, 0x5000);
}

#[test]
fn rmw_shifts_and_inc_dec() {
    let mut f = Fixture::new();
    f.bus.0[0x0040] = 0x81;
    f.load(0x8000, &[0x06, 0x40]); // ASL zp: 0x81->0x02, C set. 5 cycles
    assert_eq!(f.run(0x8000, 1), 5);
    assert_eq!(f.bus.0[0x0040], 0x02);
    assert!(f.cpu.p & F_C != 0);

    f.bus.0[0x2000] = 0x01;
    f.cpu.x = 0x05;
    f.load(0x8000, &[0x5E, 0xFB, 0x1F]); // LSR abx: 7 cycles
    assert_eq!(f.run(0x8000, 1), 7);
    assert_eq!(f.bus.0[0x2000], 0x00);
    assert!(f.cpu.p & (F_C | F_Z) == (F_C | F_Z));

    // ROL acc chains carry: A=0x80,C=1 -> 0x01, C=1
    f.load(0x8000, &[0xA9, 0x80, 0x38, 0x2A]);
    f.run(0x8000, 3);
    assert_eq!(f.cpu.a, 0x01);
    assert!(f.cpu.p & F_C != 0);

    // INC abs wrap to zero sets Z (6 cycles)
    f.bus.0[0x3000] = 0xFF;
    f.load(0x8000, &[0xEE, 0x00, 0x30]);
    assert_eq!(f.run(0x8000, 1), 6);
    assert_eq!(f.bus.0[0x3000], 0x00);
    assert!(f.cpu.p & F_Z != 0);

    // DEC zpx (6 cycles)
    f.bus.0[0x0042] = 0x01;
    f.cpu.x = 0x02;
    f.load(0x8000, &[0xD6, 0x40]);
    assert_eq!(f.run(0x8000, 1), 6);
    assert_eq!(f.bus.0[0x0042], 0x00);
}

#[test]
fn compares_and_bit() {
    let mut f = Fixture::new();
    // CMP equal -> Z+C
    f.load(0x8000, &[0xA9, 0x30, 0xC9, 0x30]);
    f.run(0x8000, 2);
    assert!(f.cpu.p & (F_Z | F_C) == (F_Z | F_C));
    // CPX less -> N set, C clear
    f.load(0x8000, &[0xA2, 0x10, 0xE0, 0x20]);
    f.run(0x8000, 2);
    assert!(f.cpu.p & F_C == 0 && f.cpu.p & F_N != 0);
    // CPY greater -> C set
    f.load(0x8000, &[0xA0, 0x40, 0xC0, 0x30]);
    f.run(0x8000, 2);
    assert!(f.cpu.p & F_C != 0);
    // BIT: A=0x0F & 0xF0 -> Z; N/V from operand
    f.bus.0[0x0050] = 0xF0;
    f.load(0x8000, &[0xA9, 0x0F, 0x24, 0x50]);
    f.run(0x8000, 2);
    assert!(f.cpu.p & (F_Z | F_N | F_V) == (F_Z | F_N | F_V));
}

#[test]
fn jmp_indirect_page_wrap_bug() {
    let mut f = Fixture::new();
    // Vector at 0x10FF: lo=0x34; hi must come from 0x1000 (wrap), not 0x1100.
    f.bus.0[0x10FF] = 0x34;
    f.bus.0[0x1000] = 0x12;
    f.bus.0[0x1100] = 0x99;
    f.load(0x8000, &[0x6C, 0xFF, 0x10]);
    assert_eq!(f.run(0x8000, 1), 5);
    assert_eq!(f.cpu.pc, 0x1234);
}

#[test]
fn unofficial_reliable_ops() {
    let mut f = Fixture::new();
    // LAX zp
    f.bus.0[0x0060] = 0x5A;
    f.load(0x8000, &[0xA7, 0x60]);
    f.run(0x8000, 1);
    assert_eq!((f.cpu.a, f.cpu.x), (0x5A, 0x5A));
    // SAX zp
    f.cpu.a = 0xF0;
    f.cpu.x = 0x0F;
    f.load(0x8000, &[0x87, 0x61]);
    f.run(0x8000, 1);
    assert_eq!(f.bus.0[0x0061], 0x00);
    // DCP zp: mem 5 -> 4, CMP A=4 -> Z
    f.bus.0[0x0062] = 0x05;
    f.load(0x8000, &[0xA9, 0x04, 0xC7, 0x62]);
    f.run(0x8000, 2);
    assert_eq!(f.bus.0[0x0062], 0x04);
    assert!(f.cpu.p & F_Z != 0);
    // RLA zp: ROL 0x41 -> 0x82 then AND A=0xFF -> 0x82 (CLC first: ROL uses C)
    f.bus.0[0x0063] = 0x41;
    f.load(0x8000, &[0x18, 0xA9, 0xFF, 0x27, 0x63]);
    f.run(0x8000, 3);
    assert_eq!(f.cpu.a, 0x82);
    // SLO zp: ASL 0x41 -> 0x82, ORA -> A
    f.bus.0[0x0064] = 0x41;
    f.load(0x8000, &[0xA9, 0x0F, 0x07, 0x64]);
    f.run(0x8000, 2);
    assert_eq!(f.cpu.a, 0x8F);
    // SRE zp: LSR 0x03 -> 0x01 C=1, EOR A=0xFF -> 0xFE
    f.bus.0[0x0065] = 0x03;
    f.load(0x8000, &[0xA9, 0xFF, 0x47, 0x65]);
    f.run(0x8000, 2);
    assert_eq!(f.cpu.a, 0xFE);
    assert!(f.cpu.p & F_C != 0);
    // RRA zp: ROR 0x02 -> 0x01, ADC A=0x01 -> 0x02 (CLC first: ROR uses C)
    f.bus.0[0x0066] = 0x02;
    f.load(0x8000, &[0x18, 0xA9, 0x01, 0x67, 0x66]);
    f.run(0x8000, 3);
    assert_eq!(f.cpu.a, 0x02);
    // ISB zp: INC 0x02 -> 0x03, SBC A=0x05 -> 0x02
    f.bus.0[0x0067] = 0x02;
    f.load(0x8000, &[0x38, 0xA9, 0x05, 0xE7, 0x67]);
    f.run(0x8000, 3);
    assert_eq!(f.cpu.a, 0x02);
    // SBX: X=(0xFF&0x0F)-0x05 = 0x0A
    f.cpu.a = 0xFF;
    f.cpu.x = 0x0F;
    f.load(0x8000, &[0xCB, 0x05]);
    f.run(0x8000, 1);
    assert_eq!(f.cpu.x, 0x0A);
    // NOP variants lengths
    f.load(0x8000, &[0x1A, 0x80, 0x00, 0x04, 0x10, 0x0C, 0x00, 0x20]);
    assert_eq!(f.run(0x8000, 4), 2 + 2 + 3 + 4);
    assert_eq!(f.cpu.pc, 0x8008);
    // KIL jams until reset (state frozen: the LDA never executes)
    f.cpu.a = 0x42;
    f.load(0x8000, &[0x02, 0xA9, 0xFF]);
    f.run(0x8000, 1);
    assert!(f.cpu.jammed);
    f.run(0x8000, 1);
    assert_eq!(f.cpu.a, 0x42);
}

#[test]
fn transfers_flags_and_misc_cycles() {
    let mut f = Fixture::new();
    // TAX/TAY/TSX/TXA/TXS/TYA = 2 cycles each (6 transfers)
    f.load(0x8000, &[0xA9, 0x80, 0xAA, 0xA8, 0xBA, 0x8A, 0x9A, 0x98]);
    assert_eq!(f.run(0x8000, 7), 2 + 2 * 6);
    // TSX overwrites X with SP (0xBD); TXA already ran, TYA restores A.
    assert_eq!((f.cpu.a, f.cpu.x, f.cpu.y), (0x80, 0xBD, 0x80));
    assert!(f.cpu.p & F_N != 0);
    // INX/DEX/INY/DEY wrap both ways
    f.load(0x8000, &[0xA2, 0xFF, 0xE8, 0xCA, 0xA0, 0xFF, 0xC8, 0x88]);
    f.run(0x8000, 8);
    assert_eq!((f.cpu.x, f.cpu.y), (0xFF, 0xFF));
    assert!(f.cpu.p & F_N != 0);
    // JMP abs 3, JSR 6 tested elsewhere; RTI 6
    f.bus.0[0xFFFE] = 0x34;
    f.bus.0[0xFFFF] = 0x12;
    f.load(0x8000, &[0xA9, 0x01, 0x00]);
    f.run(0x8000, 2); // LDA + BRK
    assert_eq!(f.cpu.pc, 0x1234);
    f.bus.0[0x1234] = 0x40; // RTI
    assert_eq!(f.run(0x1234, 1), 6);
}

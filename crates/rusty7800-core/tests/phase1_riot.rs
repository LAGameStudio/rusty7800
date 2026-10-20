//! Phase 1 acceptance: RIOT 6532 incl. the timer rules.
//!
//! Pins the documented behavior from `6532riot.cpp` (flag 0→1 keeps
//! counting at 1-cycle rate; timer read/write clears the flag; flag set +
//! contents 0 free-runs) — the rules embodying the Saarna timer fix.

use rusty7800_core::riot::{Riot, PA7_FLAG, TIMER_FLAG};

/// Timer-write offsets: 0x14 | divsel | (irq_en << 3).
const T_DIV1: u8 = 0x14;
const T_DIV8: u8 = 0x15;
const T_DIV64: u8 = 0x16;
const T_DIV1024: u8 = 0x17;
const T_DIV1_IRQ: u8 = 0x1C;

#[test]
fn reset_state_matches_mame() {
    let r = Riot::new();
    // shift 10, COUNTING, 256<<10 ticks -> value 0 (256>>... wraps to u8 0)
    assert_eq!(r.timer_value(), 0);
    assert!(!r.irq_asserted);
}

#[test]
fn timer_counts_down_and_times_out() {
    let mut r = Riot::new();
    r.reg_write(T_DIV1, 5); // remaining = 1 + 5
    assert_eq!(r.timer_value(), 6);
    r.tick(5);
    assert_eq!(r.timer_value(), 1);
    r.tick(1); // expiry -> FINISHING, flag set, reload 256
    assert_eq!(r.timer_value(), 0); // 256 as u8
    assert_eq!(r.reg_read(0x05) & TIMER_FLAG, TIMER_FLAG);
}

#[test]
fn timer_read_clears_flag_and_sets_irq_enable() {
    let mut r = Riot::new();
    r.reg_write(T_DIV1, 2);
    r.tick(3); // expire
    assert_eq!(r.reg_read(0x05) & TIMER_FLAG, TIMER_FLAG);
    // Timer read with A3=0 clears enable + flag...
    let _ = r.reg_read(0x04);
    assert_eq!(r.reg_read(0x05) & TIMER_FLAG, 0);
    // ...with A3=1 sets enable.
    r.reg_write(T_DIV1, 2);
    r.tick(3);
    let _ = r.reg_read(0x0C);
    assert_eq!(r.reg_read(0x05) & TIMER_FLAG, 0);
}

#[test]
fn divisors_scale_timeout() {
    for (off, shift) in [(T_DIV1, 0), (T_DIV8, 3), (T_DIV64, 6), (T_DIV1024, 10)] {
        let mut r = Riot::new();
        r.reg_write(off, 4);
        let expect = 1 + (4u32 << shift);
        assert_eq!(r.timer_value(), (expect >> shift) as u8, "offset {off:#04X}");
        r.tick((expect - 1) as u64);
        assert_eq!(r.reg_read(0x05) & TIMER_FLAG, 0);
        r.tick(1);
        assert_eq!(r.reg_read(0x05) & TIMER_FLAG, TIMER_FLAG, "offset {off:#04X}");
    }
}

#[test]
fn finishing_free_runs_and_keeps_counting() {
    // Flag 0->1 keeps counting at 1-cycle rate (header rule 1).
    let mut r = Riot::new();
    r.reg_write(T_DIV1, 1); // remaining = 2
    r.tick(2); // expire -> FINISHING @256
    assert_eq!(r.reg_read(0x05) & TIMER_FLAG, TIMER_FLAG);
    // Without any read (flag stays set), the counter keeps running...
    r.tick(6);
    assert_eq!(r.timer_value(), 250); // 256 - 6, unshifted in FINISHING
    // ...and wraps 0 -> 255 (contents-0 stop rule shows as wrap reload).
    r.tick(250);
    assert_eq!(r.timer_value(), 0);
    r.tick(1);
    assert_eq!(r.timer_value(), 255);
}

#[test]
fn timer_irq_asserts_when_enabled() {
    let mut r = Riot::new();
    assert!(!r.irq_asserted);
    r.reg_write(T_DIV1_IRQ, 2); // IRQ enable set
    r.tick(3);
    assert!(r.irq_asserted);
    // Timer read clears flag -> line drops.
    let _ = r.reg_read(0x04);
    assert!(!r.irq_asserted);
}

#[test]
fn ports_ddr_and_pa7_edge_detect() {
    let mut r = Riot::new();
    // Port A as output: reads return latch regardless of input.
    r.reg_write(0x01, 0xFF); // DDRA
    r.set_pa_in(0xAA);
    r.reg_write(0x00, 0x55); // OUTA
    assert_eq!(r.reg_read(0x00), 0x55);
    // Port A as input: reads return input lines.
    r.reg_write(0x01, 0x00);
    assert_eq!(r.reg_read(0x00), 0xAA);
    // Mixed DDR.
    r.reg_write(0x01, 0xF0);
    r.reg_write(0x00, 0xFF);
    r.set_pa_in(0x00);
    assert_eq!(r.reg_read(0x00), 0xF0);
    assert_eq!(r.reg_read(0x01), 0xF0); // DDR readable

    // PA7 negative-edge detect: enable (0x06), dir = high-to-low.
    r.reg_write(0x01, 0x00); // input
    r.reg_write(0x06, 0x00); // edge ctl: A7 IRQ enable, A0=0 dir
    r.set_pa_in(0x00); // settle prev=0
    let _ = r.reg_read(0x05); // clear stale PA7
    r.set_pa_in(0x80); // rising: no flag (wrong direction)
    assert_eq!(r.reg_read(0x05) & PA7_FLAG, 0);
    r.set_pa_in(0x00); // falling: flag
    assert_eq!(r.reg_read(0x05) & PA7_FLAG, PA7_FLAG);
    // Flags read clears PA7 implicitly.
    assert_eq!(r.reg_read(0x05) & PA7_FLAG, 0);

    // Port B basic.
    r.reg_write(0x03, 0x0F); // DDRB low nibble out
    r.reg_write(0x02, 0xFF); // OUTB
    r.set_pb_in(0xA0);
    assert_eq!(r.reg_read(0x02), 0xAF);
}

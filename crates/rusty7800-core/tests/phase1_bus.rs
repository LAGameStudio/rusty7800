//! Phase 1 acceptance: bus decode, mirrors, stall accounting.

use rusty7800_core::bus::{access_cost_half, decode, Clock, Memory, Region, COST_FAST_HALF, COST_SLOW_HALF};
use rusty7800_core::sally::CpuBus;

#[test]
fn decode_table_matches_address_map() {
    assert_eq!(decode(0x0000), Region::Tia);
    assert_eq!(decode(0x001F), Region::Tia);
    assert_eq!(decode(0x0020), Region::Maria);
    assert_eq!(decode(0x003F), Region::Maria);
    assert_eq!(decode(0x0040), Region::ZpMirror);
    assert_eq!(decode(0x00FF), Region::ZpMirror);
    assert_eq!(decode(0x0140), Region::SpMirror);
    assert_eq!(decode(0x01FF), Region::SpMirror);
    assert_eq!(decode(0x0280), Region::Riot);
    assert_eq!(decode(0x029F), Region::Riot);
    assert_eq!(decode(0x0480), Region::RiotRam);
    assert_eq!(decode(0x04FF), Region::RiotRam);
    assert_eq!(decode(0x1800), Region::Sram1);
    assert_eq!(decode(0x1FFF), Region::Sram1);
    assert_eq!(decode(0x2000), Region::Sram2);
    assert_eq!(decode(0x27FF), Region::Sram2);
    assert_eq!(decode(0x4000), Region::Cart);
    assert_eq!(decode(0xFFFF), Region::Cart);
    assert_eq!(decode(0x0100), Region::Open);
    assert_eq!(decode(0x0300), Region::Open);
}

#[test]
fn zp_and_sp_mirrors_alias_sram2() {
    let mut m = Memory::new();
    // zp mirror -> 0x2040..0x20FF
    m.write(0x2040, 0xAA);
    assert_eq!(m.read(0x0040), 0xAA);
    m.write(0x0077, 0xBB);
    assert_eq!(m.read(0x2077), 0xBB);
    m.write(0x00FF, 0xCC);
    assert_eq!(m.read(0x20FF), 0xCC);
    // sp mirror -> 0x2140..0x21FF
    m.write(0x2140, 0x11);
    assert_eq!(m.read(0x0140), 0x11);
    m.write(0x01FF, 0x22);
    assert_eq!(m.read(0x21FF), 0x22);
    // mirrors don't disturb neighbors
    m.write(0x203F, 0x77);
    m.write(0x2100, 0x88);
    assert_eq!(m.read(0x203F), 0x77);
    assert_eq!(m.read(0x2100), 0x88);
}

#[test]
fn sram_and_riot_ram_sizes() {
    let mut m = Memory::new();
    m.write(0x1800, 0x01);
    m.write(0x1FFF, 0x02);
    assert_eq!(m.read(0x1800), 0x01);
    assert_eq!(m.read(0x1FFF), 0x02);
    m.write(0x0480, 0x03);
    m.write(0x04FF, 0x04);
    assert_eq!(m.read(0x0480), 0x03);
    assert_eq!(m.read(0x04FF), 0x04);
    // open bus reads 0xFF, writes ignored
    assert_eq!(m.read(0x4000), 0xFF);
    m.write(0x4000, 0x12);
    assert_eq!(m.read(0x4000), 0xFF);
}

#[test]
fn slow_access_classification() {
    // TIA registers and RIOT RAM are slow (1.5x); everything else fast.
    assert_eq!(access_cost_half(0x0000), COST_SLOW_HALF);
    assert_eq!(access_cost_half(0x0010), COST_SLOW_HALF);
    assert_eq!(access_cost_half(0x0480), COST_SLOW_HALF);
    assert_eq!(access_cost_half(0x04FF), COST_SLOW_HALF);
    for addr in [0x0020, 0x0040, 0x0140, 0x0280, 0x1800, 0x2000, 0x4000, 0xFFFF] {
        assert_eq!(access_cost_half(addr), COST_FAST_HALF, "addr {addr:#06X}");
    }
    let mut c = Clock::new();
    c.access(0x2000); // fast: 2 halves
    c.access(0x0000); // slow: 3 halves
    assert_eq!(c.half_cycles(), 5);
    assert_eq!(c.cycles(), 2);
}

#[test]
fn cpu_runs_on_memory_bus() {
    // LDA #$42 / STA $2040 via zp mirror; verifies CpuBus integration.
    use rusty7800_core::sally::Sally;
    let mut m = Memory::new();
    // Program lives in SRAM (cart area is open bus until Phase 5).
    m.write_raw(0x2000, 0xA9);
    m.write_raw(0x2001, 0x42);
    m.write_raw(0x2002, 0x85);
    m.write_raw(0x2003, 0x40);
    let mut cpu = Sally::new();
    cpu.pc = 0x2000;
    assert_eq!(cpu.step(&mut m), 2);
    assert_eq!(cpu.step(&mut m), 3);
    assert_eq!(m.read_raw(0x2040), 0x42);
}

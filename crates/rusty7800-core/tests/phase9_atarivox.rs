//! Phase 9: AtariVox/SaveKey on the live bus.
//!
//! Drives the real path a game uses — SALLY-view `poke`/`peek` of RIOT
//! SWCHA (`$280`) data + DDR (`$281`) → machine pin notification → the
//! plugged port device — and asserts EEPROM contents, ACK/NACK reads
//! back through the DDR mux, slot registration, NVRAM helpers, speech
//! drain, and snapshot round-trip. UART timing (baud decode) is covered
//! at device level in `ctrl::atarivox` (a `poke` advances ~1 cycle, far
//! below a 19 200-baud cell, so the bus cannot synthesize speech here).

use rusty7800_core::ctrl::{PortDevice, SLOT_NAMES};
use rusty7800_core::machine::Machine;
use rusty7800_core::specs;
use rusty7800_core::timing::MachineId;

/// Port-2 (low nibble) I2C bit-banger over RIOT `$280`/`$281`.
struct Wire<'a> {
    m: &'a mut Machine,
    /// Current SWCHA low-nibble latch (bit0 DATA, bit2 SDA, bit3 SCL).
    lat: u8,
}

impl<'a> Wire<'a> {
    fn new(m: &'a mut Machine) -> Self {
        Wire { m, lat: 0x0D }
    }

    fn poke_swcha(&mut self, v: u8) {
        self.lat = v & 0x0F;
        self.m.poke(0x280, v);
    }

    fn poke_ddr(&mut self, v: u8) {
        self.m.poke(0x281, v);
    }

    /// Port-2 nibble read back through the RIOT DDR mux.
    fn nibble(&mut self) -> u8 {
        self.m.peek(0x280) & 0x0F
    }

    fn scl(&mut self, high: bool) {
        let v = if high {
            self.lat | 0x08
        } else {
            self.lat & !0x08
        };
        self.poke_swcha(v);
    }

    fn sda(&mut self, high: bool) {
        let v = if high {
            self.lat | 0x04
        } else {
            self.lat & !0x04
        };
        self.poke_swcha(v);
    }

    fn start(&mut self) {
        // Idle (SCL/SDA high, outputs) then SDA falls: START.
        self.poke_ddr(0x0D);
        self.poke_swcha(0x0D);
        self.sda(false);
        self.scl(false);
    }

    fn stop(&mut self) {
        self.sda(false);
        self.scl(true);
        self.sda(true); // STOP
        self.scl(false);
    }

    fn write_bit(&mut self, bit: bool) {
        self.sda(bit);
        self.scl(true);
        self.scl(false);
    }

    /// One byte + ACK clock (SDA released, device pulls low = ACK).
    fn write_byte(&mut self, b: u8) -> bool {
        for i in (0..8).rev() {
            self.write_bit((b >> i) & 1 != 0);
        }
        self.poke_ddr(0x09); // release SDA
        self.scl(true);
        let ack = self.nibble() & 0x04 == 0;
        self.scl(false);
        self.poke_ddr(0x0D);
        ack
    }

    fn read_byte(&mut self, ack: bool) -> u8 {
        self.poke_ddr(0x09); // release SDA for the device to drive
        let mut b = 0u8;
        for _ in 0..8 {
            self.scl(true);
            b = (b << 1) | ((self.nibble() >> 2) & 1);
            self.scl(false);
        }
        self.poke_ddr(0x0D);
        self.sda(!ack);
        self.scl(true);
        self.scl(false);
        b
    }
}

fn write_mem(m: &mut Machine, addr: u16, data: &[u8]) -> bool {
    let mut w = Wire::new(m);
    w.start();
    if !w.write_byte(0xA0) || !w.write_byte((addr >> 8) as u8) || !w.write_byte(addr as u8) {
        return false;
    }
    for &b in data {
        if !w.write_byte(b) {
            return false;
        }
    }
    w.stop();
    true
}

fn read_mem(m: &mut Machine, addr: u16, n: usize) -> Option<Vec<u8>> {
    let mut w = Wire::new(m);
    w.start();
    if !w.write_byte(0xA0) || !w.write_byte((addr >> 8) as u8) || !w.write_byte(addr as u8) {
        return None;
    }
    w.stop();
    w.start();
    if !w.write_byte(0xA1) {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(w.read_byte(i + 1 < n));
    }
    w.stop();
    Some(out)
}

fn eeprom_of(m: &Machine) -> Vec<u8> {
    match m.port2.device() {
        Some(PortDevice::AtariVox(d)) | Some(PortDevice::SaveKey(d)) => d.nvram_image(),
        _ => panic!("expected atarivox/savekey on port 2"),
    }
}

#[test]
fn slots_register_both_devices() {
    assert!(SLOT_NAMES.contains(&"atarivox"));
    assert!(SLOT_NAMES.contains(&"savekey"));
    assert!(specs::CONTROL_PORT_DEVICES
        .iter()
        .any(|(n, _)| *n == "atarivox"));
    let mut m = Machine::new(MachineId::Ntsc);
    m.port2.select_slot("atarivox");
    assert_eq!(m.port2.device().unwrap().slot_name(), "atarivox");
    m.port2.select_slot("savekey");
    assert_eq!(m.port2.device().unwrap().slot_name(), "savekey");
    assert!(m.port2.device().unwrap().slot_name() != "atarivox");
    // Unknown names still unplug.
    m.port2.select_slot("nope");
    assert!(!m.port2.exists());
}

#[test]
fn bus_bitbang_writes_and_reads_eeprom() {
    let mut m = Machine::new(MachineId::Ntsc);
    m.port2.select_slot("atarivox");
    assert!(write_mem(&mut m, 0x0140, b"HI-7800"));
    assert_eq!(&eeprom_of(&m)[0x140..0x147], b"HI-7800");
    assert_eq!(read_mem(&mut m, 0x0140, 7).unwrap(), b"HI-7800");
}

#[test]
fn bus_reports_nak_for_unknown_control() {
    let mut m = Machine::new(MachineId::Ntsc);
    m.port2.select_slot("atarivox");
    let mut w = Wire::new(&mut m);
    w.start();
    assert!(!w.write_byte(0xB0)); // neither $A0 nor $A1: NACK
    w.stop();
}

#[test]
fn bus_reads_ready_high_when_idle() {
    let mut m = Machine::new(MachineId::Ntsc);
    m.port2.select_slot("atarivox");
    assert!(write_mem(&mut m, 0x0200, &[0xAA]));
    // DDR bit 1 is an input; READY idles high (no speech in flight).
    let mut w = Wire::new(&mut m);
    w.poke_ddr(0x0D);
    assert!(w.nibble() & 0x02 != 0);
}

#[test]
fn savekey_keeps_eeprom_but_has_no_speech() {
    let mut m = Machine::new(MachineId::Ntsc);
    m.port2.select_slot("savekey");
    assert!(write_mem(&mut m, 0x0100, &[0x11, 0x22]));
    assert_eq!(read_mem(&mut m, 0x0100, 2).unwrap(), vec![0x11, 0x22]);
    // No speech hardware: drain is None, READY reads high.
    assert!(m.take_port_speech(2).is_none());
    let mut w = Wire::new(&mut m);
    w.poke_ddr(0x0D);
    assert!(w.nibble() & 0x02 != 0);
}

#[test]
fn machine_nvram_and_speech_helpers() {
    let mut m = Machine::new(MachineId::Ntsc);
    // Plain joystick port: no NVRAM, no speech.
    assert!(m.port_nvram_image(1).is_none());
    assert!(!m.load_port_nvram(1, &[0; 8]));
    assert!(m.take_port_speech(1).is_none());
    m.port2.select_slot("atarivox");
    assert!(write_mem(&mut m, 0x1000, &[1, 2, 3, 4]));
    let img = m.port_nvram_image(2).expect("atarivox nvram");
    assert_eq!(img.len(), 32 * 1024);
    assert_eq!(&img[0x1000..0x1004], &[1, 2, 3, 4]);
    // Idle device: empty (not missing) speech log drains.
    assert_eq!(m.take_port_speech(2), Some(vec![]));
    // Reload round-trips.
    let mut m2 = Machine::new(MachineId::Ntsc);
    m2.port2.select_slot("atarivox");
    assert!(m2.load_port_nvram(2, &img));
    assert_eq!(&eeprom_of(&m2)[0x1000..0x1004], &[1, 2, 3, 4]);
}

#[test]
fn snapshot_round_trips_eeprom() {
    let mut m = Machine::new(MachineId::Ntsc);
    m.port2.select_slot("atarivox");
    assert!(write_mem(&mut m, 0x0300, &[0xDE, 0xAD]));
    let snap = m.save_snapshot();
    let mut m2 = Machine::new(MachineId::Ntsc);
    m2.port2.select_slot("atarivox");
    m2.load_snapshot(&snap).expect("snapshot loads");
    assert_eq!(&eeprom_of(&m2)[0x300..0x302], &[0xDE, 0xAD]);
}

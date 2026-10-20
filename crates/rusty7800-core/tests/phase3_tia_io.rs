//! Phase 3 acceptance: TIA reads/writes (INPT mux, paddles, lightguns).

use rusty7800_core::tia::TiaAudio;
use rusty7800_core::tia_io::{BeamState, TiaIo, TiaPort};

#[derive(Clone, Default)]
struct FakePort {
    joy: u8,
    pot_x: u8,
    pot_y: u8,
    light_x: u8,
    light_y: u8,
    has_pot_x: bool,
    has_pot_y: bool,
    paddle: bool,
    gun: bool,
    pro: bool,
}

impl TiaPort for FakePort {
    fn joy_r(&self) -> u8 {
        self.joy
    }
    fn pot_x(&self) -> u8 {
        self.pot_x
    }
    fn pot_y(&self) -> u8 {
        self.pot_y
    }
    fn light_x(&self) -> u8 {
        self.light_x
    }
    fn light_y(&self) -> u8 {
        self.light_y
    }
    fn has_pot_x(&self) -> bool {
        self.has_pot_x
    }
    fn has_pot_y(&self) -> bool {
        self.has_pot_y
    }
    fn is_paddle(&self) -> bool {
        self.paddle
    }
    fn is_lightgun(&self) -> bool {
        self.gun
    }
    fn has_pro_buttons(&self) -> bool {
        self.pro
    }
}

fn beam(hpos: u16, vpos: u16) -> BeamState {
    BeamState {
        hpos,
        vpos,
        lines: 263,
        is_pal: false,
    }
}

fn read(io: &mut TiaIo, off: u16, cycles: u64, p1: &FakePort, p2: &FakePort) -> (u8, u64) {
    io.read(off, cycles, beam(0, 0), p1, p2)
}

#[test]
fn collision_and_undefined_reads() {
    let mut io = TiaIo::new();
    let (p1, p2) = (FakePort::default(), FakePort::default());
    for off in 0x00..=0x07 {
        assert_eq!(read(&mut io, off, 0, &p1, &p2).0, 0x00);
    }
    assert_eq!(read(&mut io, 0x0E, 0, &p1, &p2).0, 0xFF);
    assert_eq!(read(&mut io, 0x0F, 0, &p1, &p2).0, 0xFF);
    // Mirrored offsets mask to the same registers.
    assert_eq!(read(&mut io, 0x108, 0, &p1, &p2).0, 0x00);
}

#[test]
fn paddle_discharge_ramps() {
    let mut io = TiaIo::new();
    let p1 = FakePort {
        paddle: true,
        pot_x: 50,
        pot_y: 200,
        ..Default::default()
    };
    let p2 = FakePort::default();
    io.paddle_start = 1000;
    // INPT0: elapsed 0 -> 0x00; elapsed == level -> still 0x00 (needs >).
    assert_eq!(read(&mut io, 0x08, 1000, &p1, &p2).0, 0x00);
    assert_eq!(read(&mut io, 0x08, 1000 + 50 * 97, &p1, &p2).0, 0x00);
    assert_eq!(read(&mut io, 0x08, 1000 + 51 * 97, &p1, &p2).0, 0x80);
    // INPT1 uses pot_y.
    assert_eq!(read(&mut io, 0x09, 1000 + 200 * 97, &p1, &p2).0, 0x00);
    assert_eq!(read(&mut io, 0x09, 1000 + 201 * 97, &p1, &p2).0, 0x80);
    // INPT2 XOR quirk: level is pot_x ^ 0xFF (here 0 ^ 0xFF = 255).
    let p2p = FakePort {
        paddle: true,
        pot_x: 0,
        pot_y: 7,
        ..Default::default()
    };
    assert_eq!(read(&mut io, 0x0A, 1000 + 255 * 97, &p1, &p2p).0, 0x00);
    assert_eq!(read(&mut io, 0x0A, 1000 + 256 * 97, &p1, &p2p).0, 0x80);
    // INPT3 plain pot_y on port 2.
    assert_eq!(read(&mut io, 0x0B, 1000 + 7 * 97, &p1, &p2p).0, 0x00);
    assert_eq!(read(&mut io, 0x0B, 1000 + 8 * 97, &p1, &p2p).0, 0x80);
}

#[test]
fn pot_and_digital_fallbacks() {
    let mut io = TiaIo::new();
    // Analog pot device (not paddle): value returned directly.
    let p1 = FakePort {
        has_pot_x: true,
        has_pot_y: true,
        pot_x: 0x5A,
        pot_y: 0xA5,
        ..Default::default()
    };
    let p2 = FakePort::default();
    assert_eq!(read(&mut io, 0x08, 99999, &p1, &p2).0, 0x5A);
    assert_eq!(read(&mut io, 0x09, 0, &p1, &p2).0, 0xA5);
    // Digital fallbacks from joy bits.
    let p1 = FakePort {
        joy: 0x10,
        ..Default::default()
    };
    assert_eq!(read(&mut io, 0x08, 0, &p1, &p2).0, 0x80);
    let p1 = FakePort {
        joy: 0x40,
        ..Default::default()
    };
    assert_eq!(read(&mut io, 0x09, 0, &p1, &p2).0, 0x80);
    let p2 = FakePort {
        joy: 0x10,
        ..Default::default()
    };
    assert_eq!(read(&mut io, 0x0A, 0, &p1, &p2).0, 0x80);
    let p2 = FakePort {
        joy: 0x40,
        ..Default::default()
    };
    assert_eq!(read(&mut io, 0x0B, 0, &p1, &p2).0, 0x80);
    assert_eq!(read(&mut io, 0x08, 0, &p2, &p2).0, 0x00);
}

#[test]
fn inpt4_inpt5_buttons_and_pro_mode() {
    let mut io = TiaIo::new();
    let p2 = FakePort::default();
    // Plain button: bit 0x20 -> 0x80.
    let p1 = FakePort {
        joy: 0x20,
        ..Default::default()
    };
    assert_eq!(read(&mut io, 0x0C, 0, &p1, &p2).0, 0x80);
    assert_eq!(read(&mut io, 0x0C, 0, &p2, &p2).0, 0x00);
    // Pro buttons in one-button mode: A/B (0x50) force the VCS line low.
    let p1 = FakePort {
        joy: 0x50,
        pro: true,
        ..Default::default()
    };
    assert_eq!(read(&mut io, 0x0C, 0, &p1, &p2).0, 0x00);
    // ...but not in two-button mode (plain bit-5 line reads normally).
    io.p1_one_button = false;
    let p1 = FakePort {
        joy: 0x20,
        pro: true,
        ..Default::default()
    };
    assert_eq!(read(&mut io, 0x0C, 0, &p1, &p2).0, 0x80);
    io.p1_one_button = true;
    // Port 2 mirrors on INPT5.
    let p2 = FakePort {
        joy: 0x20,
        ..Default::default()
    };
    assert_eq!(read(&mut io, 0x0D, 0, &p1, &p2).0, 0x80);
    let p2 = FakePort {
        joy: 0x50,
        pro: true,
        ..Default::default()
    };
    assert_eq!(read(&mut io, 0x0D, 0, &p1, &p2).0, 0x00);
}

#[test]
fn lightgun_hit_and_miss_ntsc() {
    let mut io = TiaIo::new();
    let p2 = FakePort::default();
    // light (128,128): gun = ((128*160)/255+95)%227 = 175,
    // ((128*228)/255)+16 = 130. Beam exactly there -> lit (0x00).
    let p1 = FakePort {
        gun: true,
        light_x: 128,
        light_y: 128,
        ..Default::default()
    };
    let (v, _) = io.read(0x0C, 0, beam(350, 130), &p1, &p2);
    assert_eq!(v, 0x00);
    // Within 8 px (dx=5, dy=5 -> 50 < 64) still lit.
    let (v, _) = io.read(0x0C, 0, beam(360, 135), &p1, &p2);
    assert_eq!(v, 0x00);
    // Far away -> dark (0x80).
    let (v, _) = io.read(0x0C, 0, beam(0, 0), &p1, &p2);
    assert_eq!(v, 0x80);
    // Port 2 gun drives INPT5 instead.
    let p2 = FakePort {
        gun: true,
        light_x: 128,
        light_y: 128,
        ..Default::default()
    };
    let p1 = FakePort::default();
    let (v, _) = io.read(0x0D, 0, beam(350, 130), &p1, &p2);
    assert_eq!(v, 0x00);
    // ...and leaves INPT4 on plain-button behavior.
    let (v, _) = io.read(0x0C, 0, beam(350, 130), &p1, &p2);
    assert_eq!(v, 0x00); // joy 0, no bit -> 0x00
}

#[test]
fn lightgun_pal_scaling() {
    let mut io = TiaIo::new();
    let p2 = FakePort::default();
    let p1 = FakePort {
        gun: true,
        light_x: 128,
        light_y: 128,
        ..Default::default()
    };
    // PAL: gun_y = ((128*260)/255)+24 = 154. cpu_y = vpos % 313.
    let pal = BeamState {
        hpos: 350,
        vpos: 154,
        lines: 313,
        is_pal: true,
    };
    let (v, _) = io.read(0x0C, 0, pal, &p1, &p2);
    assert_eq!(v, 0x00);
}

#[test]
fn stall_gate_ticks_every_second_access() {
    let mut io = TiaIo::new();
    let (p1, p2) = (FakePort::default(), FakePort::default());
    // No guns: never stalls.
    for _ in 0..4 {
        assert_eq!(read(&mut io, 0x08, 0, &p1, &p2).1, 0);
    }
    // Gun on port 1: 0, 1, 0, 1, ...
    let p1 = FakePort {
        gun: true,
        ..Default::default()
    };
    let mut stalls = Vec::new();
    for _ in 0..4 {
        stalls.push(read(&mut io, 0x08, 0, &p1, &p2).1);
    }
    assert_eq!(stalls, vec![0, 1, 0, 1]);
    // Writes share the same alternator (delay left at 0 by the reads above).
    let mut audio = TiaAudio::new(31_400, 31_400);
    let s1 = io.write(0x00, 0x00, 0, false, false, &mut audio);
    let s2 = io.write(0x00, 0x00, 0, true, false, &mut audio);
    let s3 = io.write(0x00, 0x00, 0, true, false, &mut audio);
    assert_eq!((s1, s2, s3), (0, 0, 1));
}

#[test]
fn ctrl_lock_vblank_and_audio_routing() {
    let mut io = TiaIo::new();
    let mut audio = TiaAudio::new(31_400, 31_400);
    // Unlocked: INPTCTRL latches reg + lock; bit 0 sets maria_flag.
    assert_eq!(io.write(0x00, 0x01, 0, false, false, &mut audio), 0);
    assert!(io.ctrl_lock);
    assert_eq!(io.ctrl_reg, 0x01);
    assert!(io.maria_flag);
    // Locked: further writes don't change the reg...
    io.write(0x00, 0x03, 0, false, false, &mut audio);
    assert_eq!(io.ctrl_reg, 0x01);
    // ...but offset 0x01 with bit 0 still raises maria_flag.
    io.maria_flag = false;
    io.write(0x01, 0x01, 0, false, false, &mut audio);
    assert!(io.maria_flag);
    // Mirrored writes (offset >= 0x20) skip the lock block.
    let mut io2 = TiaIo::new();
    io2.write(0x120, 0x01, 0, false, false, &mut audio);
    assert!(!io2.ctrl_lock);

    // VBLANK paddle timing: ground then release stamps paddle_start.
    let mut io3 = TiaIo::new();
    io3.write(0x01, 0x80, 5000, false, false, &mut audio);
    io3.write(0x01, 0x00, 6000, false, false, &mut audio);
    assert_eq!(io3.paddle_start, 6000);
    // Release without prior ground: no stamp.
    let mut io4 = TiaIo::new();
    io4.write(0x01, 0x00, 7000, false, false, &mut audio);
    assert_eq!(io4.paddle_start, 0);

    // Audio write-through only when (ctrl_reg & 3) == 3.
    let mut io5 = TiaIo::new();
    io5.write(0x00, 0x03, 0, false, false, &mut audio);
    io5.write(0x19, 0x0F, 0, false, false, &mut audio); // AUDV0
    assert_eq!(audio.outvol(0), 15 << 10);
    let mut audio2 = TiaAudio::new(31_400, 31_400);
    let mut io6 = TiaIo::new();
    io6.write(0x00, 0x01, 0, false, false, &mut audio2);
    io6.write(0x19, 0x0F, 0, false, false, &mut audio2);
    assert_eq!(audio2.outvol(0), 0);
}

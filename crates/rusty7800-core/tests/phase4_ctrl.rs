//! Phase 4 acceptance: every device × every read, empty-slot semantics,
//! keypad matrix, wheel gray code, trakball/mouse quadrature, console byte.

use rusty7800_core::ctrl::console;
use rusty7800_core::ctrl::{
    AmigaMouse, ConsoleState, ControlPort, DrivingWheel, Keypad, LightGun, Paddles, PortDevice,
    ProlineJoystick, StMouse, Trakball, VcsJoystick, KEY_PAUSE, KEY_RESET, KEY_SELECT,
};
use rusty7800_core::tia_io::TiaPort;

#[test]
fn idle_bytes_match_mame_ports() {
    // Idle bytes derived from the INPUT_PORTS active levels + unused bits.
    assert_eq!(ProlineJoystick::new().joy_r(), 0xAF);
    // VCS: dirs + fire button (0x20, active-low, idle 1) + 0x80 unused-high.
    assert_eq!(VcsJoystick::new().joy_r(), 0xAF);
    assert_eq!(Paddles::new().joy_r(), 0xFF);
    assert_eq!(LightGun::new().joy_r(), 0xAE);
    assert_eq!(DrivingWheel::new().joy_r(), 0xAC);
    assert_eq!(Trakball::new().joy_r(), 0xA0);
    // Keypad powers on with column latch 0 = row 0 selected, no keys held:
    // joy bit-2 clear -> 0x00, pot bits clear -> 0xFF (per the C++ scan loops).
    assert_eq!(Keypad::new().joy_r(), 0x00);
    assert_eq!(Keypad::new().pot_x(), 0xFF);
    assert_eq!(Keypad::new().pot_y(), 0xFF);
    assert_eq!(AmigaMouse::new().joy_r(), 0x60);
    assert_eq!(StMouse::new().joy_r(), 0x60);
}

#[test]
fn capability_flags_match_cpp() {
    let pro = ProlineJoystick::new();
    assert!(pro.has_pro_buttons());
    assert!(!pro.has_pot_x() && !pro.is_paddle() && !pro.is_lightgun());
    let vcs = VcsJoystick::new();
    assert!(!vcs.has_pro_buttons());
    let pad = Paddles::new();
    assert!(pad.has_pot_x() && pad.has_pot_y() && pad.is_paddle() && !pad.is_lightgun());
    assert_eq!((pad.pot_x(), pad.pot_y()), (0x80, 0x80));
    let gun = LightGun::new();
    assert!(gun.is_lightgun() && !gun.has_pot_x());
    let key = Keypad::new();
    assert!(key.has_pot_x() && key.has_pot_y() && !key.is_paddle());
    for dev in [
        PortDevice::from_slot("driving_wheel").unwrap(),
        PortDevice::from_slot("cx22_trakball").unwrap(),
        PortDevice::from_slot("amiga_mouse").unwrap(),
        PortDevice::from_slot("st_mouse").unwrap(),
    ] {
        let (px, py, pad, gun, pro) = match &dev {
            PortDevice::Wheel(d) => (d.has_pot_x(), d.has_pot_y(), d.is_paddle(), d.is_lightgun(), d.has_pro_buttons()),
            PortDevice::Trakball(d) => (d.has_pot_x(), d.has_pot_y(), d.is_paddle(), d.is_lightgun(), d.has_pro_buttons()),
            PortDevice::AmigaMouse(d) => (d.has_pot_x(), d.has_pot_y(), d.is_paddle(), d.is_lightgun(), d.has_pro_buttons()),
            PortDevice::StMouse(d) => (d.has_pot_x(), d.has_pot_y(), d.is_paddle(), d.is_lightgun(), d.has_pro_buttons()),
            _ => unreachable!(),
        };
        assert!(!px && !py && !pad && !gun && !pro, "{}", dev.slot_name());
    }
}

#[test]
fn button_and_direction_bits() {
    let mut pro = ProlineJoystick::new();
    pro.up = true;
    pro.btn_a = true;
    pro.btn_b = true;
    assert_eq!(pro.joy_r(), 0xAF & !0x01 | 0x10 | 0x40);
    let mut vcs = VcsJoystick::new();
    vcs.left = true;
    vcs.button = true;
    assert_eq!(vcs.joy_r(), 0xAF & !0x04 & !0x20);
    let mut pad = Paddles::new();
    pad.btn1 = true;
    pad.btn2 = true;
    assert_eq!(pad.joy_r(), 0xF3);
    let mut gun = LightGun::new();
    gun.trigger = true;
    assert_eq!(gun.joy_r(), 0xAE | 0x01);
}

#[test]
fn empty_slot_reads_ff_and_ignores_writes() {
    let mut port = ControlPort::empty();
    assert!(!port.exists());
    assert_eq!(port.joy_r(), 0xFF);
    assert_eq!(port.pot_x(), 0xFF);
    assert_eq!(port.pot_y(), 0xFF);
    assert_eq!(port.light_x(), 0xFF);
    assert_eq!(port.light_y(), 0xFF);
    assert!(!port.has_pot_x() && !port.is_paddle() && !port.is_lightgun());
    port.joy_w(0x00); // ignored, stays empty
    assert!(!port.exists());
    assert_eq!(port.joy_r(), 0xFF);
}

#[test]
fn default_port_is_proline_and_slots_round_trip() {
    let port = ControlPort::new();
    assert!(port.exists());
    assert_eq!(port.joy_r(), 0xAF);
    assert!(port.has_pro_buttons());
    // Every slot name constructs, reports its own name, and unplugs cleanly.
    for name in rusty7800_core::ctrl::SLOT_NAMES {
        let mut p = ControlPort::empty();
        p.select_slot(name);
        assert!(p.exists(), "{name}");
        assert_eq!(p.device().unwrap().slot_name(), name);
    }
    let mut p = ControlPort::new();
    p.select_slot("bogus");
    assert!(!p.exists());
    p.select_slot("keypad");
    p.joy_w(0x0E); // routed to the keypad's row latch
    assert_eq!(p.joy_r(), 0x00); // row 0 selected, nothing pressed
}

#[test]
fn keypad_matrix_rows() {
    let mut key = Keypad::new();
    // Select row 0 (clear bit 0), press key 1 (row 0, bit 0).
    key.joy_w(0x0E);
    key.press(rusty7800_core::ctrl::keypad::KEY_1);
    assert_eq!(key.pot_x(), 0x00); // row bit 0 set
    assert_eq!(key.pot_y(), 0xFF); // row bit 1 clear
    assert_eq!(key.joy_r(), 0x00); // row bit 2 clear
    // Same row, key 3 (row 0, bit 2) flips joy_r only.
    key.release(rusty7800_core::ctrl::keypad::KEY_1);
    key.press(rusty7800_core::ctrl::keypad::KEY_3);
    assert_eq!((key.pot_x(), key.pot_y(), key.joy_r()), (0xFF, 0xFF, 0xFF));
    // Row 3 (# key = index 11 = row 3, bit 2).
    key.release(rusty7800_core::ctrl::keypad::KEY_3);
    key.joy_w(0x07);
    key.press(rusty7800_core::ctrl::keypad::KEY_HASH);
    assert_eq!(key.joy_r(), 0xFF);
    assert_eq!((key.pot_x(), key.pot_y()), (0xFF, 0xFF));
    // No row selected (all bits set): joy 0xFF, pots 0x00.
    key.joy_w(0x0F);
    assert_eq!((key.joy_r(), key.pot_x(), key.pot_y()), (0xFF, 0x00, 0x00));
    // First selected row wins: rows 0+1 selected, key in row 1 hidden.
    key.joy_w(0x0C);
    key.press(rusty7800_core::ctrl::keypad::KEY_5);
    assert_eq!(key.joy_r(), 0x00); // row 0 scanned first, empty
}

#[test]
fn wheel_gray_code_sequence() {
    let mut w = DrivingWheel::new();
    // (wheel & 0x18) >> 3 cycles 0,1,2,3 as counter crosses 8,16,24.
    let mut seen = Vec::new();
    for v in [0x00u8, 0x08, 0x10, 0x18, 0x20] {
        w.wheel = v;
        seen.push(w.joy_r() & 0x03);
    }
    assert_eq!(seen, vec![0x00, 0x02, 0x03, 0x01, 0x00]);
    // spin() wraps like the MAME port byte.
    w.wheel = 0xFF;
    w.spin(1);
    assert_eq!(w.wheel, 0x00);
    // Button clears bit 0x20.
    w.button = true;
    assert_eq!(w.joy_r() & 0x20, 0);
}

#[test]
fn trakball_direction_encoding() {
    let mut t = Trakball::new();
    // X increasing: direction bit set, toggle bit flips each motion.
    t.set_position(1, 0);
    assert_eq!(t.joy_r() & 0x03, 0x03);
    t.set_position(2, 0);
    assert_eq!(t.joy_r() & 0x03, 0x01);
    // X decreasing: direction bit cleared, toggle flips.
    t.set_position(1, 0);
    let xcode = t.joy_r() & 0x03;
    assert_eq!(xcode & 0x01, 0);
    // Y motion lands in pins 3-4.
    let mut t2 = Trakball::new();
    t2.set_position(0, 5);
    assert_eq!((t2.joy_r() >> 2) & 0x03, 0x03);
    // No motion: codes hold, base keeps button/unused bits.
    let before = t2.joy_r();
    t2.set_position(0, 5);
    assert_eq!(t2.joy_r(), before);
    t2.button = true;
    assert_eq!(t2.joy_r() & 0x20, 0);
}

#[test]
fn mouse_quadrature_tables() {
    // Amiga: X pins 2+4, Y pins 1+3.
    let mut m = AmigaMouse::new();
    m.mouse_x = 1;
    m.mouse_y = 2;
    assert_eq!(
        m.joy_r(),
        0x60 | rusty7800_core::ctrl::amigamouse::AXIS_LOOKUP_X[1]
            | rusty7800_core::ctrl::amigamouse::AXIS_LOOKUP_Y[2]
    );
    assert_eq!(
        (
            rusty7800_core::ctrl::amigamouse::AXIS_LOOKUP_X,
            rusty7800_core::ctrl::amigamouse::AXIS_LOOKUP_Y
        ),
        ([0x00, 0x02, 0x0A, 0x08], [0x00, 0x01, 0x05, 0x04])
    );
    // ST: X pins 1+2, Y pins 3+4 — different wiring, same buttons.
    let mut s = StMouse::new();
    s.mouse_x = 3;
    s.mouse_y = 1;
    assert_eq!(s.joy_r(), 0x60 | 0x01 | 0x08);
    assert_eq!(
        (
            rusty7800_core::ctrl::stmouse::AXIS_LOOKUP_X,
            rusty7800_core::ctrl::stmouse::AXIS_LOOKUP_Y
        ),
        ([0x00, 0x02, 0x03, 0x01], [0x00, 0x08, 0x0C, 0x04])
    );
    // Buttons clear their bits on both mice.
    m.btn_left = true;
    m.btn_right = true;
    assert_eq!(m.joy_r() & 0x60, 0);
    // move_by wraps like the MAME port byte.
    m.mouse_x = 0xFF;
    m.move_by(1, -1);
    assert_eq!((m.mouse_x, m.mouse_y), (0x00, 0x01));
}

#[test]
fn console_byte_and_keys() {
    assert_eq!((KEY_RESET, KEY_SELECT, KEY_PAUSE), ('T', 'E', 'O'));
    // Idle: buttons released (1), unused set, difficulty A (1,1).
    assert_eq!(ConsoleState::new().byte(), 0xFF);
    let mut c = ConsoleState::new();
    c.reset = true;
    c.select = true;
    c.pause = true;
    assert_eq!(c.byte(), 0xF4);
    // Difficulty B (left position) clears its bit.
    c = ConsoleState::new();
    c.left_b = true;
    c.right_b = true;
    assert_eq!(c.byte(), 0x3F);
}

#[test]
fn riot_port_wiring() {
    // Nibble mux: P1 dirs high, P2 dirs low.
    assert_eq!(console::riot_joystick_byte(0xAF, 0xCF), 0xFF);
    assert_eq!(console::riot_joystick_byte(0xA0, 0xC5), 0x05);
    // Write split: high nibble to P1, low to P2.
    assert_eq!(console::riot_joystick_split(0xE7), (0x0E, 0x07));
    // Pull-up gate: DDR 0x283 bit enables each latch; DRIVER_INIT = set.
    let (mut p1, mut p2) = (true, true);
    console::pullup_write(0xFF, 0x00, &mut p1, &mut p2);
    assert!(!p1 && !p2);
    console::pullup_write(0x04, 0x00, &mut p1, &mut p2);
    assert!(!p1 && !p2); // p2 DDR bit clear: latch holds
    console::pullup_write(0x10, 0xFF, &mut p1, &mut p2);
    assert!(!p1 && p2);
}

#[test]
fn ports_drive_tia_io_end_to_end() {
    use rusty7800_core::tia_io::{BeamState, TiaIo};
    let beam = BeamState {
        hpos: 0,
        vpos: 0,
        lines: 263,
        is_pal: false,
    };
    // Proline A button in one-button mode pulls INPT4 low.
    let mut io = TiaIo::new();
    let mut p1 = ControlPort::new();
    let p2 = ControlPort::empty();
    if let Some(PortDevice::Proline(d)) = p1.device_mut() {
        d.btn_a = true;
    }
    assert_eq!(io.read(0x0C, 0, beam, &p1, &p2).0, 0x00);
    // Paddle on port 2: INPT2 uses the discharge ramp.
    let mut p2pad = ControlPort::empty();
    p2pad.select_slot("paddle");
    if let Some(PortDevice::Paddles(d)) = p2pad.device_mut() {
        d.pot_x = 10;
    }
    io.paddle_start = 0;
    // INPT2 applies the `pot ^ 0xFF` quirk: threshold is 10 ^ 0xFF = 245.
    assert_eq!(io.read(0x0A, 5 * 97, beam, &p1, &p2pad).0, 0x00);
    assert_eq!(io.read(0x0A, 246 * 97, beam, &p1, &p2pad).0, 0x80);
    // Empty port 2 reads 0xFF, so the digital fallback sees bit 0x20 set.
    assert_eq!(io.read(0x0D, 0, beam, &p1, &p2).0, 0x80);
}

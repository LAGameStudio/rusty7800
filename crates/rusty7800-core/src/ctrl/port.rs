//! Control-port slot: device picker + null-safety semantics.
//!
//! Phase 4 port of `a7800-5.2/src/devices/bus/a7800_ctrl/ctrl.{h,cpp}`
//! (BSD-3-Clause, slot mechanics only). Slot names and order match
//! `SLOT_INTERFACE_START(a7800_control_port_devices)`; the machine
//! default for both ports is `proline_joystick` (per `MACHINE_CONFIG`).
//! An empty slot reads `0xFF` on every query and ignores writes
//! (`ctrl.h`: `joy_r/pot_x_r/…` null-safety).

use crate::tia_io::TiaPort;

use super::amigamouse::AmigaMouse;
use super::atarivox::AtariVox;
use super::keypad::Keypad;
use super::lightgun::LightGun;
use super::paddles::Paddles;
use super::proline::ProlineJoystick;
use super::stmouse::StMouse;
use super::trakball::Trakball;
use super::vcs::VcsJoystick;
use super::wheel::DrivingWheel;

/// Slot names in `SLOT_INTERFACE` order (also mirrored in `specs.rs`).
pub const SLOT_NAMES: [&str; 11] = [
    "proline_joystick",
    "vcs_joystick",
    "paddle",
    "lightgun",
    "driving_wheel",
    "cx22_trakball",
    "keypad",
    "amiga_mouse",
    "st_mouse",
    "atarivox",
    "savekey",
];

/// Default device for both ports (machine config default slot).
pub const DEFAULT_SLOT: &str = "proline_joystick";

/// A pluggable control-port device.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum PortDevice {
    Proline(ProlineJoystick),
    Vcs(VcsJoystick),
    Paddles(Paddles),
    LightGun(LightGun),
    Wheel(DrivingWheel),
    Trakball(Trakball),
    Keypad(Keypad),
    AmigaMouse(AmigaMouse),
    StMouse(StMouse),
    AtariVox(AtariVox),
    SaveKey(AtariVox),
}

impl PortDevice {
    /// Build a device by slot name (unknown names yield `None`).
    pub fn from_slot(name: &str) -> Option<Self> {
        match name {
            "proline_joystick" => Some(PortDevice::Proline(ProlineJoystick::new())),
            "vcs_joystick" => Some(PortDevice::Vcs(VcsJoystick::new())),
            "paddle" => Some(PortDevice::Paddles(Paddles::new())),
            "lightgun" => Some(PortDevice::LightGun(LightGun::new())),
            "driving_wheel" => Some(PortDevice::Wheel(DrivingWheel::new())),
            "cx22_trakball" => Some(PortDevice::Trakball(Trakball::new())),
            "keypad" => Some(PortDevice::Keypad(Keypad::new())),
            "amiga_mouse" => Some(PortDevice::AmigaMouse(AmigaMouse::new())),
            "st_mouse" => Some(PortDevice::StMouse(StMouse::new())),
            "atarivox" => Some(PortDevice::AtariVox(AtariVox::new())),
            "savekey" => Some(PortDevice::SaveKey(AtariVox::savekey())),
            _ => None,
        }
    }

    pub fn slot_name(&self) -> &'static str {
        match self {
            PortDevice::Proline(_) => "proline_joystick",
            PortDevice::Vcs(_) => "vcs_joystick",
            PortDevice::Paddles(_) => "paddle",
            PortDevice::LightGun(_) => "lightgun",
            PortDevice::Wheel(_) => "driving_wheel",
            PortDevice::Trakball(_) => "cx22_trakball",
            PortDevice::Keypad(_) => "keypad",
            PortDevice::AmigaMouse(_) => "amiga_mouse",
            PortDevice::StMouse(_) => "st_mouse",
            PortDevice::AtariVox(_) => "atarivox",
            PortDevice::SaveKey(_) => "savekey",
        }
    }
}

/// A control port with a pluggable (or empty) device.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ControlPort {
    device: Option<PortDevice>,
}

impl Default for ControlPort {
    /// Machine default: Proline joystick in both ports.
    fn default() -> Self {
        ControlPort {
            device: PortDevice::from_slot(DEFAULT_SLOT),
        }
    }
}

impl ControlPort {
    pub fn new() -> Self {
        Self::default()
    }

    /// Empty slot: every read returns `0xFF`, writes are ignored.
    pub fn empty() -> Self {
        ControlPort { device: None }
    }

    pub fn exists(&self) -> bool {
        self.device.is_some()
    }

    pub fn plug(&mut self, device: PortDevice) {
        self.device = Some(device);
    }

    /// Select a device by slot name; unknown names unplug the port.
    pub fn select_slot(&mut self, name: &str) {
        self.device = PortDevice::from_slot(name);
    }

    pub fn unplug(&mut self) {
        self.device = None;
    }

    pub fn device(&self) -> Option<&PortDevice> {
        self.device.as_ref()
    }

    pub fn device_mut(&mut self) -> Option<&mut PortDevice> {
        self.device.as_mut()
    }

    /// Device-driven direction-pin levels for RIOT PA input bits
    /// (empty slot floats to pull-up high).
    pub fn pin_levels(&mut self, now: u64) -> u8 {
        match &mut self.device {
            Some(PortDevice::Proline(d)) => d.pin_levels(now),
            Some(PortDevice::Vcs(d)) => d.pin_levels(now),
            Some(PortDevice::Paddles(d)) => d.pin_levels(now),
            Some(PortDevice::LightGun(d)) => d.pin_levels(now),
            Some(PortDevice::Wheel(d)) => d.pin_levels(now),
            Some(PortDevice::Trakball(d)) => d.pin_levels(now),
            Some(PortDevice::Keypad(d)) => d.pin_levels(now),
            Some(PortDevice::AmigaMouse(d)) => d.pin_levels(now),
            Some(PortDevice::StMouse(d)) => d.pin_levels(now),
            Some(PortDevice::AtariVox(d)) => d.pin_levels(now),
            Some(PortDevice::SaveKey(d)) => d.pin_levels(now),
            None => 0x0F,
        }
    }

    /// Forward a console pin drive to the plugged device.
    pub fn pin_write(&mut self, levels: u8, mask: u8, now: u64) {
        match &mut self.device {
            Some(PortDevice::Proline(d)) => d.pin_write(levels, mask, now),
            Some(PortDevice::Vcs(d)) => d.pin_write(levels, mask, now),
            Some(PortDevice::Paddles(d)) => d.pin_write(levels, mask, now),
            Some(PortDevice::LightGun(d)) => d.pin_write(levels, mask, now),
            Some(PortDevice::Wheel(d)) => d.pin_write(levels, mask, now),
            Some(PortDevice::Trakball(d)) => d.pin_write(levels, mask, now),
            Some(PortDevice::Keypad(d)) => d.pin_write(levels, mask, now),
            Some(PortDevice::AmigaMouse(d)) => d.pin_write(levels, mask, now),
            Some(PortDevice::StMouse(d)) => d.pin_write(levels, mask, now),
            Some(PortDevice::AtariVox(d)) => d.pin_write(levels, mask, now),
            Some(PortDevice::SaveKey(d)) => d.pin_write(levels, mask, now),
            None => {}
        }
    }
}

macro_rules! delegate {
    ($self:ident, $method:ident () -> $ret:ty, $empty:expr) => {
        match &$self.device {
            Some(PortDevice::Proline(d)) => d.$method(),
            Some(PortDevice::Vcs(d)) => d.$method(),
            Some(PortDevice::Paddles(d)) => d.$method(),
            Some(PortDevice::LightGun(d)) => d.$method(),
            Some(PortDevice::Wheel(d)) => d.$method(),
            Some(PortDevice::Trakball(d)) => d.$method(),
            Some(PortDevice::Keypad(d)) => d.$method(),
            Some(PortDevice::AmigaMouse(d)) => d.$method(),
            Some(PortDevice::StMouse(d)) => d.$method(),
            Some(PortDevice::AtariVox(d)) => d.$method(),
            Some(PortDevice::SaveKey(d)) => d.$method(),
            None => $empty,
        }
    };
}

impl TiaPort for ControlPort {
    fn joy_r(&self) -> u8 {
        delegate!(self, joy_r() -> u8, 0xFF)
    }
    fn pot_x(&self) -> u8 {
        delegate!(self, pot_x() -> u8, 0xFF)
    }
    fn pot_y(&self) -> u8 {
        delegate!(self, pot_y() -> u8, 0xFF)
    }
    fn light_x(&self) -> u8 {
        delegate!(self, light_x() -> u8, 0xFF)
    }
    fn light_y(&self) -> u8 {
        delegate!(self, light_y() -> u8, 0xFF)
    }
    fn has_pot_x(&self) -> bool {
        delegate!(self, has_pot_x() -> bool, false)
    }
    fn has_pot_y(&self) -> bool {
        delegate!(self, has_pot_y() -> bool, false)
    }
    fn is_paddle(&self) -> bool {
        delegate!(self, is_paddle() -> bool, false)
    }
    fn is_lightgun(&self) -> bool {
        delegate!(self, is_lightgun() -> bool, false)
    }
    fn has_pro_buttons(&self) -> bool {
        delegate!(self, has_pro_buttons() -> bool, false)
    }

    fn joy_w(&mut self, data: u8) {
        // Empty slots ignore writes (ctrl.h `joy_w` null-safety).
        if let Some(PortDevice::Keypad(d)) = &mut self.device {
            d.joy_w(data);
        }
    }
}

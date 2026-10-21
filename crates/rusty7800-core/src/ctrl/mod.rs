//! Rusty7800 `ctrl` — control-port controllers.
//!
//! Phase 4 port of `a7800-5.2/src/devices/bus/a7800_ctrl/*` (BSD-3-Clause)
//! plus the driver-side port wiring in `a7800.cpp` (`riot_joystick_r/w`,
//! `riot_console_button_r`, `riot_button_pullup_w`, console `INPUT_PORTS`).
//! All 11 slot devices implement [`TiaPort`](crate::tia_io::TiaPort) so the
//! Phase 3 `TiaIo` read path queries them directly; [`ControlPort`] adds
//! the empty-slot (`0xFF` / ignore-write) semantics from `ctrl.h`.
//! Slots 10–11 (`atarivox`/`savekey`) are new — MAME's slot bus has no
//! such device; see [`atarivox`] for the hardware reference.

pub mod atarivox;
pub mod proline;
pub mod vcs;
pub mod paddles;
pub mod lightgun;
pub mod wheel;
pub mod trakball;
pub mod amigamouse;
pub mod stmouse;
pub mod keypad;
pub mod port;
pub mod console;

pub use atarivox::AtariVox;
pub use proline::ProlineJoystick;
pub use vcs::VcsJoystick;
pub use paddles::Paddles;
pub use lightgun::LightGun;
pub use wheel::DrivingWheel;
pub use trakball::Trakball;
pub use amigamouse::AmigaMouse;
pub use stmouse::StMouse;
pub use keypad::Keypad;
pub use port::{ControlPort, PortDevice, DEFAULT_SLOT, SLOT_NAMES};
pub use console::{ConsoleState, KEY_PAUSE, KEY_RESET, KEY_SELECT};

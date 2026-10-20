//! Rusty7800 emulation core — Atari 7800.
//!
//! Phase 0: spec constants + palette tables + module skeleton.
//! Phases 1+ implement each module against the A7800 5.2 sources cited within.

#![forbid(unsafe_code)]

pub mod specs;
pub mod palette;

pub mod sally;
pub mod maria;
pub mod tia;
pub mod tia_io;
pub mod riot;
pub mod pokey;
pub mod ym2151;
pub mod bus;
pub mod cart;
pub mod ctrl;
pub mod timing;
pub mod state;
pub mod socket;

/// Crate name of the emulator.
pub const EMULATOR_NAME: &str = "Rusty7800";

//! Rusty7800 `state` — versioned save-states + NVRAM helpers.
//!
//! Phase 8 port of the MAME `save_item` coverage for the `a7800*`
//! machines (driver + MARIA + carts + RIOT/POKEY/TIA state, per the plan
//! §2 system checklist). [`Snapshot`] captures a whole [`Machine`](crate::machine::Machine)
//! — CPU, MARIA, TIA audio + I/O, RIOT, SRAM, BIOS image, ports, console,
//! cartridge blob, raster position, current framebuffer, pending audio,
//! and DMA stats — so save/load round-trips mid-frame exactly. ROM bytes
//! are immutable and travel with the loaded image, never the snapshot:
//! loading requires the same slot key (checked, not assumed).

use serde::{Deserialize, Serialize};

use crate::cart::SavedCartState;
use crate::ctrl::{ConsoleState, ControlPort};
use crate::maria::MariaState;
use crate::riot::Riot;
use crate::sally::Sally;
use crate::tia::TiaState;
use crate::tia_io::TiaIo;

/// Save-state layout version (bump on any incompatible change).
pub const SNAPSHOT_VERSION: u32 = 1;

/// Cartridge identity + mutable blob. Identity (not image bytes) is what
/// `load_snapshot` matches against the live cart.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedCart {
    pub slot_key: String,
    pub pcb_id: i32,
    pub state: SavedCartState,
}

/// Whole-machine save-state. All bulk fields are length-checked on load
/// (same-machine snapshots always match; anything else is rejected).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    /// MAME driver name (`MachineId::driver_name`).
    pub machine: String,
    pub sally: Sally,
    pub maria: MariaState,
    pub audio: TiaState,
    pub tia: TiaIo,
    pub riot: Riot,
    pub sram1: Vec<u8>,
    pub sram2: Vec<u8>,
    pub riot_ram: Vec<u8>,
    pub bios_present: bool,
    pub bios: Vec<u8>,
    pub port1: ControlPort,
    pub port2: ControlPort,
    pub console: ConsoleState,
    pub cart: Option<SavedCart>,
    pub vpos: u16,
    pub frame: Vec<u8>,
    pub samples: Vec<i16>,
    pub clock_halves: u64,
    pub audio_frac: f64,
    /// Last completed frame's per-line DMA cycles (`screen.frame` source).
    pub dma_frame: Vec<i32>,
    /// In-progress frame's per-line DMA cycles (exact mid-frame resume).
    pub dma_accum: Vec<i32>,
}

/// Save-state load failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    /// `found` layout version is not `SNAPSHOT_VERSION`.
    Version(u32),
    /// Snapshot is for another machine (`expected` live, `found` file).
    Machine { expected: String, found: String },
    /// Snapshot cart identity differs (`None` = no cart).
    Cart { expected: Option<String>, found: Option<String> },
    /// A bulk field has the wrong length for this machine.
    Shape { field: &'static str, len: usize },
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::Version(v) => write!(f, "unsupported snapshot version {v}"),
            SnapshotError::Machine { expected, found } => {
                write!(f, "snapshot is for machine {found}, live machine is {expected}")
            }
            SnapshotError::Cart { expected, found } => {
                write!(f, "snapshot cart {found:?} does not match live cart {expected:?}")
            }
            SnapshotError::Shape { field, len } => {
                write!(f, "snapshot field {field} has wrong length {len}")
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

/// 32-bit FNV-1a over bytes (NVRAM hashes, frame hashes, replay checks).
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

//! Rusty7800 agent-socket surface (plan Phase 8).
//!
//! The protocol core (transport-agnostic JSON-RPC dispatch) lives in
//! [`rusty7800_core::socket`]; the schema is `PROTOCOL.md` next to this
//! file. The app shell (later phase) binds [`AgentSession`] to its
//! token-gated localhost transport; headless harnesses use it in-process.
//!
//! [`AgentSession`]: rusty7800_core::socket::AgentSession

pub use rusty7800_core::socket::*;
pub use rusty7800_core::timing::MachineId;

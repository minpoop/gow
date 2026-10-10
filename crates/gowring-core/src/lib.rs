//! gowring-core — Ragnarök's rules as a host-independent engine.
//!
//! The engine never touches a game. Every frame the host (the Elden Ring DLL, or the headless
//! [`sim`] used by the tests) hands it a [`Frame`] and applies the [`Command`]s that come back.
//! Everything tunable lives in `design/*.json` and arrives here as generated statics ([`sheets`]).

pub mod sheets {
    #![allow(dead_code, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/sheets_gen.rs"));
}

pub mod camera;
pub mod data;
pub mod engine;
pub mod hud;
pub mod input;
pub mod melee;
pub mod mimir;
pub mod rage;
pub mod runic;
pub mod sim;
pub mod stun;
pub mod thrown;
pub mod types;

pub use data::Data;
pub use engine::{Engine, TickOutput};
pub use types::*;

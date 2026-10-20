//! Reader for the player's own God of War Ragnarök folder.
//!
//! Everything here *reads* the install and writes converted copies into a local cache. Nothing from
//! the game is ever shipped. Formats follow the community's published tooling (Ragnarök's
//! `.audiopack.toc` / `.audiopack`, the GoW WAD container, Wwise `.bnk`, DDS); every parser is
//! defensive and reports what it could not understand so `--diagnose` can say exactly what a real
//! install looks like. See docs/STATUS.md for which of these have met a real install (none yet).

pub mod audiopack;
pub mod bnk;
pub mod catalog;
pub mod convert;
pub mod dds;
pub mod erpart;
pub mod lodpack;
pub mod retarget;
pub mod lz4;
pub mod diagnose;
pub mod hudmap;
pub mod kratos;
pub mod kratos_look;
pub mod kratos_part;
pub mod oodle;
pub mod prepare;
pub mod scan;
pub mod texpack;
pub mod wad;
pub mod wadtex;
pub mod weapon_part;
pub mod wtoc;

pub use catalog::{Catalog, CatalogEntry, EntryKind};
pub use prepare::{AssetReq, AssetState, PrepareReport};

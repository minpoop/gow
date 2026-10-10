//! Design sheets for the Kratos-in-Elden-Ring mashup.
//!
//! * [`model`]   – load the JSON sheets from `design/`.
//! * [`validate`] – the preflight: every unfilled cell, bad value and unresolved reference.
//! * [`codegen`]  – one Rust struct per sheet, one literal per row (used by `gowring-core/build.rs`).

pub mod codegen;
pub mod model;
pub mod validate;

pub use model::{ColType, Column, Sheet, Sheets};
pub use validate::{check, Finding, Report, Severity};

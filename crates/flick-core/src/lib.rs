//! Core Flick domain types, configuration structs, extension traits and errors.
//!
//! This crate is intentionally I/O-free. Runtime crates own devices, network, storage and
//! subprocesses; they exchange these shared types at the boundaries defined by the spec.

pub mod action;
pub mod config;
pub mod error;
pub mod frame;
pub mod gesture;
pub mod hands;
pub mod ids;
pub mod packs;
pub mod targeting;
pub mod traits;

pub use action::*;
pub use config::*;
pub use error::*;
pub use frame::*;
pub use gesture::*;
pub use hands::*;
pub use ids::*;
pub use packs::*;
pub use targeting::*;
pub use traits::*;

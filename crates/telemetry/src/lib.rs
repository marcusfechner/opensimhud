//! Game-agnostic sim-racing telemetry.
//!
//! Adapters translate one game's raw data into the normalized [`model`]. Everything derived
//! from it (gaps, relative, fuel to finish, lap log, …) lives in `core`, so every game gets
//! it for free. This crate has no Tauri dependency, so it can be tested and reused by the CLI.

pub mod adapter;
pub mod adapters;
pub mod core;
pub mod input;
pub mod model;
pub mod stream;

pub use adapter::{AdapterEvent, GameAdapter};
pub use core::Session;

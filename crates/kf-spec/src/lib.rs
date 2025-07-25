#![forbid(unsafe_code)]

//! Inert, compile-time access to the frozen Key Frame v1 specification assets.
//!
//! This crate has no parser and no codec algorithm. Consumers receive the same
//! reviewed bytes that drive the independent oracle and generated normative
//! document; implementations may interpret them, but may not replace them
//! with implementation-owned tables.

mod assets;

pub use assets::{Asset, V1_ASSETS};

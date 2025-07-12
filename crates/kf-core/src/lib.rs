#![forbid(unsafe_code)]

//! Production-side primitives shared by the codec crates: bit-level input and
//! output, fixed-point helpers, the error taxonomy, and the deterministic
//! generator.
//!
//! Nothing in this crate or anything above it may use `f32` or `f64`, iterate
//! an unordered container, or read a clock. Those three rules are why this
//! crate exists, and the forbidden-API scan enforces them at the workspace
//! boundary. Floating point belongs to the tools' metrics and the inspector's
//! interface, where it cannot reach a decoded pixel.
//!
//! The reference decoder deliberately does **not** depend on this crate. It
//! owns its own reader, checksum, arithmetic, and state machine, so that the
//! two decoders cannot share a wrong assumption; the dependency-boundary check
//! fails the build if an edge ever appears.
//!
//! Nothing here is normative. The bit layouts, tables, and rounding rules these
//! helpers implement are frozen as inert assets under `spec/`, and every
//! routine is checked against the independent oracle's vectors rather than
//! against its own output. A checksum or a bit reader that became the de facto
//! specification is the one failure this project has decided it must not have.

mod bitio;
mod error;
mod fixed;
mod rng;

pub use bitio::{BitReader, BitWriter};
pub use error::CoreError;
pub use fixed::{clamp_u8, rounded_shift_i64};
pub use rng::Xoshiro256PlusPlus;

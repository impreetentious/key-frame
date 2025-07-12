#![forbid(unsafe_code)]

//! Storage-only planar 8-bit frames shared by the production and reference
//! decoders.
//!
//! This crate deliberately contains no prediction, transform, filtering,
//! padding, or other codec arithmetic. Keeping the shared executable surface
//! inert prevents the two decoders from agreeing through a common bug.

mod frame;
mod plane;

pub use frame::{Frame, FrameError};
pub use plane::{Plane, PlaneError};

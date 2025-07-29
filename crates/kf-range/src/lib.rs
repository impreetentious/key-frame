#![forbid(unsafe_code)]

//! Adaptive binary contexts and the byte-oriented Key Frame range coder.

mod accounting;
mod cost;
mod decoder;
mod encoder;
mod error;
mod probability;

pub use accounting::{EmissionEvent, EncodedRange, RangeStats};
pub use cost::modeled_cost_q16;
pub use decoder::RangeDecoder;
pub use encoder::RangeEncoder;
pub use error::RangeError;
pub use probability::{CONTEXT_COUNT, ContextBank, Probability};

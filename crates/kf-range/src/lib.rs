#![forbid(unsafe_code)]

//! Adaptive binary contexts and the byte-oriented Key Frame range coder.

mod cost;
mod error;
mod probability;

pub use cost::modeled_cost_q16;
pub use error::RangeError;
pub use probability::{CONTEXT_COUNT, ContextBank, Probability};

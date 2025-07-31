#![forbid(unsafe_code)]

//! Independent, readability-first Key Frame reference decoder.

mod crc;
mod decoder;
mod error;
mod motion;
mod predict;
mod range;
mod reader;
mod syntax;
mod transform;

pub use decoder::ReferenceDecoder;
pub use error::ReferenceError;

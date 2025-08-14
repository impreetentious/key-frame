#![forbid(unsafe_code)]

//! Independent, readability-first Key Frame reference decoder.

mod crc;
mod deblock;
mod decoder;
mod error;
mod motion;
mod predict;
mod range;
mod reader;
mod scan;
mod status;
mod syntax;
mod transform;

pub use decoder::ReferenceDecoder;
pub use error::ReferenceError;
pub use status::{RefFrameStatus, RefRecovery, RefStreamReport};

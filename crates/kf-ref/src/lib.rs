#![forbid(unsafe_code)]

//! Independent, readability-first Key Frame reference decoder.

mod coverage;
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

pub use coverage::{ReferenceCoverage, ReferenceElement};
pub use decoder::{RefSeek, ReferenceDecoder};
pub use error::ReferenceError;
pub use status::{RefFrameStatus, RefRecovery, RefStreamReport};

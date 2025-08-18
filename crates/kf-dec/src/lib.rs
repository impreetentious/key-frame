#![forbid(unsafe_code)]

//! Fast, scalar Key Frame decoder.

mod decoder;
mod error;
mod reconstruct;
mod seek;
mod status;

pub use decoder::{FastDecoder, StreamCoverage};
pub use error::DecodeError;
pub use seek::{SeekOutcome, StreamIndex};
pub use status::{FrameStatus, Recovery, StreamReport};

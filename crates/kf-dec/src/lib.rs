#![forbid(unsafe_code)]

//! Fast, scalar Key Frame decoder.

mod decoder;
mod error;
mod reconstruct;
mod status;

pub use decoder::{FastDecoder, StreamCoverage};
pub use error::DecodeError;
pub use status::{FrameStatus, Recovery, StreamReport};

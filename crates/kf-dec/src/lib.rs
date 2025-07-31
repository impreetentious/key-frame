#![forbid(unsafe_code)]

//! Fast, scalar Key Frame decoder.

mod decoder;
mod error;
mod reconstruct;

pub use decoder::FastDecoder;
pub use error::DecodeError;

#![forbid(unsafe_code)]

//! Deterministic scalar Key Frame encoder.

mod encoder;
mod error;
mod gop;
mod motion_search;
mod rate;

pub use encoder::{
    BlockAccounting, EncodedStream, Encoder, FrameAccounting, IntraEncoder, SuperblockAccounting,
};
pub use error::EncodeError;
pub use gop::{FrameDecision, GopPlanner};
pub use rate::{RateControl, RateController};

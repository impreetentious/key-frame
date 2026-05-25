#![forbid(unsafe_code)]

//! Deterministic scalar Key Frame encoder.

mod encoder;
mod error;
mod gop;
mod motion_search;
mod rate;
mod toolset;

/// The largest quantizer the specification declares, read rather than restated.
///
/// Both places this encoder validates a quantizer carried `63`. The declaration
/// was reconciled against `quant.toml` and read by neither, so a narrowed range
/// would have left the encoder emitting streams the decoders refuse.
pub(crate) fn declared_qp_max() -> u8 {
    use std::sync::OnceLock;
    static MAX: OnceLock<u8> = OnceLock::new();
    *MAX.get_or_init(|| {
        let asset = kf_spec::V1_ASSETS
            .iter()
            .find(|asset| asset.name == "constants.toml")
            .expect("invariant: kf-spec exposes constants.toml");
        asset
            .contents
            .lines()
            .find_map(|line| line.strip_prefix("qp_max = ")?.trim().parse().ok())
            .expect("invariant: checked constants declare qp_max")
    })
}

pub use encoder::{
    BlockAccounting, EncodedStream, Encoder, FrameAccounting, IntraEncoder, SuperblockAccounting,
};
pub use error::EncodeError;
pub use gop::{FrameDecision, GopPlanner};
pub use rate::{RateControl, RateController};
pub use toolset::Toolset;

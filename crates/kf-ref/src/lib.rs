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

/// The largest coefficient magnitude a conformant stream may carry.
///
/// This decoder reads the declaration through its own parser, as it does the
/// deblock tables, the context initials, the interpolation taps, and the packet
/// payload bounds. Reading the same normative bytes is not sharing an
/// implementation; typing the same number as the other decoder is how two
/// implementations come to agree with each other rather than with the
/// specification, which is the one way their agreement stops being evidence.
pub(crate) fn declared_coefficient_abs_max() -> u32 {
    use std::sync::OnceLock;
    static MAX: OnceLock<u32> = OnceLock::new();
    *MAX.get_or_init(|| {
        let asset = kf_spec::V1_ASSETS
            .iter()
            .find(|asset| asset.name == "quant.toml")
            .expect("invariant: kf-spec exposes quant.toml");
        asset
            .contents
            .lines()
            .find_map(|line| {
                line.strip_prefix("coefficient_abs_max = ")?
                    .trim()
                    .parse()
                    .ok()
            })
            .expect("invariant: checked quant asset declares coefficient_abs_max")
    })
}

pub use coverage::{ReferenceCoverage, ReferenceElement};
pub use decoder::{RefSeek, ReferenceDecoder};
pub use error::ReferenceError;
pub use status::{RefFrameStatus, RefRecovery, RefStreamReport};

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

/// One declared scalar, read through this decoder's own parser.
///
/// Reading the same normative bytes as the other decoder is not sharing an
/// implementation. Typing the same number is: it is how two implementations
/// come to agree with each other rather than with the specification, which is
/// the one way their agreement stops being evidence.
///
/// The production side is held to these by
/// `crates/kf-bitstream/tests/normative_limits.rs`, which drives its real
/// constructors with the declared values. This side had no counterpart, so its
/// picture bounds, quantizer range, bit depth, chroma code, and coefficient cap
/// were literals that agreed with the specification only because someone typed
/// them twice.
pub(crate) fn declared(asset_name: &str, key: &str) -> u32 {
    let asset = kf_spec::V1_ASSETS
        .iter()
        .find(|asset| asset.name == asset_name)
        .unwrap_or_else(|| panic!("invariant: kf-spec exposes {asset_name}"));
    let prefix = format!("{key} = ");
    asset
        .contents
        .lines()
        .find_map(|line| line.strip_prefix(&prefix)?.trim().parse().ok())
        .unwrap_or_else(|| panic!("invariant: {asset_name} declares {key}"))
}

/// The declared limits this decoder enforces, read once.
pub(crate) struct DeclaredLimits {
    pub min_width: u32,
    pub max_width: u32,
    pub min_height: u32,
    pub max_height: u32,
    pub superblock_size: u32,
    pub qp_max: u8,
    pub bit_depth: u8,
    pub chroma_code: u8,
    pub coefficient_abs_max: u32,
    pub payload_minimum: u32,
    pub payload_maximum: u32,
}

pub(crate) fn limits() -> &'static DeclaredLimits {
    use std::sync::OnceLock;
    static LIMITS: OnceLock<DeclaredLimits> = OnceLock::new();
    LIMITS.get_or_init(|| {
        let byte = |key: &str| {
            u8::try_from(declared("constants.toml", key))
                .unwrap_or_else(|_| panic!("invariant: {key} fits a byte"))
        };
        DeclaredLimits {
            min_width: declared("constants.toml", "min_width"),
            max_width: declared("constants.toml", "max_width"),
            min_height: declared("constants.toml", "min_height"),
            max_height: declared("constants.toml", "max_height"),
            superblock_size: declared("constants.toml", "superblock_size"),
            qp_max: byte("qp_max"),
            bit_depth: byte("bit_depth"),
            chroma_code: byte("chroma_code"),
            coefficient_abs_max: declared("quant.toml", "coefficient_abs_max"),
            payload_minimum: declared("constants.toml", "decoder_initial_bytes"),
            payload_maximum: declared("constants.toml", "max_payload_bytes"),
        }
    })
}

pub use coverage::{ReferenceCoverage, ReferenceElement};
pub use decoder::{RefSeek, ReferenceDecoder};
pub use error::ReferenceError;
pub use status::{RefFrameStatus, RefRecovery, RefStreamReport};

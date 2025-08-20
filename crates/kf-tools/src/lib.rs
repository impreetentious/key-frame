#![forbid(unsafe_code)]

//! Host-side command-line tooling for Key Frame streams and metrics.

mod conformance;
mod hash;
mod y4m;

pub use conformance::{HandVector, Residual, hand_vectors, ladder_levels, transform_schedule};
pub use hash::sha256_hex;
// Re-exported so the command-line tools keep one import surface even though
// the probe now lives where the WebAssembly surface can reach it too.
pub use kf_probe::{BlockProbe, ProbeReport, SuperblockProbe, probe_stream};
pub use y4m::{Y4mError, Y4mStream, decode_y4m, encode_y4m};

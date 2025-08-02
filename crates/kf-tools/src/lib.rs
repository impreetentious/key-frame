#![forbid(unsafe_code)]

//! Host-side command-line tooling for Key Frame streams and metrics.

mod hash;
mod probe;

pub use hash::sha256_hex;
pub use probe::{BlockProbe, ProbeReport, SuperblockProbe, probe_stream};

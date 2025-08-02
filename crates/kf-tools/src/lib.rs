#![forbid(unsafe_code)]

//! Host-side command-line tooling for Key Frame streams and metrics.

mod hash;
mod probe;
mod y4m;

pub use hash::sha256_hex;
pub use probe::{BlockProbe, ProbeReport, SuperblockProbe, probe_stream};
pub use y4m::{Y4mError, Y4mStream, decode_y4m, encode_y4m};

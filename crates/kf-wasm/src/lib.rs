#![deny(unsafe_code)]

//! The WebAssembly surface over the fast decoder.
//!
//! Two layers, kept apart on purpose. The session module holds the decode logic in
//! ordinary safe Rust and is tested natively. The boundary module is the export:
//! a handful of `extern "C"` functions over the module's linear memory, no
//! imports, and the only `unsafe` in the repository, each block naming the
//! invariant the host must uphold.
//!
//! Because the module imports nothing, the artifact the application loads is
//! the same artifact the equality gate runs, which is the only way a
//! native-versus-WebAssembly hash comparison means anything.

// The single, named exception to the repository-wide ban. It is declared here,
// at the crate root, so that a reader meets it before the module rather than
// discovering it buried inside one. Every `unsafe` block inside carries the
// invariant the host must uphold for it.
#[allow(unsafe_code)]
mod abi;
mod session;

pub use abi::ABI_VERSION;
pub use session::{Session, Status, StreamInfo};

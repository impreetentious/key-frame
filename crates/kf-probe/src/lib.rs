#![forbid(unsafe_code)]

//! Syntax reporting: what a stream says, block by block, as structured data.
//!
//! This is the parse layer plus the shadow canonical replay that reconciles
//! modeled entropy against emitted bytes. It reads streams; it never decodes
//! pixels, which is why it depends on the syntax and entropy layers and on
//! neither decoder.
//!
//! It lives in its own crate because two very different callers need it and
//! neither should carry the other's weight: the command-line tools, which write
//! its JSON to a file, and the WebAssembly surface, which hands the same JSON
//! to a browser. Folding it into the fast decoder would put a reporting layer
//! on the critical path of every consumer that only wants pixels.

mod probe;

pub use probe::{BlockProbe, ProbeReport, SuperblockProbe, probe_frame, probe_stream};

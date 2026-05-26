//! Shared helpers for the tool tests.
//!
//! Each integration test file is its own crate, so a helper used by several of
//! them has to live in a module they all include. This holds the one they all
//! needed: a scratch directory that two tests running at once cannot collide
//! in.

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
};

/// A directory of this call's own, under the system temporary directory.
///
/// Three things keep two runs apart: the process id, a monotonic counter, and
/// the caller's label. The counter is what actually makes the name unique
/// within a run — these tests are parallel by default, and a shared directory
/// had one test removing files another was still reading, which surfaces as a
/// failure on whichever test was unlucky rather than on the collision.
pub fn scratch(label: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let directory = std::env::temp_dir().join(format!(
        "key-frame-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).expect("a scratch directory");
    directory
}

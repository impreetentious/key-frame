//! The two decoders must stay independently written.
//!
//! The verification story is that `kf-ref` and `kf-dec` are two answers to the
//! same specification, so agreement between them is evidence. One shared helper
//! turns that into one decoder and a mirror, and the evidence quietly becomes
//! circular — the suite still passes, and it no longer proves anything.
//!
//! The scan lives here rather than beside the reference decoder because reading
//! the filesystem is forbidden inside the codec perimeter. This crate is a host
//! tool, so it can check the boundary from outside it.

use std::fs;
use std::path::{Path, PathBuf};

/// Crates the reference decoder is allowed to depend on: inert frame storage
/// and inert specification data. Nothing that decides anything.
const PERMITTED: [&str; 2] = ["kf-frame", "kf-spec"];

/// Production codec crates whose implementations must never be shared with the
/// reference decoder, in either direction.
const PRODUCTION: [&str; 8] = [
    "kf_core",
    "kf_range",
    "kf_bitstream",
    "kf_transform",
    "kf_predict",
    "kf_dec",
    "kf_enc",
    "kf_tools",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("invariant: the crate lives two levels below the workspace root")
        .to_path_buf()
}

fn rust_sources(directory: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current).expect("invariant: source directory is readable") {
            let path = entry
                .expect("invariant: directory entry is readable")
                .path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }
    sources.sort();
    sources
}

#[test]
fn trap_reference_dependency_boundary() {
    let root = workspace_root();
    let reference = root.join("crates/kf-ref");
    assert!(
        reference.is_dir(),
        "the reference decoder crate is missing from the workspace"
    );

    // The manifest may name only the inert crates. A dependency added here is
    // the cheapest way to lose the independence claim.
    let manifest = fs::read_to_string(reference.join("Cargo.toml"))
        .expect("invariant: the reference manifest is readable");
    let mut in_dependency_table = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_dependency_table = matches!(
                trimmed,
                "[dependencies]" | "[dev-dependencies]" | "[build-dependencies]"
            );
            continue;
        }
        if !in_dependency_table || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let name = trimmed
            .split(['=', ' '])
            .next()
            .expect("invariant: a dependency line names a crate");
        assert!(
            PERMITTED.contains(&name),
            "the reference decoder declares a dependency on {name}"
        );
    }

    // No source file may import a production codec crate, including through a
    // path that a manifest scan alone would not catch.
    let sources = rust_sources(&reference.join("src"));
    assert!(
        !sources.is_empty(),
        "the reference decoder source scan matched nothing and would pass vacuously"
    );
    for source in &sources {
        let body = fs::read_to_string(source).expect("invariant: source file is readable");
        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
                continue;
            }
            for crate_name in PRODUCTION {
                assert!(
                    !trimmed.contains(&format!("use {crate_name}"))
                        && !trimmed.contains(&format!("extern crate {crate_name}")),
                    "{} imports {crate_name}",
                    source.display()
                );
            }
        }
    }

    // The fast decoder must not reach back the other way outside its tests: a
    // production decode path that calls the reference implementation would make
    // the two agree by construction.
    let fast = rust_sources(&root.join("crates/kf-dec/src"));
    assert!(
        !fast.is_empty(),
        "the fast decoder source scan matched nothing and would pass vacuously"
    );
    for source in &fast {
        let body = fs::read_to_string(source).expect("invariant: source file is readable");
        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
                continue;
            }
            assert!(
                !trimmed.contains("use kf_ref"),
                "{} imports the reference decoder outside its tests",
                source.display()
            );
        }
    }
}

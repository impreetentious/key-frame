//! The embedded asset set, held to the manifest that declares it.

use kf_spec::V1_ASSETS;

/// The manifest is the specification's own statement of its closed set. It is
/// deliberately not one of the embedded assets — a set cannot contain the list
/// of what it contains — so this test reads it from the tree the same way the
/// crate embeds the rest.
const MANIFEST: &str = include_str!("../../../spec/v1/manifest.toml");

/// The asset names the manifest requires, in declared order.
fn declared_names() -> Vec<&'static str> {
    let body = MANIFEST
        .split_once("required = [")
        .expect("manifest.toml declares a required asset list")
        .1
        .split_once(']')
        .expect("the required list is closed")
        .0;
    body.split(',')
        .map(|entry| entry.trim().trim_matches('"'))
        .filter(|entry| !entry.is_empty())
        .collect()
}

#[test]
fn frozen_asset_names_are_the_ones_the_manifest_requires() {
    // Order as well as membership. The set is closed and stable, and a
    // consumer that walks it — the document generator does — renders in this
    // order, so a reshuffle is a change to the published specification.
    //
    // Restating the sixteen names here would make this test agree with a third
    // copy of the list rather than with the manifest: the same defect the
    // specification checker carried until it was made to read the manifest too.
    let names: Vec<_> = V1_ASSETS.iter().map(|asset| asset.name).collect();
    assert_eq!(names, declared_names());
    assert!(!names.is_empty(), "an empty set would make this vacuous");
    assert!(V1_ASSETS.iter().all(|asset| !asset.contents.is_empty()));
}

#[test]
fn context_asset_carries_the_closed_count() {
    let contexts = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "contexts.toml")
        .expect("invariant: closed asset list includes contexts");
    assert!(contexts.contents.lines().any(|line| line == "count = 144"));
}

use kf_spec::V1_ASSETS;

#[test]
fn frozen_asset_names_are_complete_and_stable() {
    let names: Vec<_> = V1_ASSETS.iter().map(|asset| asset.name).collect();
    assert_eq!(
        names,
        [
            "constants.toml",
            "fields.toml",
            "contexts.toml",
            "syntax.toml",
            "intra.toml",
            "mc.toml",
            "mc-vectors.toml",
            "deblock.toml",
            "search.toml",
            "scans.toml",
            "transforms.toml",
            "quant.toml",
            "costs.toml",
            "transform-vectors.toml",
            "vectors.json",
            "probe.schema.json",
        ]
    );
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

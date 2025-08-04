use kf_range::{CONTEXT_COUNT, CoverageCounter, RangeError};
use kf_spec::V1_ASSETS;

fn frozen_ids() -> Vec<u16> {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "contexts.toml")
        .expect("invariant: kf-spec exposes contexts.toml");
    let mut ids = Vec::new();
    for line in asset.contents.lines() {
        let Some(raw) = line.strip_prefix("ids = [") else {
            continue;
        };
        let raw = raw
            .strip_suffix(']')
            .expect("invariant: context id array has a closing bracket");
        for value in raw.split(',') {
            ids.push(
                value
                    .trim()
                    .parse::<u16>()
                    .expect("invariant: context id is a decimal u16"),
            );
        }
    }
    ids
}

#[test]
fn coverage_counter_matches_frozen_asset_ids() {
    let ids = frozen_ids();
    assert_eq!(ids.len(), CONTEXT_COUNT);
    assert_eq!(
        ids,
        (0..u16::try_from(CONTEXT_COUNT).unwrap()).collect::<Vec<_>>()
    );
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), CONTEXT_COUNT);

    let mut counter = CoverageCounter::new();
    for id in ids {
        counter.record(id).unwrap();
    }
    assert!(counter.is_complete());
    assert_eq!(counter.recorded(), CoverageCounter::capacity());
    assert!(counter.missing().is_empty());
}

#[test]
fn merge_unions_distinct_ids() {
    let mut left = CoverageCounter::new();
    left.record(0).unwrap();
    left.record(12).unwrap();
    let mut right = CoverageCounter::new();
    right.record(12).unwrap();
    right.record(143).unwrap();
    left.merge(&right);
    assert_eq!(left.recorded(), 3);
    assert!(left.contains(0));
    assert!(left.contains(12));
    assert!(left.contains(143));
    assert!(!left.contains(1));
}

#[test]
fn trap_coverage_rejects_unknown_id() {
    let mut counter = CoverageCounter::new();
    assert_eq!(
        counter.record(144),
        Err(RangeError::InvalidContext { id: 144 })
    );
    assert!(!counter.contains(144));
}

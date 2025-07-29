use kf_range::{Probability, RangeEncoder};

#[test]
fn adaptive_skew_compresses_better_than_fixed_half() {
    let mut adaptive = RangeEncoder::new();
    let mut probability = Probability::new(2048).unwrap();
    let mut bypass = RangeEncoder::new();
    for _ in 0..2048 {
        adaptive.encode_context(false, &mut probability).unwrap();
        bypass.encode_bypass(false).unwrap();
    }
    let adaptive = adaptive.finish();
    let bypass = bypass.finish();
    assert!(adaptive.bytes.len() * 4 < bypass.bytes.len());
    assert!(adaptive.stats.modeled_entropy_q16 < bypass.stats.modeled_entropy_q16);
}

#[test]
fn finalization_tail_is_not_assigned_to_a_bin() {
    let mut encoder = RangeEncoder::new();
    encoder.encode_bypass(true).unwrap();
    let result = encoder.finish();
    assert!(
        result
            .stats
            .emission_events
            .iter()
            .any(|event| event.finalization)
    );
}

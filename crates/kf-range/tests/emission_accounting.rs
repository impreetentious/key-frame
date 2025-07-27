use kf_range::{Probability, RangeEncoder};

#[test]
fn emission_buckets_conserve_the_payload() {
    let mut encoder = RangeEncoder::new();
    let mut probability = Probability::new(3072).unwrap();
    for symbol in [true, true, true, false, true, true, false, true]
        .into_iter()
        .cycle()
        .take(32)
    {
        encoder.encode_context(symbol, &mut probability).unwrap();
    }
    let result = encoder.finish();
    let bucket_sum: u64 = result
        .stats
        .emission_events
        .iter()
        .map(|event| u64::from(event.bytes))
        .sum();
    assert_eq!(bucket_sum, result.stats.emitted_bytes);
    assert_eq!(bucket_sum, u64::try_from(result.bytes.len()).unwrap());
    assert!(
        result
            .stats
            .emission_events
            .iter()
            .any(|event| event.finalization)
    );
}

use kf_dec::FastDecoder;
use kf_ref::ReferenceDecoder;

const DAMAGED: &[u8] = include_bytes!("../../../conformance/crashes/false_sync_prefix.kfv");

#[test]
fn false_sync_prefix_matches_both_decoders() {
    let fast = FastDecoder::new().decode_stream(DAMAGED).unwrap();
    let reference = ReferenceDecoder::new().decode_stream(DAMAGED).unwrap();
    assert_eq!(fast, reference);
    assert_eq!(fast.len(), 1);
}

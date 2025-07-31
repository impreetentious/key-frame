use kf_dec::FastDecoder;
use kf_ref::ReferenceDecoder;

const ORACLE_STREAM: [u8; 54] = [
    0x4b, 0x46, 0x56, 0x31, 0x01, 0x00, 0x40, 0x00, 0x40, 0x00, 0x01, 0x08, 0x18, 0x00, 0x01, 0x00,
    0x78, 0x00, 0x10, 0x00, 0x0e, 0x72, 0xb9, 0x5d, 0x4b, 0x46, 0x50, 0x31, 0x06, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x07, 0x20, 0x00, 0x00, 0x23, 0x0e, 0x00, 0x74, 0x8a, 0x7c, 0x2a, 0x57,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

#[test]
fn fast_decoder_matches_independent_reference_on_oracle_stream() {
    let fast = FastDecoder::new()
        .decode_intra_stream(&ORACLE_STREAM)
        .unwrap()
        .remove(0);
    let reference = ReferenceDecoder::new()
        .decode_intra_stream(&ORACLE_STREAM)
        .unwrap();
    assert_eq!(fast, reference);
    assert!(fast.y.data().iter().all(|&sample| sample == 128));
}

#[test]
fn failed_frame_invalidates_fast_decoder_references() {
    let mut decoder = FastDecoder::new();
    decoder.decode_intra_stream(&ORACLE_STREAM).unwrap();
    assert!(decoder.has_references());
    let mut corrupt = ORACLE_STREAM;
    corrupt[53] ^= 1;
    assert!(decoder.decode_intra_stream(&corrupt).is_err());
    assert!(!decoder.has_references());
}

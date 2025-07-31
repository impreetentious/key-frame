use kf_ref::ReferenceDecoder;

const ORACLE_STREAM: [u8; 54] = [
    0x4b, 0x46, 0x56, 0x31, 0x01, 0x00, 0x40, 0x00, 0x40, 0x00, 0x01, 0x08, 0x18, 0x00, 0x01, 0x00,
    0x78, 0x00, 0x10, 0x00, 0x0e, 0x72, 0xb9, 0x5d, 0x4b, 0x46, 0x50, 0x31, 0x06, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x07, 0x20, 0x00, 0x00, 0x23, 0x0e, 0x00, 0x74, 0x8a, 0x7c, 0x2a, 0x57,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

#[test]
fn independent_intra_stream_decodes_to_neutral_frame() {
    let mut decoder = ReferenceDecoder::new();
    let frame = decoder.decode_intra_stream(&ORACLE_STREAM).unwrap();
    assert!(frame.y.data().iter().all(|&sample| sample == 128));
    assert!(frame.cb.data().iter().all(|&sample| sample == 128));
    assert!(frame.cr.data().iter().all(|&sample| sample == 128));
    assert!(decoder.has_references());
}

#[test]
fn corrupt_stream_does_not_commit_references() {
    let mut corrupt = ORACLE_STREAM;
    corrupt[53] ^= 1;
    let mut decoder = ReferenceDecoder::new();
    assert!(decoder.decode_intra_stream(&corrupt).is_err());
    assert!(!decoder.has_references());
}

//! The independent decoder's random access, checked against its own linear
//! decode and against its own state hygiene.

use kf_ref::ReferenceDecoder;

/// The literal oracle-authored stream: one keyframe, nothing else.
const ORACLE_STREAM: [u8; 54] = [
    0x4b, 0x46, 0x56, 0x31, 0x01, 0x00, 0x40, 0x00, 0x40, 0x00, 0x01, 0x08, 0x18, 0x00, 0x01, 0x00,
    0x78, 0x00, 0x10, 0x00, 0x0e, 0x72, 0xb9, 0x5d, 0x4b, 0x46, 0x50, 0x31, 0x06, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x07, 0x20, 0x00, 0x00, 0x23, 0x0e, 0x00, 0x74, 0x8a, 0x7c, 0x2a, 0x57,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

#[test]
fn seeking_matches_this_decoders_own_linear_decode() {
    let linear = ReferenceDecoder::new()
        .decode_stream(&ORACLE_STREAM)
        .unwrap();
    let outcome = ReferenceDecoder::new()
        .seek_frame(&ORACLE_STREAM, 0)
        .unwrap();
    assert_eq!(outcome.frame, linear[0]);
    assert_eq!(outcome.keyframe_index, 0);
    assert_eq!(outcome.frames_decoded, 1);
}

#[test]
fn a_target_past_the_end_is_refused() {
    let error = ReferenceDecoder::new()
        .seek_frame(&ORACLE_STREAM, 4)
        .unwrap_err();
    assert!(error.to_string().contains("seek.target"), "{error}");
}

#[test]
fn a_failed_seek_leaves_no_state_behind() {
    let mut decoder = ReferenceDecoder::new();
    decoder.decode_stream(&ORACLE_STREAM).unwrap();
    assert!(decoder.has_references());
    assert!(decoder.seek_frame(&ORACLE_STREAM, 7).is_err());
    assert!(!decoder.has_references());
    assert!(decoder.committed_p1().is_none());
}

#[test]
fn a_damaged_payload_is_refused_before_any_frame_is_produced() {
    let mut corrupt = ORACLE_STREAM;
    corrupt[53] ^= 1;
    let mut decoder = ReferenceDecoder::new();
    assert!(decoder.seek_frame(&corrupt, 0).is_err());
    assert!(!decoder.has_references());
}

#[test]
fn a_successful_seek_commits_the_same_state_as_a_linear_decode() {
    let mut linear = ReferenceDecoder::new();
    linear.decode_stream(&ORACLE_STREAM).unwrap();
    let mut sought = ReferenceDecoder::new();
    sought.seek_frame(&ORACLE_STREAM, 0).unwrap();
    assert_eq!(sought.committed_p1(), linear.committed_p1());
    assert_eq!(sought.has_references(), linear.has_references());
}

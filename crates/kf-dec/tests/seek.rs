//! Seeking is bounded by keyframes, and asking for a frame that is not there
//! is an error rather than a nearby answer.

use kf_dec::{DecodeError, FastDecoder, StreamIndex};
use kf_ref::ReferenceDecoder;

/// The literal oracle-authored stream: one keyframe, nothing else.
const ORACLE_STREAM: [u8; 54] = [
    0x4b, 0x46, 0x56, 0x31, 0x01, 0x00, 0x40, 0x00, 0x40, 0x00, 0x01, 0x08, 0x18, 0x00, 0x01, 0x00,
    0x78, 0x00, 0x10, 0x00, 0x0e, 0x72, 0xb9, 0x5d, 0x4b, 0x46, 0x50, 0x31, 0x06, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x07, 0x20, 0x00, 0x00, 0x23, 0x0e, 0x00, 0x74, 0x8a, 0x7c, 0x2a, 0x57,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

#[test]
fn the_index_reads_a_stream_without_decoding_it() {
    let index = StreamIndex::scan(&ORACLE_STREAM).unwrap();
    assert_eq!(index.frame_count(), 1);
    assert_eq!(index.keyframes(), vec![0]);
    assert_eq!(index.entry_point(0).unwrap(), 0);
}

#[test]
fn seeking_to_the_only_frame_decodes_exactly_that_frame() {
    let linear = FastDecoder::new().decode_stream(&ORACLE_STREAM).unwrap();
    let outcome = FastDecoder::new().seek_frame(&ORACLE_STREAM, 0).unwrap();
    assert_eq!(outcome.frame, linear[0]);
    assert_eq!(outcome.keyframe_index, 0);
    assert_eq!(outcome.frames_decoded, 1);

    let independent = ReferenceDecoder::new()
        .seek_frame(&ORACLE_STREAM, 0)
        .unwrap();
    assert_eq!(independent.frame, outcome.frame);
    assert_eq!(independent.keyframe_index, 0);
    assert_eq!(independent.frames_decoded, 1);
}

#[test]
fn seeking_past_the_last_frame_is_an_error_in_both_decoders() {
    let error = FastDecoder::new()
        .seek_frame(&ORACLE_STREAM, 1)
        .unwrap_err();
    assert!(
        matches!(
            error,
            DecodeError::InvalidFrame {
                frame_index: 1,
                element: "seek.target"
            }
        ),
        "unexpected error: {error}"
    );
    assert!(
        ReferenceDecoder::new()
            .seek_frame(&ORACLE_STREAM, 1)
            .is_err()
    );
    assert!(
        StreamIndex::scan(&ORACLE_STREAM)
            .unwrap()
            .entry_point(1)
            .is_err()
    );
}

#[test]
fn a_stream_a_linear_decode_rejects_is_not_seekable() {
    // Flipping a payload byte breaks the payload checksum. Seeking scans the
    // whole packet sequence first precisely so that it fails here rather than
    // succeeding on a stream nothing else will accept.
    let mut corrupt = ORACLE_STREAM;
    corrupt[53] ^= 1;
    assert!(FastDecoder::new().decode_stream(&corrupt).is_err());
    assert!(StreamIndex::scan(&corrupt).is_err());
    assert!(FastDecoder::new().seek_frame(&corrupt, 0).is_err());
    assert!(ReferenceDecoder::new().seek_frame(&corrupt, 0).is_err());
}

#[test]
fn a_failed_seek_leaves_no_reference_state_behind() {
    let mut decoder = FastDecoder::new();
    decoder.decode_stream(&ORACLE_STREAM).unwrap();
    assert!(decoder.has_references());
    assert!(decoder.seek_frame(&ORACLE_STREAM, 9).is_err());
    assert!(
        !decoder.has_references(),
        "a failed seek must not leave the previous stream's references installed"
    );
    assert!(decoder.committed_p1().is_none());
}

#[test]
fn a_successful_seek_leaves_the_decoder_where_a_linear_decode_would() {
    let mut linear = FastDecoder::new();
    linear.decode_stream(&ORACLE_STREAM).unwrap();
    let mut sought = FastDecoder::new();
    sought.seek_frame(&ORACLE_STREAM, 0).unwrap();
    assert_eq!(sought.committed_p1(), linear.committed_p1());
    assert_eq!(sought.has_references(), linear.has_references());
}

#[test]
fn decoding_from_a_frame_yields_that_frame_onward() {
    let linear = FastDecoder::new().decode_stream(&ORACLE_STREAM).unwrap();
    let tail = FastDecoder::new().decode_from(&ORACLE_STREAM, 0).unwrap();
    assert_eq!(tail, linear);
    assert!(FastDecoder::new().decode_from(&ORACLE_STREAM, 1).is_err());
}

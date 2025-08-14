//! Instrumenting a decode must not change it, and the two decoders must see
//! the same syntax in the same stream.

use kf_bitstream::SyntaxElement;
use kf_dec::FastDecoder;
use kf_ref::ReferenceDecoder;

/// The literal oracle-authored keyframe: one 64×64 superblock, DC intra, no
/// coefficients anywhere. It codes exactly three things and nothing else,
/// which makes it a usable floor for the counter.
const ORACLE_STREAM: [u8; 54] = [
    0x4b, 0x46, 0x56, 0x31, 0x01, 0x00, 0x40, 0x00, 0x40, 0x00, 0x01, 0x08, 0x18, 0x00, 0x01, 0x00,
    0x78, 0x00, 0x10, 0x00, 0x0e, 0x72, 0xb9, 0x5d, 0x4b, 0x46, 0x50, 0x31, 0x06, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x07, 0x20, 0x00, 0x00, 0x23, 0x0e, 0x00, 0x74, 0x8a, 0x7c, 0x2a, 0x57,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

#[test]
fn instrumentation_does_not_change_the_decoded_frames() {
    let plain = FastDecoder::new().decode_stream(&ORACLE_STREAM).unwrap();
    let (instrumented, _) = FastDecoder::new()
        .decode_stream_coverage(&ORACLE_STREAM)
        .unwrap();
    assert_eq!(plain, instrumented);
}

#[test]
fn both_decoders_observe_the_same_contexts_and_elements() {
    let (fast_frames, fast) = FastDecoder::new()
        .decode_stream_coverage(&ORACLE_STREAM)
        .unwrap();
    let (reference_frames, reference) = ReferenceDecoder::new()
        .decode_stream_coverage(&ORACLE_STREAM)
        .unwrap();
    assert_eq!(fast_frames, reference_frames);

    let fast_ids: Vec<u16> = (0..144).filter(|id| fast.contexts.contains(*id)).collect();
    assert_eq!(fast_ids, reference.context_ids());

    let fast_elements: Vec<&str> = SyntaxElement::ALL
        .into_iter()
        .filter(|element| fast.elements.contains(*element))
        .map(SyntaxElement::name)
        .collect();
    assert_eq!(fast_elements, reference.element_names());
}

#[test]
fn an_all_zero_keyframe_codes_only_structure_mode_and_presence() {
    let (_, coverage) = FastDecoder::new()
        .decode_stream_coverage(&ORACLE_STREAM)
        .unwrap();
    assert_eq!(
        SyntaxElement::ALL
            .into_iter()
            .filter(|element| coverage.elements.contains(*element))
            .map(SyntaxElement::name)
            .collect::<Vec<_>>(),
        vec!["partition_tree", "intra_mode", "has_coeff"]
    );
    // Nothing is significant, so no position, magnitude, or sign is ever read.
    for absent in [
        SyntaxElement::LastX,
        SyntaxElement::Sig,
        SyntaxElement::Gt1,
        SyntaxElement::NonzeroSign,
    ] {
        assert!(!coverage.elements.contains(absent));
    }
    assert!(!coverage.contexts.is_complete());
}

use kf_bitstream::SequenceHeader;
use kf_dec::FastDecoder;
use kf_enc::IntraEncoder;
use kf_frame::Frame;
use kf_ref::ReferenceDecoder;
use kf_tools::probe_stream;

fn sequence(width: u16, height: u16) -> SequenceHeader {
    SequenceHeader::new(width, height, 24, 1, 120, 16).unwrap()
}

fn accounting_sum(encoded: &kf_enc::EncodedStream) -> u64 {
    let frame = &encoded.frames[0];
    frame.frame_flush_bytes
        + frame
            .superblocks
            .iter()
            .map(|superblock| {
                superblock.structure_emitted_payload_bytes
                    + superblock
                        .blocks
                        .iter()
                        .map(|block| block.emitted_payload_bytes)
                        .sum::<u64>()
            })
            .sum::<u64>()
}

fn assert_probe_accounting(encoded: &kf_enc::EncodedStream) {
    let probe = probe_stream(&encoded.bytes).unwrap();
    let frame = &encoded.frames[0];
    assert!(probe.canonical_payload_match);
    assert_eq!(probe.canonical_replay_payload_len, frame.payload_len);
    assert_eq!(probe.frame_flush_bytes, frame.frame_flush_bytes);
    assert_eq!(probe.superblocks.len(), frame.superblocks.len());
    for (observed, expected) in probe.superblocks.iter().zip(&frame.superblocks) {
        assert_eq!(
            observed.structure_modeled_entropy_q16,
            expected.structure_modeled_entropy_q16
        );
        assert_eq!(
            observed.structure_emitted_payload_bytes,
            expected.structure_emitted_payload_bytes
        );
        assert_eq!(observed.blocks.len(), expected.blocks.len());
        for (observed_block, expected_block) in observed.blocks.iter().zip(&expected.blocks) {
            assert_eq!(
                observed_block.modeled_entropy_q16,
                expected_block.modeled_entropy_q16
            );
            assert_eq!(
                observed_block.emitted_payload_bytes,
                expected_block.emitted_payload_bytes
            );
        }
    }
}

#[test]
fn neutral_frame_is_canonical_and_conserves_emission_buckets() {
    let source = Frame::filled_420(64, 64, 128).unwrap();
    let encoded = IntraEncoder::new(sequence(64, 64), 28)
        .unwrap()
        .encode(std::slice::from_ref(&source))
        .unwrap();
    let decoded = FastDecoder::new()
        .decode_intra_stream(&encoded.bytes)
        .unwrap();
    assert_eq!(decoded, [source]);
    let reference = ReferenceDecoder::new()
        .decode_intra_stream(&encoded.bytes)
        .unwrap();
    assert_eq!(decoded[0], reference);
    assert_eq!(
        accounting_sum(&encoded),
        u64::try_from(encoded.frames[0].payload_len).unwrap()
    );
    assert_probe_accounting(&encoded);
}

#[test]
fn gradient_round_trip_is_deterministic_at_multiple_qps() {
    let mut source = Frame::filled_420(66, 64, 0).unwrap();
    for y in 0..64 {
        for x in 0..66 {
            source
                .y
                .set(x, y, u8::try_from((x * 3 + y * 2) % 256).unwrap())
                .unwrap();
        }
    }
    for qp in [0, 24, 48, 63] {
        let encoder = IntraEncoder::new(sequence(66, 64), qp).unwrap();
        let first = encoder.encode(std::slice::from_ref(&source)).unwrap();
        let second = encoder.encode(std::slice::from_ref(&source)).unwrap();
        assert_eq!(first.bytes, second.bytes);
        let decoded = FastDecoder::new()
            .decode_intra_stream(&first.bytes)
            .unwrap();
        let reference = ReferenceDecoder::new()
            .decode_intra_stream(&first.bytes)
            .unwrap();
        assert_eq!(decoded[0], reference);
        assert_eq!((decoded[0].width(), decoded[0].height()), (66, 64));
        assert_eq!(
            accounting_sum(&first),
            u64::try_from(first.frames[0].payload_len).unwrap()
        );
        assert_probe_accounting(&first);
        if qp == 0 {
            assert!(
                first.frames[0]
                    .superblocks
                    .iter()
                    .any(|superblock| superblock.blocks.len() > 1),
                "low-QP gradient should exercise partition RDO"
            );
        }
    }
}

#[test]
fn gray_and_noise_match_both_decoders_at_five_qps() {
    let gray = Frame::filled_420(64, 64, 96).unwrap();
    let mut noise = Frame::filled_420(64, 64, 0).unwrap();
    let mut state = 0x6d2b_79f5_u32;
    for sample in noise
        .y
        .data_mut()
        .iter_mut()
        .chain(noise.cb.data_mut())
        .chain(noise.cr.data_mut())
    {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *sample = state.to_le_bytes()[0];
    }
    for source in [gray, noise] {
        for qp in [0, 16, 32, 48, 63] {
            let encoded = IntraEncoder::new(sequence(64, 64), qp)
                .unwrap()
                .encode(std::slice::from_ref(&source))
                .unwrap();
            let fast = FastDecoder::new()
                .decode_intra_stream(&encoded.bytes)
                .unwrap()
                .remove(0);
            let reference = ReferenceDecoder::new()
                .decode_intra_stream(&encoded.bytes)
                .unwrap();
            assert_eq!(fast, reference);
            assert_eq!(
                accounting_sum(&encoded),
                u64::try_from(encoded.frames[0].payload_len).unwrap()
            );
            assert_probe_accounting(&encoded);
        }
    }
}

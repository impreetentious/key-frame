use kf_bitstream::SequenceHeader;
use kf_dec::FastDecoder;
use kf_enc::Encoder;
use kf_frame::Frame;
use kf_range::ContextBank;
use kf_ref::ReferenceDecoder;

fn sequence(width: u16, height: u16) -> SequenceHeader {
    SequenceHeader::new(width, height, 24, 1, 120, 16).unwrap()
}

fn gray(width: u32, height: u32, value: u8) -> Frame {
    Frame::filled_420(width, height, value).unwrap()
}

fn noise(width: u32, height: u32) -> Frame {
    let mut frame = Frame::filled_420(width, height, 0).unwrap();
    let mut state = 0xa3c5_91e7_u32;
    for sample in frame
        .y
        .data_mut()
        .iter_mut()
        .chain(frame.cb.data_mut())
        .chain(frame.cr.data_mut())
    {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *sample = state.to_le_bytes()[0];
    }
    frame
}

#[test]
fn trap_context_lockstep() {
    let sources = vec![gray(128, 64, 96), gray(128, 64, 96), noise(128, 64)];
    let encoded = Encoder::new(sequence(128, 64), 32)
        .unwrap()
        .encode(&sources)
        .unwrap();
    assert_eq!(encoded.checkpoints.len(), 6);
    assert_eq!(encoded.frames.len(), 3);
    assert!(encoded.frames[0].key);
    assert!(!encoded.frames[1].key);

    let (fast_frames, fast_checkpoints) = FastDecoder::new()
        .decode_stream_traced(&encoded.bytes)
        .unwrap();
    let (reference_frames, reference_checkpoints) = ReferenceDecoder::new()
        .decode_stream_traced(&encoded.bytes)
        .unwrap();

    assert_eq!(fast_frames, encoded.reconstructed_frames);
    assert_eq!(fast_frames, reference_frames);
    assert_eq!(encoded.checkpoints, fast_checkpoints);
    assert_eq!(encoded.checkpoints, reference_checkpoints);
    assert_ne!(encoded.checkpoints[0], ContextBank::initial().p1_values());
    assert_eq!(encoded.checkpoints[1], fast_checkpoints[1]);
    assert_eq!(
        encoded.checkpoints[5],
        FastDecoder::new()
            .decode_stream_traced(&encoded.bytes)
            .unwrap()
            .1[5]
    );
}

#[test]
fn committed_bank_matches_final_superblock_checkpoint() {
    let encoded = Encoder::new(sequence(64, 64), 28)
        .unwrap()
        .encode(&[gray(64, 64, 48), noise(64, 64)])
        .unwrap();
    let mut fast = FastDecoder::new();
    let mut reference = ReferenceDecoder::new();
    let _ = fast.decode_stream(&encoded.bytes).unwrap();
    let _ = reference.decode_stream(&encoded.bytes).unwrap();
    assert_eq!(fast.committed_p1(), encoded.checkpoints.last().copied());
    assert_eq!(
        reference.committed_p1(),
        encoded.checkpoints.last().copied()
    );
}

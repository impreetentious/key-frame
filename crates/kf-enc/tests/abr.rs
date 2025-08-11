use kf_bitstream::SequenceHeader;
use kf_enc::{Encoder, RateControl};
use kf_frame::Frame;

fn sequence() -> SequenceHeader {
    SequenceHeader::new(64, 64, 24, 1, 120, 16).unwrap()
}

fn noise_frame(seed: u32) -> Frame {
    let mut frame = Frame::filled_420(64, 64, 0).unwrap();
    let mut state = seed;
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

fn sources() -> Vec<Frame> {
    vec![
        Frame::filled_420(64, 64, 96).unwrap(),
        Frame::filled_420(64, 64, 96).unwrap(),
        noise_frame(0x8f31_a25c),
        noise_frame(0x4a19_c7e3),
    ]
}

#[test]
fn abr_encode_is_deterministic() {
    let encoder = Encoder::with_bitrate(sequence(), 80_000).unwrap();
    assert_eq!(
        encoder.rate(),
        RateControl::Abr {
            bitrate_bps: 80_000
        }
    );
    let first = encoder.encode(&sources()).unwrap();
    let second = encoder.encode(&sources()).unwrap();
    assert_eq!(first.bytes, second.bytes);
    assert_eq!(first.frames.len(), 4);
    assert!(first.frames.iter().all(|frame| frame.qp <= 63));
}

#[test]
fn constant_qp_ignores_bitrate_path() {
    let encoder = Encoder::new(sequence(), 32).unwrap();
    assert_eq!(encoder.rate(), RateControl::ConstantQp(32));
    let encoded = encoder.encode(&sources()).unwrap();
    assert!(encoded.frames.iter().all(|frame| frame.qp == 32));
}

#[test]
fn abr_qp_adapts_across_frames() {
    let tight = Encoder::with_bitrate(sequence(), 12_000)
        .unwrap()
        .encode(&sources())
        .unwrap();
    let open = Encoder::with_bitrate(sequence(), 8_000_000)
        .unwrap()
        .encode(&sources())
        .unwrap();
    assert!(tight.frames[0].qp > open.frames[0].qp);
    assert!(tight.bytes.len() < open.bytes.len() || tight.frames[0].qp >= 40);
}

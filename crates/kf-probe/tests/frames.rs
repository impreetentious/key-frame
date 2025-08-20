//! Probing a frame part-way into a stream must re-enter where a decoder would.

use kf_bitstream::SequenceHeader;
use kf_enc::Encoder;
use kf_frame::Frame;
use kf_probe::{probe_frame, probe_stream};

/// A short clip with several keyframes, so the entry point actually moves.
fn multi_keyframe_stream(key_interval: u16, frames: u32) -> Vec<u8> {
    let sequence =
        SequenceHeader::new(64, 64, 24, 1, key_interval, 2).expect("the header is in range");
    let sources = (0..frames)
        .map(|index| {
            let mut frame = Frame::filled_420(64, 64, 0).expect("the frame is in range");
            for y in 0..64 {
                for x in 0..64 {
                    let value = u8::try_from((x * 3 + y * 5 + index * 7) % 256)
                        .expect("the sample is a byte");
                    frame.y.set(x, y, value).expect("the sample is in range");
                }
            }
            frame
        })
        .collect::<Vec<_>>();
    Encoder::new(sequence, 32)
        .expect("the encoder accepts the header")
        .encode(&sources)
        .expect("the clip encodes")
        .bytes
}

#[test]
fn the_default_probe_is_the_first_frame() {
    let stream = multi_keyframe_stream(4, 8);
    assert_eq!(
        probe_stream(&stream).unwrap().to_json(),
        probe_frame(&stream, 0).unwrap().to_json()
    );
}

#[test]
fn every_frame_of_a_multi_keyframe_clip_probes() {
    let stream = multi_keyframe_stream(4, 9);
    for index in 0..9 {
        let report = probe_frame(&stream, index).unwrap();
        assert_eq!(report.frame_index, index);
        assert_eq!(report.key, index % 4 == 0);
        assert!(
            !report.superblocks.is_empty(),
            "frame {index} reported no superblocks"
        );
        assert!(
            report.canonical_payload_match,
            "frame {index}: the encoder's own payload must replay canonically"
        );
        assert_eq!(
            report.input_payload_len, report.canonical_replay_payload_len,
            "frame {index}: a matching replay must account for every input byte"
        );
    }
}

#[test]
fn a_frame_past_the_end_is_refused() {
    let stream = multi_keyframe_stream(4, 4);
    assert!(probe_frame(&stream, 4).is_err());
    assert!(probe_frame(&stream, u32::MAX).is_err());
}

#[test]
fn the_report_names_the_prediction_kinds_a_p_frame_uses() {
    // Frame one follows a keyframe, so it can skip, predict, or fall back to
    // intra. Whatever it chose, every block must name a kind the schema knows.
    let stream = multi_keyframe_stream(8, 4);
    let report = probe_frame(&stream, 1).unwrap();
    assert!(!report.key);
    for superblock in &report.superblocks {
        for block in &superblock.blocks {
            let known = matches!(
                block.mode,
                "dc" | "planar"
                    | "horizontal"
                    | "vertical"
                    | "d45"
                    | "d135"
                    | "d117"
                    | "d153"
                    | "skip"
                    | "inter"
            );
            assert!(known, "unknown prediction kind {}", block.mode);
            if block.mode == "skip" {
                assert!(block.reference.is_some());
                assert!(block.motion_vector_q4.is_none());
            }
            if block.mode == "inter" {
                assert!(block.reference.is_some());
                assert!(block.motion_vector_q4.is_some());
            }
        }
    }
}

#[test]
fn probing_a_later_frame_does_not_depend_on_the_probe_before_it() {
    // Each call re-enters from the governing keyframe on its own. Probing in
    // reverse order must give the same answers as probing forwards, or the
    // function is carrying state it should not have.
    let stream = multi_keyframe_stream(4, 6);
    let forwards = (0..6)
        .map(|index| probe_frame(&stream, index).unwrap().to_json())
        .collect::<Vec<_>>();
    let backwards = (0..6)
        .rev()
        .map(|index| probe_frame(&stream, index).unwrap().to_json())
        .collect::<Vec<_>>();
    assert_eq!(
        forwards,
        backwards.into_iter().rev().collect::<Vec<_>>(),
        "probe order changed the report"
    );
}

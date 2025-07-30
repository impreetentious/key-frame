use kf_bitstream::{
    FrameType, IntraMode, MotionVector, Prediction, ReferenceFrame, SyntaxReader, SyntaxWriter,
};
use kf_range::ContextBank;

fn round_trip(frame_type: FrameType, predictions: &[Prediction]) {
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    for &prediction in predictions {
        writer.write_prediction(frame_type, prediction).unwrap();
    }
    let (encoded, encoder_contexts) = writer.finish();
    let mut reader = SyntaxReader::new(&encoded.bytes, ContextBank::initial()).unwrap();
    for &prediction in predictions {
        assert_eq!(reader.read_prediction(frame_type).unwrap(), prediction);
    }
    assert_eq!(reader.contexts(), &encoder_contexts);
}

#[test]
fn every_intra_mode_round_trips() {
    let modes = [
        IntraMode::Dc,
        IntraMode::Planar,
        IntraMode::Horizontal,
        IntraMode::Vertical,
        IntraMode::D45,
        IntraMode::D135,
        IntraMode::D117,
        IntraMode::D153,
    ];
    let predictions: Vec<_> = modes.into_iter().map(Prediction::Intra).collect();
    round_trip(FrameType::Key, &predictions);
}

#[test]
fn trap_pframe_intra_path_and_inter_branches() {
    round_trip(
        FrameType::P,
        &[
            Prediction::Skip {
                reference: ReferenceFrame::Last,
            },
            Prediction::Skip {
                reference: ReferenceFrame::Golden,
            },
            Prediction::Intra(IntraMode::D117),
            Prediction::Inter {
                reference: ReferenceFrame::Last,
                mvd: MotionVector { x_q4: 0, y_q4: 0 },
            },
            Prediction::Inter {
                reference: ReferenceFrame::Golden,
                mvd: MotionVector {
                    x_q4: -512,
                    y_q4: 511,
                },
            },
        ],
    );
}

#[test]
fn keyframe_rejects_inter_prediction() {
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    assert!(
        writer
            .write_prediction(
                FrameType::Key,
                Prediction::Skip {
                    reference: ReferenceFrame::Last
                }
            )
            .is_err()
    );
}

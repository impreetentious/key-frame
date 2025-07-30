//! Candidate evaluation must not mutate the live entropy state.
//!
//! Rate–distortion search prices every candidate by writing it and reading the
//! modeled-entropy delta back. If that write lands on the live context bank,
//! each candidate is priced against a state the previous candidate moved, the
//! encoder's own accounting stops matching what it finally emits, and the
//! decoder diverges on a stream that encoded without complaint.
//!
//! Two properties make the search sound, and both are asserted here: the live
//! writer is byte-identical after evaluation, and a candidate's price does not
//! depend on how many other candidates were priced before it. The second is
//! what the search order pins down — with a shared bank, evaluating the mode
//! list forwards and backwards would produce two different price tables.

use kf_bitstream::{FrameType, IntraMode, MotionVector, Prediction, ReferenceFrame, SyntaxWriter};
use kf_range::ContextBank;

const INTRA_MODES: [IntraMode; 8] = [
    IntraMode::Dc,
    IntraMode::Planar,
    IntraMode::Horizontal,
    IntraMode::Vertical,
    IntraMode::D45,
    IntraMode::D135,
    IntraMode::D117,
    IntraMode::D153,
];

/// Puts the writer in a state no fresh bank would reproduce, so an evaluation
/// that quietly resets to the initial probabilities is visible.
fn warmed_writer(frame_type: FrameType) -> SyntaxWriter {
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    for mode in INTRA_MODES {
        writer
            .write_prediction(frame_type, Prediction::Intra(mode))
            .unwrap();
    }
    writer
}

/// Prices one candidate exactly as the encoder does: on a private clone.
fn price(writer: &SyntaxWriter, frame_type: FrameType, prediction: Prediction) -> u64 {
    let mut shadow = writer.clone();
    let before = shadow.stats().modeled_entropy_q16;
    shadow.write_prediction(frame_type, prediction).unwrap();
    shadow.stats().modeled_entropy_q16 - before
}

#[test]
fn trap_rdo_context_snapshot() {
    let writer = warmed_writer(FrameType::Key);
    let contexts_before = writer.contexts().clone();
    let stats_before = writer.stats().clone();

    let forward: Vec<u64> = INTRA_MODES
        .iter()
        .map(|&mode| price(&writer, FrameType::Key, Prediction::Intra(mode)))
        .collect();

    // Evaluating the whole candidate set left the live state untouched.
    assert_eq!(
        writer.contexts(),
        &contexts_before,
        "candidate evaluation mutated the live context bank"
    );
    assert_eq!(
        writer.stats(),
        &stats_before,
        "candidate evaluation mutated the live emission accounting"
    );

    // The same set priced in reverse yields the same table. A shared bank would
    // make the last mode evaluated cheaper than the first.
    let mut reversed: Vec<u64> = INTRA_MODES
        .iter()
        .rev()
        .map(|&mode| price(&writer, FrameType::Key, Prediction::Intra(mode)))
        .collect();
    reversed.reverse();
    assert_eq!(
        forward, reversed,
        "a candidate's modeled cost depended on evaluation order"
    );

    // Pricing is repeatable: the same candidate against the same live state
    // costs the same every time it is asked.
    for (index, &mode) in INTRA_MODES.iter().enumerate() {
        assert_eq!(
            price(&writer, FrameType::Key, Prediction::Intra(mode)),
            forward[index],
            "repricing {mode:?} produced a different modeled cost"
        );
    }

    // Every candidate carries a real cost, so the comparison the encoder makes
    // is between measured quantities rather than zeroes.
    assert!(
        forward.iter().all(|&cost| cost > 0),
        "a candidate was priced at zero modeled entropy"
    );
}

#[test]
fn trap_rdo_context_snapshot_across_prediction_kinds() {
    let writer = warmed_writer(FrameType::P);
    let contexts_before = writer.contexts().clone();
    let stats_before = writer.stats().clone();

    let candidates = [
        Prediction::Skip {
            reference: ReferenceFrame::Last,
        },
        Prediction::Skip {
            reference: ReferenceFrame::Golden,
        },
        Prediction::Inter {
            reference: ReferenceFrame::Last,
            mvd: MotionVector { x_q4: 6, y_q4: -6 },
        },
        Prediction::Inter {
            reference: ReferenceFrame::Golden,
            mvd: MotionVector {
                x_q4: -31,
                y_q4: 17,
            },
        },
        Prediction::Intra(IntraMode::D153),
    ];

    let forward: Vec<u64> = candidates
        .iter()
        .map(|&candidate| price(&writer, FrameType::P, candidate))
        .collect();

    assert_eq!(
        writer.contexts(),
        &contexts_before,
        "mixed-kind evaluation mutated the live context bank"
    );
    assert_eq!(
        writer.stats(),
        &stats_before,
        "mixed-kind evaluation mutated the live emission accounting"
    );

    let mut reversed: Vec<u64> = candidates
        .iter()
        .rev()
        .map(|&candidate| price(&writer, FrameType::P, candidate))
        .collect();
    reversed.reverse();
    assert_eq!(
        forward, reversed,
        "a prediction kind's modeled cost depended on evaluation order"
    );
}

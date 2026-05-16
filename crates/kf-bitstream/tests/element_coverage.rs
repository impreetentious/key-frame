//! The element counter must record exactly what the payload coded, and the
//! reader must agree with the writer bin for bin.

use kf_bitstream::{
    BlockSize, ElementCoverage, FrameType, IntraMode, MotionVector, PartitionTree, PlaneClass,
    Prediction, ReferenceFrame, SyntaxElement, SyntaxReader, SyntaxWriter, TransformBlockSize,
};
use kf_range::ContextBank;

fn split64_into_32() -> PartitionTree {
    PartitionTree::Split {
        size: BlockSize::N64,
        children: Box::new([
            PartitionTree::Leaf(BlockSize::N32),
            PartitionTree::Leaf(BlockSize::N32),
            PartitionTree::Leaf(BlockSize::N32),
            PartitionTree::Leaf(BlockSize::N32),
        ]),
    }
}

#[test]
fn an_all_zero_block_codes_presence_only() {
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    writer
        .write_coefficients(PlaneClass::Luma, TransformBlockSize::N4, &[0; 16])
        .unwrap();
    let elements = *writer.elements();
    assert!(elements.contains(SyntaxElement::HasCoeff));
    for absent in [
        SyntaxElement::LastX,
        SyntaxElement::LastY,
        SyntaxElement::Sig,
        SyntaxElement::Gt1,
        SyntaxElement::Gt2,
        SyntaxElement::MagnitudeRemainder,
        SyntaxElement::NonzeroSign,
    ] {
        assert!(
            !elements.contains(absent),
            "{} must not be coded for an empty block",
            absent.name()
        );
    }

    let (encoded, _) = writer.finish();
    let mut reader = SyntaxReader::new(&encoded.bytes, ContextBank::initial()).unwrap();
    assert_eq!(
        reader
            .read_coefficients(PlaneClass::Luma, TransformBlockSize::N4)
            .unwrap(),
        vec![0; 16]
    );
    assert_eq!(reader.elements(), &elements);
}

#[test]
fn magnitude_ladder_reaches_the_bypass_elements_in_order() {
    // A level of 1 stops at gt1; 2 reaches gt2; 3 and above spill into the
    // bypass remainder. Each rung must add exactly one element.
    let mut previous = ElementCoverage::new();
    for (level, expected_new) in [
        (1_i32, Some(SyntaxElement::Gt1)),
        (2, Some(SyntaxElement::Gt2)),
        (3, Some(SyntaxElement::MagnitudeRemainder)),
        (400, None),
    ] {
        let mut levels = vec![0_i32; 16];
        levels[0] = level;
        let mut writer = SyntaxWriter::new(ContextBank::initial());
        writer
            .write_coefficients(PlaneClass::Luma, TransformBlockSize::N4, &levels)
            .unwrap();
        let elements = *writer.elements();
        assert!(elements.contains(SyntaxElement::NonzeroSign));
        if let Some(new) = expected_new {
            assert!(
                !previous.contains(new) && elements.contains(new),
                "level {level} should be the first to code {}",
                new.name()
            );
        }
        let (encoded, _) = writer.finish();
        let mut reader = SyntaxReader::new(&encoded.bytes, ContextBank::initial()).unwrap();
        assert_eq!(
            reader
                .read_coefficients(PlaneClass::Luma, TransformBlockSize::N4)
                .unwrap(),
            levels
        );
        assert_eq!(reader.elements(), &elements);
        previous = elements;
    }
}

#[test]
fn sig_is_absent_when_the_only_level_is_the_last_position() {
    let mut levels = vec![0_i32; 16];
    levels[0] = 1;
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    writer
        .write_coefficients(PlaneClass::Luma, TransformBlockSize::N4, &levels)
        .unwrap();
    assert!(!writer.elements().contains(SyntaxElement::Sig));

    let mut trailing = vec![0_i32; 16];
    trailing[0] = 1;
    trailing[15] = 1;
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    writer
        .write_coefficients(PlaneClass::Luma, TransformBlockSize::N4, &trailing)
        .unwrap();
    assert!(writer.elements().contains(SyntaxElement::Sig));
}

#[test]
fn the_minimum_partition_size_codes_no_split_decision() {
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    writer
        .write_partition_decision(BlockSize::N8, false)
        .unwrap();
    assert!(!writer.elements().contains(SyntaxElement::PartitionTree));

    let mut writer = SyntaxWriter::new(ContextBank::initial());
    writer.write_partition(&split64_into_32()).unwrap();
    assert!(writer.elements().contains(SyntaxElement::PartitionTree));
}

#[test]
fn each_prediction_branch_codes_its_own_element_set() {
    let cases: [(FrameType, Prediction, &[SyntaxElement]); 4] = [
        (
            FrameType::Key,
            Prediction::Intra(IntraMode::D45),
            &[SyntaxElement::IntraMode],
        ),
        (
            FrameType::P,
            Prediction::Skip {
                reference: ReferenceFrame::Golden,
            },
            &[SyntaxElement::Skip, SyntaxElement::RefSelect],
        ),
        (
            FrameType::P,
            Prediction::Intra(IntraMode::Planar),
            &[
                SyntaxElement::Skip,
                SyntaxElement::IsInter,
                SyntaxElement::IntraMode,
            ],
        ),
        (
            FrameType::P,
            Prediction::Inter {
                reference: ReferenceFrame::Last,
                mvd: MotionVector { x_q4: 12, y_q4: -3 },
            },
            &[
                SyntaxElement::Skip,
                SyntaxElement::IsInter,
                SyntaxElement::RefSelect,
                SyntaxElement::Mvd,
            ],
        ),
    ];
    for (frame_type, prediction, expected) in cases {
        let mut writer = SyntaxWriter::new(ContextBank::initial());
        writer.write_prediction(frame_type, prediction).unwrap();
        let elements = *writer.elements();
        for element in SyntaxElement::ALL {
            assert_eq!(
                elements.contains(element),
                expected.contains(&element),
                "{prediction:?} on {frame_type:?}: unexpected state for {}",
                element.name()
            );
        }
        let (encoded, _) = writer.finish();
        let mut reader = SyntaxReader::new(&encoded.bytes, ContextBank::initial()).unwrap();
        assert_eq!(reader.read_prediction(frame_type).unwrap(), prediction);
        assert_eq!(reader.elements(), &elements);
    }
}

#[test]
fn one_payload_can_reach_every_element_in_the_closed_set() {
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    writer.write_partition(&split64_into_32()).unwrap();
    writer
        .write_prediction(
            FrameType::P,
            Prediction::Inter {
                reference: ReferenceFrame::Golden,
                mvd: MotionVector {
                    x_q4: -40,
                    y_q4: 40,
                },
            },
        )
        .unwrap();
    writer
        .write_prediction(FrameType::P, Prediction::Intra(IntraMode::Horizontal))
        .unwrap();
    let mut levels = vec![0_i32; 16];
    levels[0] = 9;
    levels[3] = -1;
    levels[15] = 2;
    writer
        .write_coefficients(PlaneClass::Chroma, TransformBlockSize::N4, &levels)
        .unwrap();

    let elements = *writer.elements();
    assert!(
        elements.is_complete(),
        "still missing {:?}",
        elements
            .missing()
            .iter()
            .map(|element| element.name())
            .collect::<Vec<_>>()
    );
    assert_eq!(elements.recorded(), SyntaxElement::ALL.len());
}

use kf_predict::{BlockMotion, MotionField, MotionVector, PredictError, ReferenceSlot};

fn inter(reference: ReferenceSlot, x_q4: i32, y_q4: i32) -> BlockMotion {
    BlockMotion::Inter {
        reference,
        motion_vector: MotionVector { x_q4, y_q4 },
    }
}

#[test]
fn trap_mvp_availability() {
    let mut field = MotionField::new(64, 64).unwrap();
    assert_eq!(
        field.predictor(0, 0, 8, ReferenceSlot::Last).unwrap(),
        MotionVector::default()
    );

    field
        .record(0, 0, 8, inter(ReferenceSlot::Last, 4, 40))
        .unwrap();
    field.record(8, 0, 8, BlockMotion::Intra).unwrap();
    field
        .record(16, 0, 8, inter(ReferenceSlot::Golden, 80, 8))
        .unwrap();
    field
        .record(24, 0, 8, inter(ReferenceSlot::Last, 12, 20))
        .unwrap();
    field
        .record(0, 8, 8, inter(ReferenceSlot::Last, 20, 12))
        .unwrap();
    field
        .record(8, 8, 8, inter(ReferenceSlot::Last, 8, 32))
        .unwrap();

    // For (16,8), left contributes LAST, above is GOLDEN and therefore zero,
    // above-right contributes LAST: component medians are (8,20).
    assert_eq!(
        field.predictor(16, 8, 8, ReferenceSlot::Last).unwrap(),
        MotionVector { x_q4: 8, y_q4: 20 }
    );

    field
        .record(16, 8, 8, inter(ReferenceSlot::Last, -4, -8))
        .unwrap();
    field
        .record(8, 16, 8, inter(ReferenceSlot::Last, -20, 28))
        .unwrap();

    // At (16,16), above-right is not yet decoded, so the matching above-left
    // candidate is used. The component medians mix three distinct vectors.
    assert_eq!(
        field.predictor(16, 16, 8, ReferenceSlot::Last).unwrap(),
        MotionVector { x_q4: -4, y_q4: 28 }
    );

    // A different selected reference makes every otherwise-valid neighbor
    // contribute zero.
    assert_eq!(
        field.predictor(32, 8, 8, ReferenceSlot::Golden).unwrap(),
        MotionVector::default()
    );
}

#[test]
fn motion_field_rejects_bad_geometry_and_overlap() {
    assert_eq!(
        MotionField::new(63, 64),
        Err(PredictError::InvalidMotionField {
            width: 63,
            height: 64
        })
    );
    let mut field = MotionField::new(64, 64).unwrap();
    field.record(0, 0, 16, BlockMotion::Intra).unwrap();
    assert_eq!(
        field.record(8, 8, 8, BlockMotion::Intra),
        Err(PredictError::MotionFieldOverlap { x: 8, y: 8 })
    );
}

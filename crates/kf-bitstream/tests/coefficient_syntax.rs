use kf_bitstream::{PlaneClass, SyntaxReader, SyntaxWriter, TransformBlockSize};
use kf_range::ContextBank;

fn round_trip(plane: PlaneClass, size: TransformBlockSize, levels: Vec<i32>) {
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    writer.write_coefficients(plane, size, &levels).unwrap();
    let (encoded, encoder_contexts) = writer.finish();
    let mut reader = SyntaxReader::new(&encoded.bytes, ContextBank::initial()).unwrap();
    assert_eq!(reader.read_coefficients(plane, size).unwrap(), levels);
    assert_eq!(reader.contexts(), &encoder_contexts);
}

#[test]
fn trap_all_zero_transform_block() {
    for size in [
        TransformBlockSize::N4,
        TransformBlockSize::N8,
        TransformBlockSize::N16,
        TransformBlockSize::N32,
    ] {
        round_trip(PlaneClass::Luma, size, vec![0; size.side() * size.side()]);
    }
}

#[test]
fn signed_levels_last_position_and_caps_round_trip() {
    for (plane, size) in [
        (PlaneClass::Luma, TransformBlockSize::N4),
        (PlaneClass::Chroma, TransformBlockSize::N8),
        (PlaneClass::Luma, TransformBlockSize::N16),
        (PlaneClass::Chroma, TransformBlockSize::N32),
    ] {
        let mut levels = vec![0; size.side() * size.side()];
        levels[0] = 1;
        levels[1] = -2;
        levels[size.side() + 1] = 3;
        levels[size.side() * size.side() - 1] = -32_767;
        round_trip(plane, size, levels);
    }
}

#[test]
fn one_over_coefficient_cap_is_rejected() {
    let mut levels = vec![0; 16];
    levels[0] = 32_768;
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    assert!(
        writer
            .write_coefficients(PlaneClass::Luma, TransformBlockSize::N4, &levels)
            .is_err()
    );
}

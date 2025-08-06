use kf_bitstream::{IntraMode, PlaneClass, SyntaxReader, SyntaxWriter, TransformBlockSize};
use kf_range::{CONTEXT_COUNT, ContextBank, CoverageCounter};

fn round_trip(plane: PlaneClass, size: TransformBlockSize, levels: Vec<i32>) -> CoverageCounter {
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    writer.write_coefficients(plane, size, &levels).unwrap();
    let coverage = writer.coverage().clone();
    let (encoded, encoder_contexts) = writer.finish();
    let mut reader = SyntaxReader::new(&encoded.bytes, ContextBank::initial()).unwrap();
    assert_eq!(reader.read_coefficients(plane, size).unwrap(), levels);
    assert_eq!(reader.contexts(), &encoder_contexts);
    assert_eq!(reader.coverage(), &coverage);
    coverage
}

fn patterned_levels(size: TransformBlockSize) -> Vec<i32> {
    let side = size.side();
    let mut levels = vec![0; side * side];
    levels[0] = 1;
    levels[1] = -2;
    if side > 2 {
        levels[side] = 3;
        levels[side + 1] = -4;
    }
    let last = side * side - 1;
    levels[last] = 5;
    for extra in 1..side.min(11) {
        let index = extra * side;
        if levels[index] == 0 {
            levels[index] = i32::from(u8::try_from(extra).unwrap()) + 1;
        }
    }
    levels
}

#[test]
fn coefficient_size_matrix_covers_presence_last_sig_and_magnitude() {
    let mut coverage = CoverageCounter::new();
    for plane in [PlaneClass::Luma, PlaneClass::Chroma] {
        for size in [
            TransformBlockSize::N4,
            TransformBlockSize::N8,
            TransformBlockSize::N16,
            TransformBlockSize::N32,
        ] {
            coverage.merge(&round_trip(plane, size, vec![0; size.side() * size.side()]));
            let mut dc_gt2 = vec![0; size.side() * size.side()];
            dc_gt2[0] = 3;
            coverage.merge(&round_trip(plane, size, dc_gt2));
            coverage.merge(&round_trip(plane, size, patterned_levels(size)));
        }
    }
    for id in 36..=43 {
        assert!(coverage.contains(id), "has_coeff id {id} missing");
    }
    for id in [44, 45, 48, 49, 50, 52, 53, 54, 55, 56, 57, 58, 59] {
        assert!(coverage.contains(id), "last-position id {id} missing");
    }
    for id in [120, 124, 128, 132, 136, 138, 140, 142] {
        assert!(coverage.contains(id), "magnitude id {id} missing");
    }
    assert!(coverage.recorded() >= 40);
    assert!(coverage.recorded() < u16::try_from(CONTEXT_COUNT).unwrap());
}

#[test]
fn every_intra_mode_codes_the_three_live_tree_bins() {
    let mut coverage = CoverageCounter::new();
    for mode in [
        IntraMode::Dc,
        IntraMode::Planar,
        IntraMode::Horizontal,
        IntraMode::Vertical,
        IntraMode::D45,
        IntraMode::D135,
        IntraMode::D117,
        IntraMode::D153,
    ] {
        let mut writer = SyntaxWriter::new(ContextBank::initial());
        writer
            .write_prediction(
                kf_bitstream::FrameType::Key,
                kf_bitstream::Prediction::Intra(mode),
            )
            .unwrap();
        coverage.merge(writer.coverage());
        let (encoded, contexts) = writer.finish();
        let mut reader = SyntaxReader::new(&encoded.bytes, ContextBank::initial()).unwrap();
        assert_eq!(
            reader
                .read_prediction(kf_bitstream::FrameType::Key)
                .unwrap(),
            kf_bitstream::Prediction::Intra(mode)
        );
        assert_eq!(reader.contexts(), &contexts);
    }
    assert!(coverage.contains(18));
    assert!(coverage.contains(21));
    assert!(coverage.contains(22));
}

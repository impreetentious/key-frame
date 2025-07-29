use kf_transform::{TransformSize, inverse_transform};

#[test]
fn dc_only_vectors_are_spatially_constant() {
    for (size, expected) in [
        (TransformSize::N4, 16),
        (TransformSize::N8, 32),
        (TransformSize::N16, 64),
        (TransformSize::N32, 128),
    ] {
        let mut coefficients = vec![0; size.side() * size.side()];
        coefficients[0] = 1024;
        let output = inverse_transform(&coefficients, size).unwrap();
        assert!(output.iter().all(|&sample| sample == expected));
    }
}

#[test]
fn trap_idct_extremes() {
    for size in [
        TransformSize::N4,
        TransformSize::N8,
        TransformSize::N16,
        TransformSize::N32,
    ] {
        for level in [-32_767, 32_767] {
            let coefficients = vec![level; size.side() * size.side()];
            let output = inverse_transform(&coefficients, size).unwrap();
            assert!(
                output
                    .iter()
                    .all(|&sample| (-32_768..=32_767).contains(&sample))
            );
        }
    }
}

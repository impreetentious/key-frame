use kf_transform::{TransformSize, dequantize_block, forward_transform, quantize_block};

#[test]
fn forward_quant_pipeline_is_deterministic() {
    for size in [
        TransformSize::N4,
        TransformSize::N8,
        TransformSize::N16,
        TransformSize::N32,
    ] {
        let residual: Vec<_> = (0..size.side() * size.side())
            .map(|index| i32::try_from(index % 33).unwrap() - 16)
            .collect();
        let first = forward_transform(&residual, size).unwrap();
        let second = forward_transform(&residual, size).unwrap();
        assert_eq!(first, second);
        let levels = quantize_block(&first, 32).unwrap();
        assert!(levels.iter().all(|level| level.unsigned_abs() <= 32_767));
        assert_eq!(
            dequantize_block(&levels, 32).unwrap().len(),
            size.side() * size.side()
        );
    }
}

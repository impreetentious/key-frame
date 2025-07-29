use kf_transform::{TransformError, dequantize, quantize};

#[test]
fn trap_dequant_overflow() {
    let positive = dequantize(32_767, 63).unwrap();
    let negative = dequantize(-32_767, 63).unwrap();
    assert!(positive > 0);
    assert!(negative < 0);
    assert_eq!(
        dequantize(32_768, 63),
        Err(TransformError::CoefficientMagnitude { level: 32_768 })
    );
    assert_eq!(
        dequantize(-32_768, 63),
        Err(TransformError::CoefficientMagnitude { level: -32_768 })
    );
}

#[test]
fn invalid_qp_is_a_named_error() {
    assert_eq!(quantize(1, 64), Err(TransformError::InvalidQp { qp: 64 }));
}

use kf_core::rounded_shift_i64;

use crate::{TransformError, TransformSize};

/// Applies the decoder-normative separable inverse transform.
pub fn inverse_transform(
    coefficients: &[i32],
    size: TransformSize,
) -> Result<Vec<i32>, TransformError> {
    let side = size.side();
    let expected = side * side;
    if coefficients.len() != expected {
        return Err(TransformError::WrongLength {
            expected,
            actual: coefficients.len(),
        });
    }
    let matrix = size.matrix();
    let mut horizontal = vec![0_i32; expected];
    for frequency_y in 0..side {
        for sample_x in 0..side {
            let mut sum = 0_i64;
            for frequency_x in 0..side {
                let coefficient = i64::from(coefficients[frequency_y * side + frequency_x]);
                let basis = i64::from(matrix[frequency_x * side + sample_x]);
                sum = sum.saturating_add(coefficient.saturating_mul(basis));
            }
            let rounded = rounded_shift_i64(sum, size.inverse_first_shift());
            horizontal[frequency_y * side + sample_x] =
                i32::try_from(rounded.clamp(i64::from(i32::MIN), i64::from(i32::MAX)))
                    .expect("invariant: first inverse stage is clamped to i32");
        }
    }

    let mut output = vec![0_i32; expected];
    for sample_y in 0..side {
        for sample_x in 0..side {
            let mut sum = 0_i64;
            for frequency_y in 0..side {
                let intermediate = i64::from(horizontal[frequency_y * side + sample_x]);
                let basis = i64::from(matrix[frequency_y * side + sample_y]);
                sum = sum.saturating_add(intermediate.saturating_mul(basis));
            }
            let rounded = rounded_shift_i64(sum, size.inverse_second_shift());
            output[sample_y * side + sample_x] = i32::try_from(rounded.clamp(-32_768, 32_767))
                .expect("invariant: final inverse stage is clamped to i16 domain");
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::inverse_transform;
    use crate::{TransformError, TransformSize};

    #[test]
    fn zero_coefficients_reconstruct_zero() {
        for size in [
            TransformSize::N4,
            TransformSize::N8,
            TransformSize::N16,
            TransformSize::N32,
        ] {
            let output = inverse_transform(&vec![0; size.side() * size.side()], size).unwrap();
            assert!(output.iter().all(|&value| value == 0));
        }
    }

    #[test]
    fn wrong_shape_is_rejected_before_indexing() {
        assert_eq!(
            inverse_transform(&[0; 15], TransformSize::N4),
            Err(TransformError::WrongLength {
                expected: 16,
                actual: 15
            })
        );
    }
}

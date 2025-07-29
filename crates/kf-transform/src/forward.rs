use kf_core::rounded_shift_i64;

use crate::{TransformError, TransformSize};

/// Applies the deterministic encoder-side reference forward transform.
pub fn forward_transform(
    residual: &[i32],
    size: TransformSize,
) -> Result<Vec<i32>, TransformError> {
    let side = size.side();
    let expected = side * side;
    if residual.len() != expected {
        return Err(TransformError::WrongLength {
            expected,
            actual: residual.len(),
        });
    }
    let matrix = size.matrix();
    let shift1 = match size {
        TransformSize::N4 => 3,
        TransformSize::N8 => 4,
        TransformSize::N16 => 5,
        TransformSize::N32 => 6,
    };
    let shift2 = match size {
        TransformSize::N4 => 10,
        TransformSize::N8 => 11,
        TransformSize::N16 => 12,
        TransformSize::N32 => 13,
    };

    let mut horizontal = vec![0_i32; expected];
    for sample_y in 0..side {
        for frequency_x in 0..side {
            let mut sum = 0_i64;
            for sample_x in 0..side {
                let sample = i64::from(residual[sample_y * side + sample_x]);
                let basis = i64::from(matrix[frequency_x * side + sample_x]);
                sum = sum.saturating_add(sample.saturating_mul(basis));
            }
            let rounded = rounded_shift_i64(sum, shift1);
            horizontal[sample_y * side + frequency_x] =
                i32::try_from(rounded.clamp(i64::from(i32::MIN), i64::from(i32::MAX)))
                    .expect("invariant: first forward stage is clamped to i32");
        }
    }

    let mut output = vec![0_i32; expected];
    for frequency_y in 0..side {
        for frequency_x in 0..side {
            let mut sum = 0_i64;
            for sample_y in 0..side {
                let intermediate = i64::from(horizontal[sample_y * side + frequency_x]);
                let basis = i64::from(matrix[frequency_y * side + sample_y]);
                sum = sum.saturating_add(intermediate.saturating_mul(basis));
            }
            let rounded = rounded_shift_i64(sum, shift2);
            output[frequency_y * side + frequency_x] =
                i32::try_from(rounded.clamp(i64::from(i32::MIN), i64::from(i32::MAX)))
                    .expect("invariant: second forward stage is clamped to i32");
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::forward_transform;
    use crate::TransformSize;

    #[test]
    fn zero_residual_has_no_coefficients() {
        for size in [
            TransformSize::N4,
            TransformSize::N8,
            TransformSize::N16,
            TransformSize::N32,
        ] {
            let coefficients =
                forward_transform(&vec![0; size.side() * size.side()], size).unwrap();
            assert!(coefficients.iter().all(|&value| value == 0));
        }
    }
}

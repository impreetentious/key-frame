use std::sync::OnceLock;

use kf_spec::V1_ASSETS;

use crate::TransformError;

const MAX_LEVEL: i32 = 32_767;

/// Quantizes one transform coefficient with symmetric integer rounding.
pub fn quantize(coefficient: i32, qp: u8) -> Result<i32, TransformError> {
    let scale = i64::from(scale(qp)?);
    let magnitude = i64::from(coefficient).saturating_abs();
    let level = magnitude.saturating_mul(16).saturating_add(scale / 2) / scale;
    let clamped = level.min(i64::from(MAX_LEVEL));
    let signed = if coefficient < 0 { -clamped } else { clamped };
    Ok(i32::try_from(signed).expect("invariant: quantized level is clamped to i32"))
}

/// Dequantizes one legal bitstream coefficient in i64 before narrowing.
pub fn dequantize(level: i32, qp: u8) -> Result<i32, TransformError> {
    if level.unsigned_abs() > u32::try_from(MAX_LEVEL).expect("invariant: positive cap fits u32") {
        return Err(TransformError::CoefficientMagnitude { level });
    }
    let value = (i64::from(level)
        .saturating_mul(i64::from(scale(qp)?))
        .saturating_add(8))
        >> 4;
    Ok(i32::try_from(value).expect("invariant: legal level and qscale fit i32 after dequant"))
}

/// Quantizes a complete coefficient block.
pub fn quantize_block(coefficients: &[i32], qp: u8) -> Result<Vec<i32>, TransformError> {
    coefficients
        .iter()
        .map(|&coefficient| quantize(coefficient, qp))
        .collect()
}

/// Dequantizes a complete legal coefficient block.
pub fn dequantize_block(levels: &[i32], qp: u8) -> Result<Vec<i32>, TransformError> {
    levels.iter().map(|&level| dequantize(level, qp)).collect()
}

fn scale(qp: u8) -> Result<i32, TransformError> {
    qscales()
        .get(usize::from(qp))
        .copied()
        .ok_or(TransformError::InvalidQp { qp })
}

fn qscales() -> &'static [i32] {
    static SCALES: OnceLock<Vec<i32>> = OnceLock::new();
    SCALES
        .get_or_init(|| {
            let asset = V1_ASSETS
                .iter()
                .find(|asset| asset.name == "quant.toml")
                .expect("invariant: kf-spec exposes quant.toml");
            let prefix = "qscale = [";
            let line = asset
                .contents
                .lines()
                .find(|line| line.starts_with(prefix))
                .expect("invariant: checked quant asset has qscale");
            let body = line
                .strip_prefix(prefix)
                .and_then(|value| value.strip_suffix(']'))
                .expect("invariant: checked qscale has balanced brackets");
            let values: Vec<i32> = body
                .split(',')
                .map(|value| {
                    value
                        .trim()
                        .parse::<i32>()
                        .expect("invariant: checked qscale value is i32")
                })
                .collect();
            assert_eq!(
                values.len(),
                64,
                "invariant: checked qscale has one value per QP"
            );
            values
        })
        .as_slice()
}

#[cfg(test)]
mod tests {
    use super::{dequantize, quantize};

    #[test]
    fn zero_is_stable_at_every_qp() {
        for qp in 0..=63 {
            assert_eq!(quantize(0, qp).unwrap(), 0);
            assert_eq!(dequantize(0, qp).unwrap(), 0);
        }
    }
}

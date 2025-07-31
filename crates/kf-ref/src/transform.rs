use kf_spec::V1_ASSETS;

pub(crate) fn inverse_levels(
    levels: &[i32],
    qp: u8,
    size: usize,
) -> Result<Vec<i32>, crate::ReferenceError> {
    let scale = qscale(qp)?;
    let coefficients: Vec<i32> = levels
        .iter()
        .map(|&level| {
            if level.unsigned_abs() > 32_767 {
                return Err(crate::ReferenceError::new(0, "coefficient.level_cap"));
            }
            let value = (i64::from(level) * i64::from(scale) + 8) >> 4;
            i32::try_from(value).map_err(|_| crate::ReferenceError::new(0, "dequantize"))
        })
        .collect::<Result<_, _>>()?;
    Ok(inverse(&coefficients, size))
}

fn inverse(coefficients: &[i32], size: usize) -> Vec<i32> {
    let matrix = matrix(size);
    let mut horizontal = vec![0_i32; size * size];
    for fy in 0..size {
        for x in 0..size {
            let mut total = 0_i64;
            for fx in 0..size {
                total += i64::from(coefficients[fy * size + fx]) * i64::from(matrix[fx * size + x]);
            }
            horizontal[fy * size + x] = i32::try_from(rounded_shift(total, 7))
                .expect("invariant: zero reference stage fits i32");
        }
    }
    let shift = match size {
        4 => 11,
        8 => 10,
        16 => 9,
        32 => 8,
        _ => panic!("invariant: reference transform size is one of 4/8/16/32"),
    };
    let mut output = vec![0_i32; size * size];
    for y in 0..size {
        for x in 0..size {
            let mut total = 0_i64;
            for fy in 0..size {
                total += i64::from(horizontal[fy * size + x]) * i64::from(matrix[fy * size + y]);
            }
            output[y * size + x] = i32::try_from(rounded_shift(total, shift).clamp(-32768, 32767))
                .expect("invariant: reference residual is clamped to i16 domain");
        }
    }
    output
}

fn matrix(size: usize) -> Vec<i16> {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "transforms.toml")
        .expect("invariant: kf-spec exposes transforms.toml");
    let prefix = format!("n{size} = [");
    let line = asset
        .contents
        .lines()
        .find(|line| line.starts_with(&prefix))
        .expect("invariant: checked transform size exists");
    line.strip_prefix(&prefix)
        .and_then(|body| body.strip_suffix(']'))
        .expect("invariant: checked transform array has balanced brackets")
        .split(',')
        .map(|value| {
            value
                .trim()
                .parse::<i16>()
                .expect("invariant: checked matrix coefficient is i16")
        })
        .collect()
}

fn qscale(qp: u8) -> Result<i32, crate::ReferenceError> {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "quant.toml")
        .expect("invariant: kf-spec exposes quant.toml");
    let prefix = "qscale = [";
    let line = asset
        .contents
        .lines()
        .find(|line| line.starts_with(prefix))
        .expect("invariant: checked quant asset contains qscale");
    line.strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(']'))
        .expect("invariant: checked qscale has balanced brackets")
        .split(',')
        .nth(usize::from(qp))
        .ok_or_else(|| crate::ReferenceError::new(0, "frame.qp"))?
        .trim()
        .parse::<i32>()
        .map_err(|_| crate::ReferenceError::new(0, "quant.asset"))
}

fn rounded_shift(value: i64, shift: u8) -> i64 {
    let bias = 1_i64 << (shift - 1);
    if value >= 0 {
        (value + bias) >> shift
    } else {
        -((value.saturating_abs() + bias) >> shift)
    }
}

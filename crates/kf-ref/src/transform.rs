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
    let first_shift = declared_shift("inverse_shift1", size);
    let second_shift = declared_shift("inverse_shift2", size);
    let mut horizontal = vec![0_i32; size * size];
    for fy in 0..size {
        for x in 0..size {
            let mut total = 0_i64;
            for fx in 0..size {
                total += i64::from(coefficients[fy * size + fx]) * i64::from(matrix[fx * size + x]);
            }
            horizontal[fy * size + x] = i32::try_from(rounded_shift(total, first_shift))
                .expect("invariant: zero reference stage fits i32");
        }
    }
    let mut output = vec![0_i32; size * size];
    for y in 0..size {
        for x in 0..size {
            let mut total = 0_i64;
            for fy in 0..size {
                total += i64::from(horizontal[fy * size + x]) * i64::from(matrix[fy * size + y]);
            }
            output[y * size + x] =
                i32::try_from(rounded_shift(total, second_shift).clamp(-32768, 32767))
                    .expect("invariant: reference residual is clamped to i16 domain");
        }
    }
    output
}

/// Reads a stage shift out of the frozen asset and evaluates what it declares.
///
/// This decoder is meant to agree with the production one only by agreeing with
/// the specification, so it takes the shifts from the same declaration rather
/// than from a table of its own. A table here would agree with `kf-transform`'s
/// literals and with nothing else, which is precisely the failure two decoders
/// exist to catch: both would keep the old scaling after a specification edit
/// and their agreement would prove nothing.
///
/// The parser is deliberately this crate's own. Independence means not sharing
/// the implementation, not refusing to read the same normative bytes.
fn declared_shift(key: &str, size: usize) -> u8 {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "transforms.toml")
        .expect("invariant: kf-spec exposes transforms.toml");
    let prefix = format!("{key} = ");
    let declaration = asset
        .contents
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .expect("invariant: checked transform asset declares every stage shift")
        .trim()
        .replace(['"', ' '], "");

    let log2_side = size
        .checked_ilog2()
        .expect("invariant: transform side is a positive power of two");
    let log2_side = u8::try_from(log2_side).expect("invariant: transform side is small");
    assert_eq!(
        1_usize << log2_side,
        size,
        "invariant: transform side is a power of two"
    );

    if let Ok(literal) = declaration.parse::<u8>() {
        literal
    } else if let Some(addend) = declaration.strip_prefix("log2(N)+") {
        log2_side
            + addend
                .parse::<u8>()
                .expect("invariant: checked shift addend is a small integer")
    } else if let Some((minuend, subtrahend)) = declaration.split_once('-')
        && subtrahend == "log2(N)"
    {
        minuend
            .parse::<u8>()
            .expect("invariant: checked shift minuend is a small integer")
            - log2_side
    } else {
        panic!("invariant: checked transform asset uses a known shift form: {declaration}")
    }
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

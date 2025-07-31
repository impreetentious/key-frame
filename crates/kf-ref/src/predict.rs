use kf_frame::Plane;

use crate::{ReferenceError, syntax::RefIntraMode};

pub(crate) fn predict(
    plane: &Plane,
    x: u32,
    y: u32,
    size: u32,
    mode: RefIntraMode,
) -> Result<Vec<u8>, ReferenceError> {
    let side = usize::try_from(size).map_err(|_| ReferenceError::new(0, "predict.size"))?;
    let extent = side * 2 + 1;
    let mut top = Vec::new();
    let mut left = Vec::new();
    if y > 0 {
        for offset in 0..extent {
            let offset = u32::try_from(offset).unwrap();
            top.push(
                plane
                    .get(x.saturating_add(offset).min(plane.width() - 1), y - 1)
                    .map_err(|_| ReferenceError::new(0, "predict.top"))?,
            );
        }
    }
    if x > 0 {
        for offset in 0..extent {
            let offset = u32::try_from(offset).unwrap();
            left.push(
                plane
                    .get(x - 1, y.saturating_add(offset).min(plane.height() - 1))
                    .map_err(|_| ReferenceError::new(0, "predict.left"))?,
            );
        }
    }
    let top_available = !top.is_empty();
    let left_available = !left.is_empty();
    match (top_available, left_available) {
        (false, false) => {
            top.resize(extent, 128);
            left.resize(extent, 128);
        }
        (true, false) => left.resize(extent, top[0]),
        (false, true) => top.resize(extent, left[0]),
        (true, true) => {}
    }

    let mut output = vec![0_u8; side * side];
    match mode {
        RefIntraMode::Dc => {
            let mut sum = 0_u32;
            let mut count = 0_u32;
            if top_available {
                sum += top[..side]
                    .iter()
                    .map(|&sample| u32::from(sample))
                    .sum::<u32>();
                count += size;
            }
            if left_available {
                sum += left[..side]
                    .iter()
                    .map(|&sample| u32::from(sample))
                    .sum::<u32>();
                count += size;
            }
            let dc = (sum + count / 2).checked_div(count).map_or(128, |value| {
                u8::try_from(value).expect("invariant: byte reference mean stays a byte")
            });
            output.fill(dc);
        }
        RefIntraMode::Planar => {
            let denominator = size * 2;
            for row in 0..side {
                for column in 0..side {
                    let value = u32::try_from(side - 1 - column).unwrap() * u32::from(left[row])
                        + u32::try_from(column + 1).unwrap() * u32::from(top[side])
                        + u32::try_from(side - 1 - row).unwrap() * u32::from(top[column])
                        + u32::try_from(row + 1).unwrap() * u32::from(left[side]);
                    output[row * side + column] =
                        u8::try_from((value + size) / denominator).unwrap();
                }
            }
        }
        RefIntraMode::Horizontal => {
            for row in 0..side {
                output[row * side..(row + 1) * side].fill(left[row]);
            }
        }
        RefIntraMode::Vertical => {
            for row in output.chunks_exact_mut(side) {
                row.copy_from_slice(&top[..side]);
            }
        }
        RefIntraMode::D45 => angular(&mut output, &top, &left, side, 32),
        RefIntraMode::D135 => angular(&mut output, &top, &left, side, -32),
        RefIntraMode::D117 => angular(&mut output, &top, &left, side, -21),
        RefIntraMode::D153 => angular(&mut output, &top, &left, side, -11),
    }
    Ok(output)
}

fn angular(output: &mut [u8], top: &[u8], left: &[u8], side: usize, angle: i32) {
    for row in 0..side {
        for column in 0..side {
            let projected =
                i32::try_from(column).unwrap() * 32 + (i32::try_from(row).unwrap() + 1) * angle;
            let index = projected.div_euclid(32);
            let fraction = u32::try_from(projected.rem_euclid(32)).unwrap();
            let sample = |position: i32| {
                if position >= 0 {
                    top[usize::try_from(position).unwrap().min(top.len() - 1)]
                } else {
                    left[usize::try_from(-position - 1).unwrap().min(left.len() - 1)]
                }
            };
            let value = (32 - fraction) * u32::from(sample(index))
                + fraction * u32::from(sample(index + 1));
            output[row * side + column] = u8::try_from((value + 16) >> 5).unwrap();
        }
    }
}

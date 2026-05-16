use std::sync::OnceLock;

use kf_frame::Plane;
use kf_spec::V1_ASSETS;

use crate::{ReferenceError, syntax::RefIntraMode};

/// Intra constants read from the frozen asset through this crate's own parser.
///
/// Independence means not sharing the implementation with `kf-predict`, not
/// keeping a private copy of the numbers. A private copy would agree with the
/// production decoder's copy and with nothing else, so the two decoders would
/// go on agreeing after a specification edit that neither of them honoured.
struct RefIntra {
    denominator: i32,
    shift: u32,
    rounding: u32,
    fallback: u8,
    d45: i32,
    d135: i32,
    d117: i32,
    d153: i32,
}

fn intra() -> &'static RefIntra {
    static INTRA: OnceLock<RefIntra> = OnceLock::new();
    INTRA.get_or_init(|| {
        let contents = V1_ASSETS
            .iter()
            .find(|asset| asset.name == "intra.toml")
            .expect("invariant: kf-spec exposes intra.toml")
            .contents;
        let number = |key: &str| -> i32 {
            let prefix = format!("{key} = ");
            contents
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .expect("invariant: checked intra asset declares every scalar")
                .trim()
                .parse::<i32>()
                .expect("invariant: checked intra scalar is an integer")
        };
        let denominator = number("angular_denominator");
        let shift = denominator
            .checked_ilog2()
            .expect("invariant: checked angular denominator is a positive power of two");
        RefIntra {
            denominator,
            shift,
            rounding: u32::try_from(number("angular_rounding_offset"))
                .expect("invariant: checked rounding offset is not negative"),
            fallback: u8::try_from(number("unavailable_fallback"))
                .expect("invariant: checked fallback sample is a byte"),
            d45: number("d45"),
            d135: number("d135"),
            d117: number("d117"),
            d153: number("d153"),
        }
    })
}

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
            top.resize(extent, intra().fallback);
            left.resize(extent, intra().fallback);
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
            let dc = (sum + count / 2)
                .checked_div(count)
                .map_or(intra().fallback, |value| {
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
        RefIntraMode::D45 => angular(&mut output, &top, &left, side, intra().d45),
        RefIntraMode::D135 => angular(&mut output, &top, &left, side, intra().d135),
        RefIntraMode::D117 => angular(&mut output, &top, &left, side, intra().d117),
        RefIntraMode::D153 => angular(&mut output, &top, &left, side, intra().d153),
    }
    Ok(output)
}

fn angular(output: &mut [u8], top: &[u8], left: &[u8], side: usize, angle: i32) {
    for row in 0..side {
        for column in 0..side {
            let projected = i32::try_from(column).unwrap() * intra().denominator
                + (i32::try_from(row).unwrap() + 1) * angle;
            let index = projected.div_euclid(intra().denominator);
            let fraction = u32::try_from(projected.rem_euclid(intra().denominator)).unwrap();
            let sample = |position: i32| {
                if position >= 0 {
                    top[usize::try_from(position).unwrap().min(top.len() - 1)]
                } else {
                    left[usize::try_from(-position - 1).unwrap().min(left.len() - 1)]
                }
            };
            let value = (32 - fraction) * u32::from(sample(index))
                + fraction * u32::from(sample(index + 1));
            output[row * side + column] =
                u8::try_from((value + intra().rounding) >> intra().shift).unwrap();
        }
    }
}

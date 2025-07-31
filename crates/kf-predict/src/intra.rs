use kf_frame::Plane;

use crate::PredictError;

/// Closed version-one intra prediction mode set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntraMode {
    Dc,
    Planar,
    Horizontal,
    Vertical,
    D45,
    D135,
    D117,
    D153,
}

/// Predicts one square block from already reconstructed top and left samples.
pub fn predict_intra(
    plane: &Plane,
    x: u32,
    y: u32,
    size: u32,
    mode: IntraMode,
) -> Result<Vec<u8>, PredictError> {
    if !matches!(size, 4 | 8 | 16 | 32 | 64) {
        return Err(PredictError::InvalidSize { size });
    }
    let end_x = x
        .checked_add(size)
        .ok_or(PredictError::BlockOutOfBounds { x, y, size })?;
    let end_y = y
        .checked_add(size)
        .ok_or(PredictError::BlockOutOfBounds { x, y, size })?;
    if end_x > plane.width() || end_y > plane.height() {
        return Err(PredictError::BlockOutOfBounds { x, y, size });
    }

    let references = References::new(plane, x, y, size)?;
    let side = usize::try_from(size).expect("invariant: supported side fits usize");
    let mut output = vec![0_u8; side * side];
    match mode {
        IntraMode::Dc => output.fill(references.dc()),
        IntraMode::Planar => references.planar(&mut output),
        IntraMode::Horizontal => {
            for row in 0..side {
                output[row * side..(row + 1) * side].fill(references.left[row]);
            }
        }
        IntraMode::Vertical => {
            for row in output.chunks_exact_mut(side) {
                row.copy_from_slice(&references.top[..side]);
            }
        }
        IntraMode::D45 => references.angular(&mut output, 32),
        IntraMode::D135 => references.angular(&mut output, -32),
        IntraMode::D117 => references.angular(&mut output, -21),
        IntraMode::D153 => references.angular(&mut output, -11),
    }
    Ok(output)
}

struct References {
    side: usize,
    top_available: bool,
    left_available: bool,
    top: Vec<u8>,
    left: Vec<u8>,
}

impl References {
    fn new(plane: &Plane, x: u32, y: u32, size: u32) -> Result<Self, PredictError> {
        let side = usize::try_from(size).expect("invariant: supported side fits usize");
        let extent = side * 2 + 1;
        let top_available = y > 0;
        let left_available = x > 0;
        let mut top = Vec::with_capacity(extent);
        let mut left = Vec::with_capacity(extent);
        if top_available {
            for offset in 0..extent {
                let offset = u32::try_from(offset).expect("invariant: reference extent fits u32");
                top.push(plane.get(x.saturating_add(offset).min(plane.width() - 1), y - 1)?);
            }
        }
        if left_available {
            for offset in 0..extent {
                let offset = u32::try_from(offset).expect("invariant: reference extent fits u32");
                left.push(plane.get(x - 1, y.saturating_add(offset).min(plane.height() - 1))?);
            }
        }
        match (top_available, left_available) {
            (false, false) => {
                top.resize(extent, 128);
                left.resize(extent, 128);
            }
            (true, false) => left.resize(extent, top[0]),
            (false, true) => top.resize(extent, left[0]),
            (true, true) => {}
        }
        Ok(Self {
            side,
            top_available,
            left_available,
            top,
            left,
        })
    }

    fn dc(&self) -> u8 {
        let mut sum = 0_u32;
        let mut count = 0_u32;
        if self.top_available {
            sum += self.top[..self.side]
                .iter()
                .map(|&sample| u32::from(sample))
                .sum::<u32>();
            count += u32::try_from(self.side).expect("invariant: side fits u32");
        }
        if self.left_available {
            sum += self.left[..self.side]
                .iter()
                .map(|&sample| u32::from(sample))
                .sum::<u32>();
            count += u32::try_from(self.side).expect("invariant: side fits u32");
        }
        (sum + count / 2).checked_div(count).map_or(128, |value| {
            u8::try_from(value).expect("invariant: average of byte references remains a byte")
        })
    }

    fn planar(&self, output: &mut [u8]) {
        let side = self.side;
        let top_right = self.top[side];
        let bottom_left = self.left[side];
        let denominator = u32::try_from(side * 2).expect("invariant: side fits u32");
        for row in 0..side {
            for column in 0..side {
                let left_weight = u32::try_from(side - 1 - column).unwrap();
                let right_weight = u32::try_from(column + 1).unwrap();
                let top_weight = u32::try_from(side - 1 - row).unwrap();
                let bottom_weight = u32::try_from(row + 1).unwrap();
                let value = left_weight * u32::from(self.left[row])
                    + right_weight * u32::from(top_right)
                    + top_weight * u32::from(self.top[column])
                    + bottom_weight * u32::from(bottom_left);
                output[row * side + column] =
                    u8::try_from((value + u32::try_from(side).unwrap()) / denominator)
                        .expect("invariant: planar blend remains a byte");
            }
        }
    }

    fn angular(&self, output: &mut [u8], angle: i32) {
        let side = self.side;
        for row in 0..side {
            for column in 0..side {
                let projected =
                    i32::try_from(column).unwrap() * 32 + (i32::try_from(row).unwrap() + 1) * angle;
                let index = projected.div_euclid(32);
                let fraction = projected.rem_euclid(32);
                let a = u32::from(self.main(index));
                let b = u32::from(self.main(index + 1));
                let fraction = u32::try_from(fraction).unwrap();
                output[row * side + column] =
                    u8::try_from(((32 - fraction) * a + fraction * b + 16) >> 5)
                        .expect("invariant: angular interpolation remains a byte");
            }
        }
    }

    fn main(&self, index: i32) -> u8 {
        if index >= 0 {
            self.top[usize::try_from(index).unwrap().min(self.top.len() - 1)]
        } else {
            let left_index = usize::try_from(-index - 1).unwrap();
            self.left[left_index.min(self.left.len() - 1)]
        }
    }
}

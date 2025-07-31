use crate::{MotionVector, PredictError};

const CELL_SIZE: u32 = 8;

/// A decoded reference-frame slot used by spatial motion prediction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceSlot {
    Last,
    Golden,
}

/// The prediction state recorded over one coding block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockMotion {
    Intra,
    Inter {
        reference: ReferenceSlot,
        motion_vector: MotionVector,
    },
}

/// An 8×8-cell spatial motion map for one frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MotionField {
    width: u32,
    height: u32,
    cells_per_row: u32,
    cells: Vec<Option<BlockMotion>>,
}

impl MotionField {
    /// Allocates an empty field over a padded luma frame.
    pub fn new(width: u32, height: u32) -> Result<Self, PredictError> {
        if width == 0
            || height == 0
            || !width.is_multiple_of(CELL_SIZE)
            || !height.is_multiple_of(CELL_SIZE)
        {
            return Err(PredictError::InvalidMotionField { width, height });
        }
        let cells_per_row = width / CELL_SIZE;
        let cell_rows = height / CELL_SIZE;
        let cell_count = u64::from(cells_per_row) * u64::from(cell_rows);
        let cell_count = usize::try_from(cell_count)
            .map_err(|_| PredictError::InvalidMotionField { width, height })?;
        Ok(Self {
            width,
            height,
            cells_per_row,
            cells: vec![None; cell_count],
        })
    }

    /// Records one non-overlapping, aligned coding block after it is chosen.
    pub fn record(
        &mut self,
        x: u32,
        y: u32,
        size: u32,
        motion: BlockMotion,
    ) -> Result<(), PredictError> {
        self.validate_block(x, y, size)?;
        for cell_y in y / CELL_SIZE..(y + size) / CELL_SIZE {
            for cell_x in x / CELL_SIZE..(x + size) / CELL_SIZE {
                let index = self.cell_index(cell_x, cell_y);
                if self.cells[index].is_some() {
                    return Err(PredictError::MotionFieldOverlap {
                        x: cell_x * CELL_SIZE,
                        y: cell_y * CELL_SIZE,
                    });
                }
            }
        }
        for cell_y in y / CELL_SIZE..(y + size) / CELL_SIZE {
            for cell_x in x / CELL_SIZE..(x + size) / CELL_SIZE {
                let index = self.cell_index(cell_x, cell_y);
                self.cells[index] = Some(motion);
            }
        }
        Ok(())
    }

    /// Returns the normative componentwise-median predictor for a new block.
    pub fn predictor(
        &self,
        x: u32,
        y: u32,
        size: u32,
        reference: ReferenceSlot,
    ) -> Result<MotionVector, PredictError> {
        self.validate_block(x, y, size)?;
        let left = self.matching_at(i64::from(x) - 1, i64::from(y), reference);
        let above = self.matching_at(i64::from(x), i64::from(y) - 1, reference);
        let above_right =
            self.matching_at(i64::from(x) + i64::from(size), i64::from(y) - 1, reference);
        let above_left = self.matching_at(i64::from(x) - 1, i64::from(y) - 1, reference);
        let third = above_right.or(above_left).unwrap_or_default();
        let left = left.unwrap_or_default();
        let above = above.unwrap_or_default();
        Ok(MotionVector {
            x_q4: median(left.x_q4, above.x_q4, third.x_q4),
            y_q4: median(left.y_q4, above.y_q4, third.y_q4),
        })
    }

    fn validate_block(&self, x: u32, y: u32, size: u32) -> Result<(), PredictError> {
        if !matches!(size, 8 | 16 | 32 | 64)
            || !x.is_multiple_of(CELL_SIZE)
            || !y.is_multiple_of(CELL_SIZE)
        {
            return Err(PredictError::InvalidSize { size });
        }
        if x.checked_add(size).is_none_or(|end| end > self.width)
            || y.checked_add(size).is_none_or(|end| end > self.height)
        {
            return Err(PredictError::BlockOutOfBounds { x, y, size });
        }
        Ok(())
    }

    fn matching_at(&self, x: i64, y: i64, reference: ReferenceSlot) -> Option<MotionVector> {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return None;
        }
        let cell_x = u32::try_from(x).ok()? / CELL_SIZE;
        let cell_y = u32::try_from(y).ok()? / CELL_SIZE;
        match self.cells[self.cell_index(cell_x, cell_y)] {
            Some(BlockMotion::Inter {
                reference: candidate_reference,
                motion_vector,
            }) if candidate_reference == reference => Some(motion_vector),
            _ => None,
        }
    }

    fn cell_index(&self, cell_x: u32, cell_y: u32) -> usize {
        usize::try_from(u64::from(cell_y) * u64::from(self.cells_per_row) + u64::from(cell_x))
            .expect("validated motion-field index fits usize")
    }
}

fn median(first: i32, second: i32, third: i32) -> i32 {
    first
        .min(second)
        .max(first.max(second).min(third))
        .min(first.max(second))
}

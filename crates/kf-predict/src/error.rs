use core::fmt;

use kf_frame::PlaneError;

/// Checked prediction failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PredictError {
    InvalidSize { size: u32 },
    BlockOutOfBounds { x: u32, y: u32, size: u32 },
    InvalidMotionField { width: u32, height: u32 },
    MotionFieldOverlap { x: u32, y: u32 },
    Plane(PlaneError),
}

impl fmt::Display for PredictError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "prediction failed: {self:?}")
    }
}

impl std::error::Error for PredictError {}

impl From<PlaneError> for PredictError {
    fn from(value: PlaneError) -> Self {
        Self::Plane(value)
    }
}

use core::fmt;

use crate::{Plane, PlaneError};

/// An 8-bit 4:2:0 frame with one luma and two chroma planes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    width: u32,
    height: u32,
    pub y: Plane,
    pub cb: Plane,
    pub cr: Plane,
}

/// Errors constructing a 4:2:0 frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameError {
    OddDimension { width: u32, height: u32 },
    Plane(PlaneError),
}

impl fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid frame: {self:?}")
    }
}

impl std::error::Error for FrameError {}

impl From<PlaneError> for FrameError {
    fn from(value: PlaneError) -> Self {
        Self::Plane(value)
    }
}

impl Frame {
    /// Allocates a neutral-chroma 4:2:0 frame with a selected luma value.
    pub fn filled_420(width: u32, height: u32, luma: u8) -> Result<Self, FrameError> {
        if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(FrameError::OddDimension { width, height });
        }
        Ok(Self {
            width,
            height,
            y: Plane::filled(width, height, luma)?,
            cb: Plane::filled(width / 2, height / 2, 128)?,
            cr: Plane::filled(width / 2, height / 2, 128)?,
        })
    }

    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }
}

#[cfg(test)]
mod tests {
    use super::{Frame, FrameError};

    #[test]
    fn four_twenty_geometry_is_exact() {
        let frame = Frame::filled_420(64, 32, 17).unwrap();
        assert_eq!((frame.y.width(), frame.y.height()), (64, 32));
        assert_eq!((frame.cb.width(), frame.cb.height()), (32, 16));
        assert_eq!(frame.cb.get(0, 0).unwrap(), 128);
    }

    #[test]
    fn odd_dimensions_are_rejected() {
        assert_eq!(
            Frame::filled_420(65, 64, 0),
            Err(FrameError::OddDimension {
                width: 65,
                height: 64
            })
        );
    }
}

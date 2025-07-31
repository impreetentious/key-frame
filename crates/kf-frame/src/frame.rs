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

    /// Copies the visible upper-left 4:2:0 rectangle from a padded frame.
    pub fn crop_420(&self, width: u32, height: u32) -> Result<Self, FrameError> {
        if width > self.width || height > self.height {
            return Err(FrameError::Plane(PlaneError::OutOfBounds {
                x: width,
                y: height,
            }));
        }
        let mut cropped = Self::filled_420(width, height, 0)?;
        copy_plane(&self.y, &mut cropped.y)?;
        copy_plane(&self.cb, &mut cropped.cb)?;
        copy_plane(&self.cr, &mut cropped.cr)?;
        Ok(cropped)
    }

    /// Pads a frame by replicating its rightmost column and bottommost row.
    pub fn pad_420_edge(&self, width: u32, height: u32) -> Result<Self, FrameError> {
        if width < self.width || height < self.height {
            return Err(FrameError::Plane(PlaneError::OutOfBounds {
                x: width,
                y: height,
            }));
        }
        let mut padded = Self::filled_420(width, height, 0)?;
        replicate_plane(&self.y, &mut padded.y)?;
        replicate_plane(&self.cb, &mut padded.cb)?;
        replicate_plane(&self.cr, &mut padded.cr)?;
        Ok(padded)
    }
}

fn copy_plane(source: &Plane, destination: &mut Plane) -> Result<(), PlaneError> {
    for y in 0..destination.height() {
        for x in 0..destination.width() {
            destination.set(x, y, source.get(x, y)?)?;
        }
    }
    Ok(())
}

fn replicate_plane(source: &Plane, destination: &mut Plane) -> Result<(), PlaneError> {
    for y in 0..destination.height() {
        for x in 0..destination.width() {
            destination.set(
                x,
                y,
                source.get(x.min(source.width() - 1), y.min(source.height() - 1))?,
            )?;
        }
    }
    Ok(())
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

    #[test]
    fn padded_frame_crops_all_three_planes() {
        let mut frame = Frame::filled_420(128, 64, 7).unwrap();
        frame.y.set(65, 31, 99).unwrap();
        frame.cb.set(32, 15, 77).unwrap();
        let cropped = frame.crop_420(66, 32).unwrap();
        assert_eq!((cropped.width(), cropped.height()), (66, 32));
        assert_eq!(cropped.y.get(65, 31).unwrap(), 99);
        assert_eq!(cropped.cb.get(32, 15).unwrap(), 77);
        assert_eq!((cropped.cr.width(), cropped.cr.height()), (33, 16));
    }

    #[test]
    fn edge_padding_replicates_visible_boundaries() {
        let mut frame = Frame::filled_420(66, 32, 7).unwrap();
        frame.y.set(65, 31, 99).unwrap();
        frame.cb.set(32, 15, 77).unwrap();
        let padded = frame.pad_420_edge(128, 64).unwrap();
        assert_eq!(padded.y.get(127, 63).unwrap(), 99);
        assert_eq!(padded.cb.get(63, 31).unwrap(), 77);
    }
}

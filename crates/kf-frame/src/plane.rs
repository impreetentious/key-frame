use core::fmt;

/// A checked planar byte buffer with an explicit stride.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plane {
    width: u32,
    height: u32,
    stride: u32,
    data: Vec<u8>,
}

/// Errors constructing or indexing a plane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaneError {
    ZeroDimension,
    StrideTooSmall { width: u32, stride: u32 },
    SizeOverflow,
    DataLength { expected: usize, actual: usize },
    OutOfBounds { x: u32, y: u32 },
}

impl fmt::Display for PlaneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid plane: {self:?}")
    }
}

impl std::error::Error for PlaneError {}

impl Plane {
    /// Allocates a tightly packed plane filled with `value`.
    pub fn filled(width: u32, height: u32, value: u8) -> Result<Self, PlaneError> {
        Self::with_stride(width, height, width, value)
    }

    /// Allocates a plane with caller-selected row stride.
    pub fn with_stride(
        width: u32,
        height: u32,
        stride: u32,
        value: u8,
    ) -> Result<Self, PlaneError> {
        if width == 0 || height == 0 {
            return Err(PlaneError::ZeroDimension);
        }
        if stride < width {
            return Err(PlaneError::StrideTooSmall { width, stride });
        }
        let length_u64 = u64::from(stride)
            .checked_mul(u64::from(height))
            .ok_or(PlaneError::SizeOverflow)?;
        let length = usize::try_from(length_u64).map_err(|_| PlaneError::SizeOverflow)?;
        Ok(Self {
            width,
            height,
            stride,
            data: vec![value; length],
        })
    }

    /// Creates a plane from an exact backing buffer.
    pub fn from_vec(
        width: u32,
        height: u32,
        stride: u32,
        data: Vec<u8>,
    ) -> Result<Self, PlaneError> {
        let mut plane = Self::with_stride(width, height, stride, 0)?;
        if plane.data.len() != data.len() {
            return Err(PlaneError::DataLength {
                expected: plane.data.len(),
                actual: data.len(),
            });
        }
        plane.data = data;
        Ok(plane)
    }

    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub const fn stride(&self) -> u32 {
        self.stride
    }

    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    #[must_use]
    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub fn get(&self, x: u32, y: u32) -> Result<u8, PlaneError> {
        let index = self.index(x, y)?;
        Ok(self.data[index])
    }

    pub fn set(&mut self, x: u32, y: u32, value: u8) -> Result<(), PlaneError> {
        let index = self.index(x, y)?;
        self.data[index] = value;
        Ok(())
    }

    fn index(&self, x: u32, y: u32) -> Result<usize, PlaneError> {
        if x >= self.width || y >= self.height {
            return Err(PlaneError::OutOfBounds { x, y });
        }
        let index = u64::from(y) * u64::from(self.stride) + u64::from(x);
        usize::try_from(index).map_err(|_| PlaneError::SizeOverflow)
    }
}

#[cfg(test)]
mod tests {
    use super::{Plane, PlaneError};

    #[test]
    fn stride_padding_is_not_visible_as_pixels() {
        let mut plane = Plane::with_stride(3, 2, 5, 9).unwrap();
        plane.set(2, 1, 42).unwrap();
        assert_eq!(plane.get(2, 1).unwrap(), 42);
        assert_eq!(plane.data().len(), 10);
        assert_eq!(plane.get(3, 1), Err(PlaneError::OutOfBounds { x: 3, y: 1 }));
    }

    #[test]
    fn backing_length_must_match_stride_times_height() {
        assert!(matches!(
            Plane::from_vec(2, 2, 3, vec![0; 4]),
            Err(PlaneError::DataLength {
                expected: 6,
                actual: 4
            })
        ));
    }
}

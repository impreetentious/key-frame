use core::fmt;

use kf_bitstream::BitstreamError;

/// Checked encoder failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodeError {
    InvalidInput { element: &'static str },
    Bitstream(BitstreamError),
    Policy { element: &'static str },
    Reconstruction { element: &'static str },
}

impl fmt::Display for EncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "encode failed: {self:?}")
    }
}

impl std::error::Error for EncodeError {}

impl From<BitstreamError> for EncodeError {
    fn from(value: BitstreamError) -> Self {
        Self::Bitstream(value)
    }
}

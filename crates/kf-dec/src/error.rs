use core::fmt;

use kf_bitstream::BitstreamError;

/// Structured fast-decoder failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    Bitstream(BitstreamError),
    InvalidFrame {
        frame_index: u32,
        element: &'static str,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bitstream(error) => error.fmt(formatter),
            Self::InvalidFrame {
                frame_index,
                element,
            } => write!(formatter, "invalid {element} in frame {frame_index}"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl From<BitstreamError> for DecodeError {
    fn from(value: BitstreamError) -> Self {
        Self::Bitstream(value)
    }
}

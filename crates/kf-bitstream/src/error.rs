use core::fmt;

/// Structured failures at a named bitstream element and byte offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BitstreamError {
    UnexpectedEof { offset: u32, element: &'static str },
    InvalidField { offset: u32, element: &'static str },
    CrcMismatch { offset: u32, element: &'static str },
    UnsupportedVersion { version: u16 },
}

impl fmt::Display for BitstreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof { offset, element } => {
                write!(formatter, "{element} ended at byte {offset}")
            }
            Self::InvalidField { offset, element } => {
                write!(formatter, "invalid {element} at byte {offset}")
            }
            Self::CrcMismatch { offset, element } => {
                write!(formatter, "CRC mismatch for {element} at byte {offset}")
            }
            Self::UnsupportedVersion { version } => {
                write!(formatter, "unsupported bitstream version {version}")
            }
        }
    }
}

impl std::error::Error for BitstreamError {}

use core::fmt;

/// Checked transform and quantization failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransformError {
    /// Input sample/coefficient count does not equal N².
    WrongLength { expected: usize, actual: usize },
    /// An embedded matrix cannot satisfy the frozen shape.
    InvalidSpecMatrix { size: u8 },
}

impl fmt::Display for TransformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongLength { expected, actual } => {
                write!(
                    formatter,
                    "transform length {actual} does not equal {expected}"
                )
            }
            Self::InvalidSpecMatrix { size } => {
                write!(
                    formatter,
                    "embedded {size}x{size} transform matrix is malformed"
                )
            }
        }
    }
}

impl std::error::Error for TransformError {}

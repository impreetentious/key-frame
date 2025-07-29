use core::fmt;

/// Failures at the entropy-coding boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangeError {
    /// A probability is outside the 12-bit open interval.
    InvalidProbability { p1: u16 },
    /// A context id is outside the frozen bank.
    InvalidContext { id: u16 },
    /// An embedded normative table cannot satisfy its frozen shape.
    InvalidSpecAsset { asset: &'static str },
    /// Decoder initialization or renormalization reached the payload bound.
    EndOfInput { byte_offset: u32 },
}

impl fmt::Display for RangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProbability { p1 } => {
                write!(formatter, "probability p1={p1} is outside 1..=4095")
            }
            Self::InvalidContext { id } => write!(formatter, "context id {id} is outside 0..144"),
            Self::InvalidSpecAsset { asset } => {
                write!(formatter, "embedded normative asset {asset} is malformed")
            }
            Self::EndOfInput { byte_offset } => {
                write!(formatter, "range payload ended at byte {byte_offset}")
            }
        }
    }
}

impl std::error::Error for RangeError {}

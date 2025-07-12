use core::fmt;

/// Failures produced by reusable core primitives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreError {
    /// A bit reader was asked to consume more data than remains.
    EndOfInput { bit_offset: u64 },
    /// A fixed-width operation requested a width outside its contract.
    InvalidBitWidth { width: u8 },
    /// A size computation exceeded its fixed-width representation.
    SizeOverflow,
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EndOfInput { bit_offset } => {
                write!(formatter, "unexpected end of input at bit {bit_offset}")
            }
            Self::InvalidBitWidth { width } => {
                write!(formatter, "bit width {width} is outside 0..=32")
            }
            Self::SizeOverflow => formatter.write_str("size calculation overflowed"),
        }
    }
}

impl std::error::Error for CoreError {}

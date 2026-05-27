use crate::{Probability, RangeError};

const RANGE_INITIAL: u32 = 0xFFFF_FFFF;
const RANGE_TOP: u32 = 1 << 24;

/// Bounds-checked decoder for one CRC-delimited range payload.
#[derive(Clone, Debug)]
pub struct RangeDecoder<'a> {
    input: &'a [u8],
    offset: u32,
    range: u32,
    code: u32,
}

impl<'a> RangeDecoder<'a> {
    /// Initializes `code` from exactly five wrapping byte shifts.
    pub fn new(input: &'a [u8]) -> Result<Self, RangeError> {
        let mut decoder = Self {
            input,
            offset: 0,
            range: RANGE_INITIAL,
            code: 0,
        };
        for _ in 0..5 {
            decoder.code = decoder.code.wrapping_shl(8) | u32::from(decoder.read_byte()?);
        }
        Ok(decoder)
    }

    /// Decodes and then adapts one context-coded bin.
    pub fn decode_context(&mut self, probability: &mut Probability) -> Result<bool, RangeError> {
        let symbol = self.decode_bin(probability.p1())?;
        probability.update(symbol);
        Ok(symbol)
    }

    /// Decodes one fixed-half bin without modifying any context.
    pub fn decode_bypass(&mut self) -> Result<bool, RangeError> {
        self.decode_bin(2048)
    }

    /// Remaining CRC-bound finalization bytes after dimension-derived syntax.
    #[must_use]
    pub fn unread_tail(&self) -> &'a [u8] {
        let offset = usize::try_from(self.offset)
            .expect("invariant: decoder offset originated from slice indices");
        &self.input[offset..]
    }

    fn decode_bin(&mut self, p1: u16) -> Result<bool, RangeError> {
        let p0 = 4096_u32 - u32::from(p1);
        let bound = (self.range >> 12) * p0;
        let symbol = if self.code < bound {
            self.range = bound;
            false
        } else {
            self.code -= bound;
            self.range -= bound;
            true
        };
        while self.range < RANGE_TOP {
            self.range <<= 8;
            self.code = self.code.wrapping_shl(8) | u32::from(self.read_byte()?);
        }
        Ok(symbol)
    }

    fn read_byte(&mut self) -> Result<u8, RangeError> {
        let offset = usize::try_from(self.offset)
            .expect("invariant: decoder offset originated from slice indices");
        let byte = self
            .input
            .get(offset)
            .copied()
            .ok_or(RangeError::EndOfInput {
                byte_offset: self.offset,
            })?;
        self.offset = self
            .offset
            .checked_add(1)
            .expect("invariant: payload cap keeps byte offset in u32");
        Ok(byte)
    }
}

#[cfg(test)]
mod tests {
    use super::RangeDecoder;
    use crate::{Probability, RangeError};

    #[test]
    fn trap_range_short_init() {
        for length in 0..5 {
            assert!(matches!(
                RangeDecoder::new(&[0; 4][..length]),
                Err(RangeError::EndOfInput {
                    byte_offset
                })
                if byte_offset == u32::try_from(length).unwrap()
            ));
        }
    }

    #[test]
    fn decoder_accepts_an_unread_finalization_tail() {
        let bytes = [0, 0, 0, 0, 0, 0, 0xaa, 0xbb, 0xcc];
        let mut decoder = RangeDecoder::new(&bytes).unwrap();
        let mut probability = Probability::new(2048).unwrap();
        for _ in 0..24 {
            assert!(!decoder.decode_context(&mut probability).unwrap());
        }
        assert!(!decoder.unread_tail().is_empty());
    }
}

use crate::CoreError;

/// MSB-first bit writer for implementation-owned payloads.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BitWriter {
    bytes: Vec<u8>,
    partial: u8,
    used: u8,
    bit_len: u64,
}

impl BitWriter {
    /// Creates an empty writer.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: Vec::new(),
            partial: 0,
            used: 0,
            bit_len: 0,
        }
    }

    /// Appends the lowest `width` bits of `value`, most-significant bit first.
    pub fn write_bits(&mut self, value: u32, width: u8) -> Result<(), CoreError> {
        if width > 32 {
            return Err(CoreError::InvalidBitWidth { width });
        }
        for shift in (0..width).rev() {
            let bit = u8::from(((value >> shift) & 1) != 0);
            self.partial = (self.partial << 1) | bit;
            self.used += 1;
            self.bit_len = self.bit_len.checked_add(1).ok_or(CoreError::SizeOverflow)?;
            if self.used == 8 {
                self.bytes.push(self.partial);
                self.partial = 0;
                self.used = 0;
            }
        }
        Ok(())
    }

    /// Returns the number of meaningful bits written.
    #[must_use]
    pub const fn bit_len(&self) -> u64 {
        self.bit_len
    }

    /// Finishes the byte sequence, padding the final byte with zero bits.
    #[must_use]
    pub fn finish(mut self) -> Vec<u8> {
        if self.used != 0 {
            self.partial <<= 8 - self.used;
            self.bytes.push(self.partial);
        }
        self.bytes
    }
}

/// Bounds-checked MSB-first bit reader.
#[derive(Clone, Copy, Debug)]
pub struct BitReader<'a> {
    bytes: &'a [u8],
    bit_offset: u64,
}

impl<'a> BitReader<'a> {
    /// Wraps a byte slice at bit offset zero.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            bit_offset: 0,
        }
    }

    /// Reads up to 32 bits, most-significant bit first.
    pub fn read_bits(&mut self, width: u8) -> Result<u32, CoreError> {
        if width > 32 {
            return Err(CoreError::InvalidBitWidth { width });
        }
        let total_bits = u64::try_from(self.bytes.len())
            .map_err(|_| CoreError::SizeOverflow)?
            .checked_mul(8)
            .ok_or(CoreError::SizeOverflow)?;
        let end = self
            .bit_offset
            .checked_add(u64::from(width))
            .ok_or(CoreError::SizeOverflow)?;
        if end > total_bits {
            return Err(CoreError::EndOfInput {
                bit_offset: self.bit_offset,
            });
        }

        let mut value = 0_u32;
        for _ in 0..width {
            let byte_index =
                usize::try_from(self.bit_offset / 8).map_err(|_| CoreError::SizeOverflow)?;
            let bit_index =
                u8::try_from(self.bit_offset % 8).map_err(|_| CoreError::SizeOverflow)?;
            let bit = (self.bytes[byte_index] >> (7 - bit_index)) & 1;
            value = (value << 1) | u32::from(bit);
            self.bit_offset += 1;
        }
        Ok(value)
    }

    /// Reports the next unread bit offset.
    #[must_use]
    pub const fn bit_offset(&self) -> u64 {
        self.bit_offset
    }
}

#[cfg(test)]
mod tests {
    use super::{BitReader, BitWriter};
    use crate::CoreError;

    #[test]
    fn bit_round_trip_crosses_byte_boundaries() {
        let mut writer = BitWriter::new();
        writer.write_bits(0b101, 3).unwrap();
        writer.write_bits(0b11_0010, 6).unwrap();
        assert_eq!(writer.bit_len(), 9);
        let bytes = writer.finish();
        assert_eq!(bytes, [0b1011_1001, 0]);

        let mut reader = BitReader::new(&bytes);
        assert_eq!(reader.read_bits(3).unwrap(), 0b101);
        assert_eq!(reader.read_bits(6).unwrap(), 0b11_0010);
        assert_eq!(reader.bit_offset(), 9);
    }

    #[test]
    fn bit_reader_reports_the_failing_offset() {
        let mut reader = BitReader::new(&[0]);
        assert_eq!(reader.read_bits(8).unwrap(), 0);
        assert_eq!(
            reader.read_bits(1),
            Err(CoreError::EndOfInput { bit_offset: 8 })
        );
    }

    #[test]
    fn widths_above_word_size_are_rejected() {
        let mut writer = BitWriter::new();
        assert_eq!(
            writer.write_bits(0, 33),
            Err(CoreError::InvalidBitWidth { width: 33 })
        );
    }
}

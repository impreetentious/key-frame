use crate::{
    BitstreamError, PlaneClass, SyntaxElement, SyntaxReader, SyntaxWriter, TransformBlockSize,
    syntax::scan::diagonal_scan,
};

const MAX_LEVEL: u32 = 32_767;

impl SyntaxWriter {
    /// Writes one transform block, including the all-zero early termination.
    pub fn write_coefficients(
        &mut self,
        plane: PlaneClass,
        size: TransformBlockSize,
        levels: &[i32],
    ) -> Result<(), BitstreamError> {
        let expected = size.side() * size.side();
        if levels.len() != expected {
            return Err(invalid("coefficient.block_length"));
        }
        if levels.iter().any(|level| level.unsigned_abs() > MAX_LEVEL) {
            return Err(invalid("coefficient.level_cap"));
        }
        let has_coeff = levels.iter().any(|&level| level != 0);
        self.record_element(SyntaxElement::HasCoeff);
        self.context(has_coeff_context(plane, size), has_coeff)?;
        if !has_coeff {
            return Ok(());
        }

        let scan = diagonal_scan(size);
        let last_scan = scan
            .iter()
            .rposition(|&index| levels[index] != 0)
            .expect("invariant: has_coeff was derived from a nonzero level");
        let last_index = scan[last_scan];
        let last_x = last_index % size.side();
        let last_y = last_index / size.side();
        self.write_position(last_x, false, size)?;
        self.write_position(last_y, true, size)?;

        let mut nonzero_count = 0_u16;
        for (scan_position, &index) in scan.iter().enumerate().take(last_scan + 1) {
            let significant = levels[index] != 0;
            if scan_position != last_scan {
                self.record_element(SyntaxElement::Sig);
                self.context(significance_context(size, nonzero_count), significant)?;
                if !significant {
                    continue;
                }
            }
            self.write_level(levels[index], size, nonzero_count)?;
            nonzero_count = nonzero_count.saturating_add(1);
        }
        Ok(())
    }

    fn write_position(
        &mut self,
        value: usize,
        vertical: bool,
        size: TransformBlockSize,
    ) -> Result<(), BitstreamError> {
        self.record_element(if vertical {
            SyntaxElement::LastY
        } else {
            SyntaxElement::LastX
        });
        let base = (if vertical { 60 } else { 44 }) + size.group() * 4;
        for bit_index in 0..size.position_bits() {
            let shift = size.position_bits() - bit_index - 1;
            let bit = ((value >> shift) & 1) != 0;
            if bit_index < 4 {
                self.context(base + u16::from(bit_index), bit)?;
            } else {
                self.bypass(bit)?;
            }
        }
        Ok(())
    }

    fn write_level(
        &mut self,
        level: i32,
        size: TransformBlockSize,
        nonzero_count: u16,
    ) -> Result<(), BitstreamError> {
        let magnitude = level.unsigned_abs();
        if magnitude == 0 || magnitude > MAX_LEVEL {
            return Err(invalid("coefficient.nonzero_level"));
        }
        let gt1 = magnitude > 1;
        self.record_element(SyntaxElement::Gt1);
        self.context(120 + size.group() * 4 + nonzero_count.min(3), gt1)?;
        if gt1 {
            let gt2 = magnitude > 2;
            self.record_element(SyntaxElement::Gt2);
            self.context(136 + size.group() * 2 + nonzero_count.min(1), gt2)?;
            if gt2 {
                self.write_unsigned_bypass(magnitude - 3)?;
            }
        }
        self.record_element(SyntaxElement::NonzeroSign);
        self.bypass(level < 0)
    }

    fn write_unsigned_bypass(&mut self, value: u32) -> Result<(), BitstreamError> {
        self.record_element(SyntaxElement::MagnitudeRemainder);
        let code_number = value + 1;
        let prefix = 31 - code_number.leading_zeros();
        for _ in 0..prefix {
            self.bypass(false)?;
        }
        self.bypass(true)?;
        for shift in (0..prefix).rev() {
            self.bypass(((code_number >> shift) & 1) != 0)?;
        }
        Ok(())
    }
}

impl SyntaxReader<'_> {
    /// Reads one transform block into raster coefficient order.
    pub fn read_coefficients(
        &mut self,
        plane: PlaneClass,
        size: TransformBlockSize,
    ) -> Result<Vec<i32>, BitstreamError> {
        let mut levels = vec![0_i32; size.side() * size.side()];
        self.record_element(SyntaxElement::HasCoeff);
        if !self.context(has_coeff_context(plane, size))? {
            return Ok(levels);
        }
        let last_x = self.read_position(false, size)?;
        let last_y = self.read_position(true, size)?;
        if last_x >= size.side() || last_y >= size.side() {
            return Err(invalid("coefficient.last_position"));
        }
        let last_index = last_y * size.side() + last_x;
        let scan = diagonal_scan(size);
        let last_scan = scan
            .iter()
            .position(|&index| index == last_index)
            .ok_or_else(|| invalid("coefficient.last_position"))?;

        let mut nonzero_count = 0_u16;
        for (scan_position, &index) in scan.iter().enumerate().take(last_scan + 1) {
            let significant = if scan_position == last_scan {
                true
            } else {
                self.record_element(SyntaxElement::Sig);
                self.context(significance_context(size, nonzero_count))?
            };
            if significant {
                levels[index] = self.read_level(size, nonzero_count)?;
                nonzero_count = nonzero_count.saturating_add(1);
            }
        }
        if nonzero_count == 0 {
            return Err(invalid("coefficient.empty_nonzero_block"));
        }
        Ok(levels)
    }

    fn read_position(
        &mut self,
        vertical: bool,
        size: TransformBlockSize,
    ) -> Result<usize, BitstreamError> {
        self.record_element(if vertical {
            SyntaxElement::LastY
        } else {
            SyntaxElement::LastX
        });
        let base = (if vertical { 60 } else { 44 }) + size.group() * 4;
        let mut value = 0_usize;
        for bit_index in 0..size.position_bits() {
            let bit = if bit_index < 4 {
                self.context(base + u16::from(bit_index))?
            } else {
                self.bypass()?
            };
            value = (value << 1) | usize::from(bit);
        }
        Ok(value)
    }

    fn read_level(
        &mut self,
        size: TransformBlockSize,
        nonzero_count: u16,
    ) -> Result<i32, BitstreamError> {
        self.record_element(SyntaxElement::Gt1);
        let gt1 = self.context(120 + size.group() * 4 + nonzero_count.min(3))?;
        let magnitude = if !gt1 {
            1
        } else {
            self.record_element(SyntaxElement::Gt2);
            if self.context(136 + size.group() * 2 + nonzero_count.min(1))? {
                self.read_unsigned_bypass()?
                    .checked_add(3)
                    .ok_or_else(|| invalid("coefficient.level_cap"))?
            } else {
                2
            }
        };
        if magnitude > MAX_LEVEL {
            return Err(invalid("coefficient.level_cap"));
        }
        let magnitude = i32::try_from(magnitude).map_err(|_| invalid("coefficient.level"))?;
        self.record_element(SyntaxElement::NonzeroSign);
        Ok(if self.bypass()? {
            -magnitude
        } else {
            magnitude
        })
    }

    fn read_unsigned_bypass(&mut self) -> Result<u32, BitstreamError> {
        self.record_element(SyntaxElement::MagnitudeRemainder);
        let mut prefix = 0_u32;
        while !self.bypass()? {
            prefix += 1;
            if prefix > 15 {
                return Err(invalid("coefficient.remainder_prefix"));
            }
        }
        let mut code_number = 1_u32 << prefix;
        for shift in (0..prefix).rev() {
            code_number |= u32::from(self.bypass()?) << shift;
        }
        Ok(code_number - 1)
    }
}

fn has_coeff_context(plane: PlaneClass, size: TransformBlockSize) -> u16 {
    36 + if plane == PlaneClass::Chroma { 4 } else { 0 } + size.group()
}

fn significance_context(size: TransformBlockSize, nonzero_count: u16) -> u16 {
    76 + size.group() * 11 + nonzero_count.min(10)
}

fn invalid(element: &'static str) -> BitstreamError {
    BitstreamError::InvalidField { offset: 0, element }
}

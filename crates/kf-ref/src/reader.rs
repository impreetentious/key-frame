use crate::ReferenceError;

pub(crate) struct ByteReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ByteReader<'a> {
    pub(crate) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub(crate) fn bytes(
        &mut self,
        length: usize,
        element: &'static str,
    ) -> Result<&'a [u8], ReferenceError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| self.error(element))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| self.error(element))?;
        self.offset = end;
        Ok(value)
    }

    pub(crate) fn u8(&mut self, element: &'static str) -> Result<u8, ReferenceError> {
        Ok(self.bytes(1, element)?[0])
    }

    pub(crate) fn u16(&mut self, element: &'static str) -> Result<u16, ReferenceError> {
        let value = self.bytes(2, element)?;
        Ok(u16::from_le_bytes([value[0], value[1]]))
    }

    pub(crate) fn u32(&mut self, element: &'static str) -> Result<u32, ReferenceError> {
        let value = self.bytes(4, element)?;
        Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
    }

    pub(crate) const fn offset(&self) -> usize {
        self.offset
    }

    fn error(&self, element: &'static str) -> ReferenceError {
        ReferenceError::new(u32::try_from(self.offset).unwrap_or(u32::MAX), element)
    }
}

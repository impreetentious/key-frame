use kf_range::{ContextBank, EncodedRange, RangeDecoder, RangeEncoder, RangeError, RangeStats};

use crate::BitstreamError;

/// Range writer plus a caller-owned transactional context snapshot.
#[derive(Clone, Debug)]
pub struct SyntaxWriter {
    range: RangeEncoder,
    contexts: ContextBank,
}

impl SyntaxWriter {
    #[must_use]
    pub fn new(contexts: ContextBank) -> Self {
        Self {
            range: RangeEncoder::new(),
            contexts,
        }
    }

    pub(crate) fn context(&mut self, id: u16, symbol: bool) -> Result<(), BitstreamError> {
        let probability = self
            .contexts
            .get_mut(id)
            .map_err(|_| syntax_error("context.id"))?;
        self.range
            .encode_context(symbol, probability)
            .map_err(|_| syntax_error("range.encode"))
    }

    pub(crate) fn bypass(&mut self, symbol: bool) -> Result<(), BitstreamError> {
        self.range
            .encode_bypass(symbol)
            .map_err(|_| syntax_error("range.bypass"))
    }

    /// Current canonical replay accounting before finalization.
    #[must_use]
    pub const fn stats(&self) -> &RangeStats {
        self.range.stats()
    }

    /// Current transactional context state for deterministic encoder search.
    #[must_use]
    pub const fn contexts(&self) -> &ContextBank {
        &self.contexts
    }

    #[must_use]
    pub fn finish(self) -> (EncodedRange, ContextBank) {
        (self.range.finish(), self.contexts)
    }
}

/// Bounded range reader plus a transactional context snapshot.
#[derive(Clone, Debug)]
pub struct SyntaxReader<'a> {
    range: RangeDecoder<'a>,
    contexts: ContextBank,
}

impl<'a> SyntaxReader<'a> {
    pub fn new(payload: &'a [u8], contexts: ContextBank) -> Result<Self, BitstreamError> {
        Ok(Self {
            range: RangeDecoder::new(payload).map_err(range_error)?,
            contexts,
        })
    }

    pub(crate) fn context(&mut self, id: u16) -> Result<bool, BitstreamError> {
        let probability = self
            .contexts
            .get_mut(id)
            .map_err(|_| syntax_error("context.id"))?;
        self.range.decode_context(probability).map_err(range_error)
    }

    pub(crate) fn bypass(&mut self) -> Result<bool, BitstreamError> {
        self.range.decode_bypass().map_err(range_error)
    }

    #[must_use]
    pub fn contexts(&self) -> &ContextBank {
        &self.contexts
    }

    #[must_use]
    pub fn into_contexts(self) -> ContextBank {
        self.contexts
    }
}

fn range_error(error: RangeError) -> BitstreamError {
    match error {
        RangeError::EndOfInput { byte_offset } => BitstreamError::UnexpectedEof {
            offset: byte_offset,
            element: "range.payload",
        },
        _ => syntax_error("range.state"),
    }
}

fn syntax_error(element: &'static str) -> BitstreamError {
    BitstreamError::InvalidField { offset: 0, element }
}

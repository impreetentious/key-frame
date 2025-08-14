use kf_range::{
    ContextBank, CoverageCounter, EncodedRange, RangeDecoder, RangeEncoder, RangeError, RangeStats,
};

use crate::{
    BitstreamError,
    syntax::coverage::{ElementCoverage, SyntaxElement},
};

/// Range writer plus a caller-owned transactional context snapshot.
#[derive(Clone, Debug)]
pub struct SyntaxWriter {
    range: RangeEncoder,
    contexts: ContextBank,
    coverage: CoverageCounter,
    elements: ElementCoverage,
}

impl SyntaxWriter {
    #[must_use]
    pub fn new(contexts: ContextBank) -> Self {
        Self {
            range: RangeEncoder::new(),
            contexts,
            coverage: CoverageCounter::new(),
            elements: ElementCoverage::new(),
        }
    }

    pub(crate) fn context(&mut self, id: u16, symbol: bool) -> Result<(), BitstreamError> {
        self.coverage
            .record(id)
            .map_err(|_| syntax_error("context.id"))?;
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

    /// Frozen ids coded on this writer.
    #[must_use]
    pub const fn coverage(&self) -> &CoverageCounter {
        &self.coverage
    }

    /// Named syntax elements emitted on this writer.
    #[must_use]
    pub const fn elements(&self) -> &ElementCoverage {
        &self.elements
    }

    pub(crate) const fn record_element(&mut self, element: SyntaxElement) {
        self.elements.record(element);
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
    coverage: CoverageCounter,
    elements: ElementCoverage,
}

impl<'a> SyntaxReader<'a> {
    pub fn new(payload: &'a [u8], contexts: ContextBank) -> Result<Self, BitstreamError> {
        Ok(Self {
            range: RangeDecoder::new(payload).map_err(range_error)?,
            contexts,
            coverage: CoverageCounter::new(),
            elements: ElementCoverage::new(),
        })
    }

    pub(crate) fn context(&mut self, id: u16) -> Result<bool, BitstreamError> {
        self.coverage
            .record(id)
            .map_err(|_| syntax_error("context.id"))?;
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

    /// Frozen ids decoded from this payload.
    #[must_use]
    pub const fn coverage(&self) -> &CoverageCounter {
        &self.coverage
    }

    /// Named syntax elements consumed from this payload.
    #[must_use]
    pub const fn elements(&self) -> &ElementCoverage {
        &self.elements
    }

    pub(crate) const fn record_element(&mut self, element: SyntaxElement) {
        self.elements.record(element);
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

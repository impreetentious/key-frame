/// Bytes emitted by one renormalization or finalization call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmissionEvent {
    /// Number of bins accepted when the bytes became observable.
    pub after_bin: u64,
    /// Number of bytes emitted at that point.
    pub bytes: u32,
    /// Whether the event occurred during finalization rather than a bin.
    pub finalization: bool,
}

/// Aggregate range-coder instrumentation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RangeStats {
    /// Context-coded and bypass bins accepted.
    pub bins: u64,
    /// Renormalization shifts performed while coding bins.
    pub renormalizations: u64,
    /// Q16 modeled entropy accumulated before each context update.
    pub modeled_entropy_q16: u64,
    /// Bytes observable so far, including finalization after finish.
    pub emitted_bytes: u64,
    /// Temporal emission buckets; delayed carry means these are not ownership.
    pub emission_events: Vec<EmissionEvent>,
}

/// Canonical byte sequence and its complete instrumentation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedRange {
    pub bytes: Vec<u8>,
    pub stats: RangeStats,
}

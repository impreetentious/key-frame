use crate::{EmissionEvent, EncodedRange, Probability, RangeError, RangeStats, modeled_cost_q16};

const RANGE_INITIAL: u32 = 0xFFFF_FFFF;
const RANGE_TOP: u32 = 1 << 24;

/// Canonical byte-oriented binary range encoder for bitstream version 1.
#[derive(Clone, Debug)]
pub struct RangeEncoder {
    low: u64,
    range: u32,
    cache: u8,
    pending: u32,
    output: Vec<u8>,
    stats: RangeStats,
}

impl Default for RangeEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl RangeEncoder {
    /// Creates the normative initial state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            low: 0,
            range: RANGE_INITIAL,
            cache: 0,
            pending: 1,
            output: Vec::new(),
            stats: RangeStats {
                bins: 0,
                renormalizations: 0,
                modeled_entropy_q16: 0,
                emitted_bytes: 0,
                emission_events: Vec::new(),
            },
        }
    }

    /// Encodes and then adapts one context-coded bin.
    pub fn encode_context(
        &mut self,
        symbol: bool,
        probability: &mut Probability,
    ) -> Result<(), RangeError> {
        self.stats.modeled_entropy_q16 = self
            .stats
            .modeled_entropy_q16
            .saturating_add(u64::from(modeled_cost_q16(symbol, *probability)?));
        self.encode_bin(symbol, probability.p1(), false);
        probability.update(symbol);
        Ok(())
    }

    /// Encodes one fixed-half bin without adapting any context.
    pub fn encode_bypass(&mut self, symbol: bool) -> Result<(), RangeError> {
        let half = Probability::new(2048)?;
        self.stats.modeled_entropy_q16 = self
            .stats
            .modeled_entropy_q16
            .saturating_add(u64::from(modeled_cost_q16(symbol, half)?));
        self.encode_bin(symbol, 2048, false);
        Ok(())
    }

    /// Finalizes with exactly five `shift_low` calls.
    #[must_use]
    pub fn finish(mut self) -> EncodedRange {
        for _ in 0..5 {
            self.shift_low(true);
        }
        debug_assert_eq!(
            self.stats.emitted_bytes,
            u64::try_from(self.output.len()).expect("invariant: output length fits u64"),
            "invariant: emission accounting conserves output bytes"
        );
        EncodedRange {
            bytes: self.output,
            stats: self.stats,
        }
    }

    fn encode_bin(&mut self, symbol: bool, p1: u16, finalization: bool) {
        let p0 = 4096_u32 - u32::from(p1);
        let bound = (self.range >> 12) * p0;
        if symbol {
            self.low += u64::from(bound);
            self.range -= bound;
        } else {
            self.range = bound;
        }
        self.stats.bins = self.stats.bins.saturating_add(1);
        while self.range < RANGE_TOP {
            self.range <<= 8;
            self.stats.renormalizations = self.stats.renormalizations.saturating_add(1);
            self.shift_low(finalization);
        }
    }

    fn shift_low(&mut self, finalization: bool) {
        let low32 = self.low as u32;
        let carry = self.low >> 32;
        assert!(
            carry <= 1,
            "invariant: range encoder delayed carry is at most one bit"
        );
        let before = self.output.len();
        if low32 < 0xFF00_0000 || carry != 0 {
            let carry_u8 = u8::try_from(carry).expect("invariant: carry was checked as one bit");
            self.output.push(self.cache.wrapping_add(carry_u8));
            for _ in 1..self.pending {
                self.output.push(0xFF_u8.wrapping_add(carry_u8));
            }
            self.cache = u8::try_from(low32 >> 24).expect("invariant: high byte fits u8");
            self.pending = 0;
        }
        self.pending = self
            .pending
            .checked_add(1)
            .expect("invariant: pending-byte count fits u32 under payload cap");
        self.low = u64::from(low32 & 0x00FF_FFFF) << 8;

        let emitted = self.output.len() - before;
        if emitted != 0 {
            let bytes = u32::try_from(emitted)
                .expect("invariant: one emission bucket fits the payload cap");
            self.stats.emitted_bytes = self.stats.emitted_bytes.saturating_add(u64::from(bytes));
            self.stats.emission_events.push(EmissionEvent {
                after_bin: self.stats.bins,
                bytes,
                finalization,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RangeEncoder;
    use crate::Probability;

    #[test]
    fn all_zero_oracle_vector_matches() {
        let mut encoder = RangeEncoder::new();
        let mut probability = Probability::new(2048).unwrap();
        for _ in 0..24 {
            encoder.encode_context(false, &mut probability).unwrap();
        }
        let encoded = encoder.finish();
        assert_eq!(encoded.bytes, [0, 0, 0, 0, 0, 0]);
        assert_eq!(probability.p1(), 949);
        assert_eq!(encoded.stats.emitted_bytes, 6);
    }

    #[test]
    fn bypass_never_adapts_caller_probability() {
        let mut encoder = RangeEncoder::new();
        let probability = Probability::new(1234).unwrap();
        for symbol in [false, true].into_iter().cycle().take(40) {
            encoder.encode_bypass(symbol).unwrap();
        }
        assert_eq!(probability.p1(), 1234);
        assert_eq!(
            encoder.finish().bytes,
            [0x00, 0x55, 0x55, 0x4d, 0x55, 0x55, 0, 0, 0]
        );
    }
}

use crate::{CONTEXT_COUNT, RangeError};

/// Records which of the 144 frozen context ids were actually coded.
///
/// The counter is a closed bitmap generated from the same id range as
/// `contexts.toml`. It fails closed on an out-of-range id rather than
/// silently extending the set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageCounter {
    seen: [bool; CONTEXT_COUNT],
}

impl CoverageCounter {
    /// An empty counter over the frozen id range.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            seen: [false; CONTEXT_COUNT],
        }
    }

    /// Marks one frozen context id as observed.
    pub fn record(&mut self, id: u16) -> Result<(), RangeError> {
        let index = usize::from(id);
        if index >= CONTEXT_COUNT {
            return Err(RangeError::InvalidContext { id });
        }
        self.seen[index] = true;
        Ok(())
    }

    /// Unions another counter into this one.
    pub fn merge(&mut self, other: &Self) {
        for (slot, flag) in self.seen.iter_mut().zip(other.seen.iter()) {
            *slot |= *flag;
        }
    }

    /// Number of distinct frozen ids observed.
    #[must_use]
    pub fn recorded(&self) -> u16 {
        u16::try_from(self.seen.iter().filter(|seen| **seen).count())
            .expect("invariant: at most 144 context ids")
    }

    /// Frozen bank size this counter was built against.
    #[must_use]
    pub const fn capacity() -> u16 {
        144
    }

    /// Whether every frozen id has been observed at least once.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.seen.iter().all(|&seen| seen)
    }

    /// Whether a specific frozen id has been observed.
    #[must_use]
    pub fn contains(&self, id: u16) -> bool {
        self.seen.get(usize::from(id)).copied().unwrap_or(false)
    }

    /// Frozen ids that have not yet been observed, in id order.
    #[must_use]
    pub fn missing(&self) -> Vec<u16> {
        self.seen
            .iter()
            .enumerate()
            .filter_map(|(index, seen)| {
                (!seen).then_some(u16::try_from(index).expect("invariant: index fits u16"))
            })
            .collect()
    }
}

impl Default for CoverageCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::CoverageCounter;
    use crate::CONTEXT_COUNT;

    #[test]
    fn empty_counter_matches_frozen_capacity() {
        let counter = CoverageCounter::new();
        assert_eq!(CoverageCounter::capacity(), 144);
        assert_eq!(usize::from(CoverageCounter::capacity()), CONTEXT_COUNT);
        assert_eq!(counter.recorded(), 0);
        assert_eq!(counter.missing().len(), CONTEXT_COUNT);
        assert!(!counter.is_complete());
    }

    #[test]
    fn out_of_range_id_is_rejected() {
        let mut counter = CoverageCounter::new();
        assert!(counter.record(144).is_err());
        assert_eq!(counter.recorded(), 0);
    }
}

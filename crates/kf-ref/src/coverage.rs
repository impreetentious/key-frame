//! What a stream coded, tallied independently of the fast decoder.
//!
//! The fast decoder keeps its own counters. This one is written against the
//! same normative element list and nothing else: if the two disagree on a
//! conformance vector, one of the two traversals is wrong, which is precisely
//! the disagreement the two-decoder rule exists to catch.

/// One named element of the payload syntax, in this decoder's own vocabulary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ReferenceElement {
    PartitionSplit,
    SkipFlag,
    InterFlag,
    IntraModeTree,
    ReferenceSelect,
    MotionDifference,
    CoefficientPresence,
    LastColumn,
    LastRow,
    Significance,
    GreaterThanOne,
    GreaterThanTwo,
    MagnitudeRemainder,
    LevelSign,
}

impl ReferenceElement {
    /// Every element this decoder knows how to read.
    pub const ALL: [Self; 14] = [
        Self::PartitionSplit,
        Self::SkipFlag,
        Self::InterFlag,
        Self::IntraModeTree,
        Self::ReferenceSelect,
        Self::MotionDifference,
        Self::CoefficientPresence,
        Self::LastColumn,
        Self::LastRow,
        Self::Significance,
        Self::GreaterThanOne,
        Self::GreaterThanTwo,
        Self::MagnitudeRemainder,
        Self::LevelSign,
    ];

    /// The normative element name from the bitstream specification.
    #[must_use]
    pub const fn spec_name(self) -> &'static str {
        match self {
            Self::PartitionSplit => "partition_tree",
            Self::SkipFlag => "skip",
            Self::InterFlag => "is_inter",
            Self::IntraModeTree => "intra_mode",
            Self::ReferenceSelect => "ref_select",
            Self::MotionDifference => "mvd",
            Self::CoefficientPresence => "has_coeff",
            Self::LastColumn => "last_x",
            Self::LastRow => "last_y",
            Self::Significance => "sig",
            Self::GreaterThanOne => "gt1",
            Self::GreaterThanTwo => "gt2",
            Self::MagnitudeRemainder => "rice_remainder",
            Self::LevelSign => "nonzero_sign",
        }
    }
}

/// Context ids and elements observed while reading one or more payloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceCoverage {
    contexts: [bool; 144],
    elements: [bool; ReferenceElement::ALL.len()],
}

impl ReferenceCoverage {
    /// An empty tally.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            contexts: [false; 144],
            elements: [false; ReferenceElement::ALL.len()],
        }
    }

    pub(crate) const fn record_context(&mut self, id: u16) {
        let index = id as usize;
        if index < self.contexts.len() {
            self.contexts[index] = true;
        }
    }

    pub(crate) const fn record_element(&mut self, element: ReferenceElement) {
        self.elements[element as usize] = true;
    }

    /// Unions another tally into this one.
    pub fn merge(&mut self, other: &Self) {
        for (slot, flag) in self.contexts.iter_mut().zip(other.contexts.iter()) {
            *slot |= *flag;
        }
        for (slot, flag) in self.elements.iter_mut().zip(other.elements.iter()) {
            *slot |= *flag;
        }
    }

    /// Whether a context id was read.
    #[must_use]
    pub fn has_context(&self, id: u16) -> bool {
        self.contexts.get(usize::from(id)).copied().unwrap_or(false)
    }

    /// Whether an element was read.
    #[must_use]
    pub const fn has_element(&self, element: ReferenceElement) -> bool {
        self.elements[element as usize]
    }

    /// Context ids read, in ascending order.
    #[must_use]
    pub fn context_ids(&self) -> Vec<u16> {
        self.contexts
            .iter()
            .enumerate()
            .filter(|(_, seen)| **seen)
            .map(|(index, _)| u16::try_from(index).expect("invariant: index below 144"))
            .collect()
    }

    /// Normative names of the elements read, in inventory order.
    #[must_use]
    pub fn element_names(&self) -> Vec<&'static str> {
        ReferenceElement::ALL
            .into_iter()
            .filter(|element| self.has_element(*element))
            .map(ReferenceElement::spec_name)
            .collect()
    }
}

impl Default for ReferenceCoverage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{ReferenceCoverage, ReferenceElement};

    #[test]
    fn spec_names_are_distinct() {
        let mut names = ReferenceElement::ALL
            .map(ReferenceElement::spec_name)
            .to_vec();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
    }

    #[test]
    fn an_out_of_range_id_is_dropped_rather_than_widening_the_bank() {
        let mut coverage = ReferenceCoverage::new();
        coverage.record_context(144);
        coverage.record_context(143);
        assert_eq!(coverage.context_ids(), vec![143]);
    }

    #[test]
    fn merge_unions_both_halves() {
        let mut left = ReferenceCoverage::new();
        left.record_context(0);
        left.record_element(ReferenceElement::SkipFlag);
        let mut right = ReferenceCoverage::new();
        right.record_context(120);
        right.record_element(ReferenceElement::LevelSign);
        left.merge(&right);
        assert_eq!(left.context_ids(), vec![0, 120]);
        assert_eq!(left.element_names(), vec!["skip", "nonzero_sign"]);
    }
}

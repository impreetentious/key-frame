//! Which named syntax elements a payload actually coded.
//!
//! The frozen context bitmap in `kf-range` answers a narrower question: which
//! of the 144 context ids were touched. That misses every element whose bins
//! are bypass-coded, because a bypass bin has no context id to record — the
//! coefficient remainder and the coefficient sign are both invisible to it.
//! Conformance coverage is stated in terms of *elements*, so it needs its own
//! counter, recorded at exactly the sites that code the element's bins.
//!
//! The set is closed. `SyntaxElement::ALL` is the whole normative vocabulary
//! and the coverage manifest is checked against it, so a new element cannot be
//! added to the coder without the inventory noticing.

/// One named element of the payload syntax.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SyntaxElement {
    /// A superblock partition split decision at any depth.
    PartitionTree,
    /// The P-frame skip flag.
    Skip,
    /// The P-frame inter/intra flag.
    IsInter,
    /// The intra mode tree.
    IntraMode,
    /// The reference selection flag.
    RefSelect,
    /// A motion vector difference, both components.
    Mvd,
    /// The per-transform-block coefficient presence flag.
    HasCoeff,
    /// The horizontal last-significant position.
    LastX,
    /// The vertical last-significant position.
    LastY,
    /// A significance flag below the last position.
    Sig,
    /// A greater-than-one magnitude flag.
    Gt1,
    /// A greater-than-two magnitude flag.
    Gt2,
    /// The bypass-coded magnitude remainder above two.
    RiceRemainder,
    /// The bypass-coded sign of a nonzero level.
    NonzeroSign,
}

impl SyntaxElement {
    /// Every element, in the frozen inventory order.
    pub const ALL: [Self; 14] = [
        Self::PartitionTree,
        Self::Skip,
        Self::IsInter,
        Self::IntraMode,
        Self::RefSelect,
        Self::Mvd,
        Self::HasCoeff,
        Self::LastX,
        Self::LastY,
        Self::Sig,
        Self::Gt1,
        Self::Gt2,
        Self::RiceRemainder,
        Self::NonzeroSign,
    ];

    /// The inventory name, matching `conformance/syntax-coverage.toml`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::PartitionTree => "partition_tree",
            Self::Skip => "skip",
            Self::IsInter => "is_inter",
            Self::IntraMode => "intra_mode",
            Self::RefSelect => "ref_select",
            Self::Mvd => "mvd",
            Self::HasCoeff => "has_coeff",
            Self::LastX => "last_x",
            Self::LastY => "last_y",
            Self::Sig => "sig",
            Self::Gt1 => "gt1",
            Self::Gt2 => "gt2",
            Self::RiceRemainder => "rice_remainder",
            Self::NonzeroSign => "nonzero_sign",
        }
    }

    /// The element with this inventory name, if it is one of the frozen set.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|element| element.name() == name)
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// Records which named syntax elements a writer emitted or a reader consumed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ElementCoverage {
    seen: [bool; SyntaxElement::ALL.len()],
}

impl ElementCoverage {
    /// An empty counter over the closed element set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            seen: [false; SyntaxElement::ALL.len()],
        }
    }

    /// Marks one element as coded.
    pub const fn record(&mut self, element: SyntaxElement) {
        self.seen[element.index()] = true;
    }

    /// Unions another counter into this one.
    pub fn merge(&mut self, other: &Self) {
        for (slot, flag) in self.seen.iter_mut().zip(other.seen.iter()) {
            *slot |= *flag;
        }
    }

    /// Whether one element was coded.
    #[must_use]
    pub const fn contains(&self, element: SyntaxElement) -> bool {
        self.seen[element.index()]
    }

    /// Number of distinct elements coded.
    #[must_use]
    pub fn recorded(&self) -> usize {
        self.seen.iter().filter(|seen| **seen).count()
    }

    /// Whether every element in the closed set was coded.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.seen.iter().all(|&seen| seen)
    }

    /// Elements not yet coded, in inventory order.
    #[must_use]
    pub fn missing(&self) -> Vec<SyntaxElement> {
        SyntaxElement::ALL
            .into_iter()
            .filter(|element| !self.contains(*element))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{ElementCoverage, SyntaxElement};

    #[test]
    fn names_are_unique_and_round_trip() {
        let mut names = SyntaxElement::ALL.map(SyntaxElement::name).to_vec();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
        for element in SyntaxElement::ALL {
            assert_eq!(SyntaxElement::from_name(element.name()), Some(element));
        }
        assert_eq!(SyntaxElement::from_name("residual"), None);
    }

    #[test]
    fn empty_counter_is_missing_everything() {
        let coverage = ElementCoverage::new();
        assert_eq!(coverage.recorded(), 0);
        assert_eq!(coverage.missing().len(), SyntaxElement::ALL.len());
        assert!(!coverage.is_complete());
    }

    #[test]
    fn merge_unions_and_completes() {
        let mut left = ElementCoverage::new();
        let mut right = ElementCoverage::new();
        for element in SyntaxElement::ALL {
            if element == SyntaxElement::Sig {
                right.record(element);
            } else {
                left.record(element);
            }
        }
        assert!(!left.is_complete());
        assert_eq!(left.missing(), vec![SyntaxElement::Sig]);
        left.merge(&right);
        assert!(left.is_complete());
        assert_eq!(left.recorded(), SyntaxElement::ALL.len());
        assert!(left.missing().is_empty());
    }
}

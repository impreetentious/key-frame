use std::sync::OnceLock;

use kf_spec::V1_ASSETS;

use crate::RangeError;

/// Number of stable v1 context ids.
pub const CONTEXT_COUNT: usize = 144;

/// A 12-bit adaptive probability where the value always means P(symbol=1).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Probability {
    p1: u16,
}

impl Probability {
    /// Constructs a legal open-interval probability.
    pub const fn new(p1: u16) -> Result<Self, RangeError> {
        if p1 == 0 || p1 >= 4096 {
            return Err(RangeError::InvalidProbability { p1 });
        }
        Ok(Self { p1 })
    }

    /// Returns P(symbol=1) on the 4096-point scale.
    #[must_use]
    pub const fn p1(self) -> u16 {
        self.p1
    }

    /// Adapts after one coded symbol using normative signed floor division.
    pub fn update(&mut self, symbol: bool) {
        let target = if symbol { 4096_i32 } else { 0_i32 };
        let current = i32::from(self.p1);
        let delta = (target - current).div_euclid(32);
        let updated = (current + delta).clamp(1, 4095);
        self.p1 = u16::try_from(updated).expect("invariant: adapted probability is clamped to u16");
    }
}

/// Transaction-friendly storage for the closed v1 context bank.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextBank {
    contexts: [Probability; CONTEXT_COUNT],
}

impl ContextBank {
    /// Initializes every context from the frozen `contexts.toml` bytes.
    #[must_use]
    pub fn initial() -> Self {
        Self {
            contexts: *initial_contexts(),
        }
    }

    /// Reads one context.
    pub fn get(&self, id: u16) -> Result<Probability, RangeError> {
        self.contexts
            .get(usize::from(id))
            .copied()
            .ok_or(RangeError::InvalidContext { id })
    }

    /// Mutably borrows one context for coding and adaptation.
    pub fn get_mut(&mut self, id: u16) -> Result<&mut Probability, RangeError> {
        self.contexts
            .get_mut(usize::from(id))
            .ok_or(RangeError::InvalidContext { id })
    }

    /// Returns all stable contexts in id order.
    #[must_use]
    pub const fn as_slice(&self) -> &[Probability] {
        &self.contexts
    }
}

fn initial_contexts() -> &'static [Probability; CONTEXT_COUNT] {
    static INITIALS: OnceLock<[Probability; CONTEXT_COUNT]> = OnceLock::new();
    INITIALS.get_or_init(|| {
        let asset = V1_ASSETS
            .iter()
            .find(|asset| asset.name == "contexts.toml")
            .expect("invariant: kf-spec exposes contexts.toml");
        let mut values = Vec::with_capacity(CONTEXT_COUNT);
        for line in asset.contents.lines() {
            let Some(raw) = line.strip_prefix("initial_p1 = [") else {
                continue;
            };
            let raw = raw
                .strip_suffix(']')
                .expect("invariant: contexts initial array has a closing bracket");
            for value in raw.split(',') {
                let parsed = value
                    .trim()
                    .parse::<u16>()
                    .expect("invariant: context initial is a decimal u16");
                values.push(
                    Probability::new(parsed)
                        .expect("invariant: checked specification contains legal probabilities"),
                );
            }
        }
        values
            .try_into()
            .expect("invariant: checked specification contains exactly 144 context initials")
    })
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT_COUNT, ContextBank, Probability};

    #[test]
    fn initial_bank_is_complete() {
        let bank = ContextBank::initial();
        assert_eq!(bank.as_slice().len(), CONTEXT_COUNT);
        assert!(bank.as_slice().iter().all(|context| context.p1() == 2048));
    }

    #[test]
    fn trap_adaptation_negative_rounding() {
        let mut probability = Probability::new(2050).unwrap();
        probability.update(false);
        assert_eq!(probability.p1(), 1985);
    }

    #[test]
    fn trap_probability_clamp() {
        let mut low = Probability::new(1).unwrap();
        let mut high = Probability::new(4095).unwrap();
        for _ in 0..10_000 {
            low.update(false);
            high.update(true);
        }
        assert_eq!(low.p1(), 1);
        assert_eq!(high.p1(), 4095);
    }
}

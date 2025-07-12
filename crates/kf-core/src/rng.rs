/// Deterministic xoshiro256++ generator for explicitly seeded test machinery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Xoshiro256PlusPlus {
    state: [u64; 4],
}

impl Xoshiro256PlusPlus {
    /// Creates a generator from the complete non-zero state.
    ///
    /// The all-zero state is remapped to a fixed non-zero seed because it is
    /// the generator's sole absorbing state.
    #[must_use]
    pub const fn from_state(mut state: [u64; 4]) -> Self {
        if state[0] == 0 && state[1] == 0 && state[2] == 0 && state[3] == 0 {
            state = [1, 0, 0, 0];
        }
        Self { state }
    }

    /// Produces the next 64-bit word from the reference xoshiro256++ step.
    pub fn next_u64(&mut self) -> u64 {
        let result = self.state[0]
            .wrapping_add(self.state[3])
            .rotate_left(23)
            .wrapping_add(self.state[0]);
        let shifted = self.state[1] << 17;

        self.state[2] ^= self.state[0];
        self.state[3] ^= self.state[1];
        self.state[1] ^= self.state[2];
        self.state[0] ^= self.state[3];
        self.state[2] ^= shifted;
        self.state[3] = self.state[3].rotate_left(45);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::Xoshiro256PlusPlus;

    #[test]
    fn reference_state_has_stable_words() {
        let mut rng = Xoshiro256PlusPlus::from_state([1, 2, 3, 4]);
        assert_eq!(rng.next_u64(), 41_943_041);
        assert_eq!(rng.next_u64(), 58_720_359);
        assert_eq!(rng.next_u64(), 3_588_806_011_781_223);
    }

    #[test]
    fn zero_state_is_not_absorbing() {
        let mut rng = Xoshiro256PlusPlus::from_state([0; 4]);
        assert_ne!(rng.next_u64(), 0);
    }
}

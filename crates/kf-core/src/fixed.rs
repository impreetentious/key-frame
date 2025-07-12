/// Right-shifts with symmetric round-to-nearest behavior.
///
/// Positive and negative values use the same magnitude rule, avoiding the
/// implementation-language ambiguity of shifting a signed negative value.
#[must_use]
pub fn rounded_shift_i64(value: i64, shift: u8) -> i64 {
    if shift == 0 {
        return value;
    }
    let bias = 1_i64 << (shift - 1);
    if value >= 0 {
        value.saturating_add(bias) >> shift
    } else {
        -(value.saturating_abs().saturating_add(bias) >> shift)
    }
}

/// Clamps a reconstruction sample to the 8-bit display domain.
#[must_use]
pub fn clamp_u8(value: i32) -> u8 {
    u8::try_from(value.clamp(0, 255)).expect("invariant: clamped sample fits in u8")
}

#[cfg(test)]
mod tests {
    use super::{clamp_u8, rounded_shift_i64};

    #[test]
    fn signed_rounding_is_symmetric() {
        assert_eq!(rounded_shift_i64(7, 2), 2);
        assert_eq!(rounded_shift_i64(-7, 2), -2);
        assert_eq!(rounded_shift_i64(6, 2), 2);
        assert_eq!(rounded_shift_i64(-6, 2), -2);
    }

    #[test]
    fn reconstruction_clamp_covers_both_edges() {
        assert_eq!(clamp_u8(-1), 0);
        assert_eq!(clamp_u8(256), 255);
        assert_eq!(clamp_u8(127), 127);
    }
}

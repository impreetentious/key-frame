/// Right-shifts with symmetric round-to-nearest behavior.
///
/// Positive and negative values use the same magnitude rule, avoiding the
/// implementation-language ambiguity of shifting a signed negative value.
///
/// # Panics
///
/// Debug builds panic if `shift` exceeds 62. The rounding bias is
/// `1 << (shift - 1)`, so a shift of 64 would build the bias from the sign bit
/// and round the wrong way instead of overflowing visibly. Every normative
/// shift is a small constant well inside the bound, and stating it keeps a
/// future caller from discovering the silent case.
#[must_use]
pub fn rounded_shift_i64(value: i64, shift: u8) -> i64 {
    debug_assert!(
        shift <= 62,
        "rounded_shift_i64 is defined for shifts up to 62, got {shift}"
    );
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
    fn rounding_holds_at_the_documented_shift_bound() {
        // The largest shift the codec asks for is 13, but the contract promises
        // 62, so the boundary is exercised rather than assumed. At the extremes
        // the bias addition saturates rather than wrapping, which is what keeps
        // the result finite and symmetric instead of flipping sign.
        assert_eq!(rounded_shift_i64(i64::MAX, 62), 1);
        assert_eq!(rounded_shift_i64(i64::MIN, 62), -1);
        assert_eq!(rounded_shift_i64(0, 62), 0);
        assert_eq!(rounded_shift_i64(5, 0), 5);

        // Away from saturation the rounding rule itself still holds at the bound.
        let half = 1_i64 << 61;
        assert_eq!(rounded_shift_i64(half, 62), 1);
        assert_eq!(rounded_shift_i64(-half, 62), -1);
        assert_eq!(rounded_shift_i64(half - 1, 62), 0);
        assert_eq!(rounded_shift_i64(-(half - 1), 62), 0);
    }

    #[test]
    #[should_panic(expected = "defined for shifts up to 62")]
    #[cfg(debug_assertions)]
    fn shift_beyond_the_bound_is_rejected() {
        let _ = rounded_shift_i64(1, 63);
    }

    #[test]
    fn reconstruction_clamp_covers_both_edges() {
        assert_eq!(clamp_u8(-1), 0);
        assert_eq!(clamp_u8(256), 255);
        assert_eq!(clamp_u8(127), 127);
    }
}

//! Worst-case inputs through every named arithmetic stage.
//!
//! The constitution says each stage names its container width and rounding
//! rule. The failure this guards against is an implicit widening that differs
//! between a naive and an optimized implementation: both look correct on
//! ordinary residual and disagree only where the accumulator saturates. These
//! cases drive the documented extremes through both transform directions and
//! the quantizer at every size, and assert the documented clamp rather than
//! whatever the host happens to produce.

use kf_transform::{TransformSize, dequantize, forward_transform, inverse_transform, quantize};

const SIZES: [TransformSize; 4] = [
    TransformSize::N4,
    TransformSize::N8,
    TransformSize::N16,
    TransformSize::N32,
];

/// The reconstruction domain the final inverse stage is clamped into.
const RECONSTRUCTION_MIN: i32 = -32_768;
const RECONSTRUCTION_MAX: i32 = 32_767;

/// The largest coefficient magnitude a legal stream may carry.
const COEFFICIENT_ABS_MAX: i32 = 32_767;

#[test]
fn trap_stage_width_overflow() {
    for size in SIZES {
        let count = size.side() * size.side();

        // Every position at the legal coefficient cap: the first stage must
        // accumulate in i64 and the second must land inside the documented
        // reconstruction domain rather than wrapping an i32.
        for cap in [COEFFICIENT_ABS_MAX, -COEFFICIENT_ABS_MAX] {
            let output = inverse_transform(&vec![cap; count], size).unwrap();
            assert_eq!(output.len(), count);
            assert!(
                output
                    .iter()
                    .all(|&sample| (RECONSTRUCTION_MIN..=RECONSTRUCTION_MAX).contains(&sample)),
                "inverse output left the reconstruction domain at {size:?} for cap {cap}"
            );
        }

        // Beyond the legal cap the arithmetic must still saturate rather than
        // panic or wrap. A decoder rejects these coefficients before it gets
        // here; the stage itself must not be the thing that decides.
        for extreme in [i32::MAX, i32::MIN] {
            let output = inverse_transform(&vec![extreme; count], size).unwrap();
            assert!(
                output
                    .iter()
                    .all(|&sample| (RECONSTRUCTION_MIN..=RECONSTRUCTION_MAX).contains(&sample)),
                "inverse output left the reconstruction domain at {size:?} for {extreme}"
            );
        }

        // The forward direction sees residual, whose legal extremes are the
        // full 8-bit difference range. Alternating signs put the worst case
        // into a single basis function instead of spreading it over the block.
        let alternating: Vec<i32> = (0..count)
            .map(|index| if index % 2 == 0 { 255 } else { -255 })
            .collect();
        let coefficients = forward_transform(&alternating, size).unwrap();
        assert_eq!(coefficients.len(), count);

        // Rounding is symmetric at every stage, so negating the input must
        // negate the output exactly. An asymmetric shift shows up here and
        // nowhere else until two implementations disagree on a real clip.
        let negated: Vec<i32> = alternating.iter().map(|&value| -value).collect();
        let negated_coefficients = forward_transform(&negated, size).unwrap();
        for (index, (&positive, &negative)) in coefficients
            .iter()
            .zip(negated_coefficients.iter())
            .enumerate()
        {
            assert_eq!(
                positive, -negative,
                "forward stage rounding is asymmetric at {size:?} index {index}"
            );
        }

        // The same symmetry holds on the inverse path, but only away from the
        // clamp: the reconstruction domain itself is asymmetric, so a saturated
        // sample is expected to pin at -32768 with no positive counterpart.
        // Checking every unsaturated pair keeps the rounding claim honest
        // without asserting the domain is something it is not.
        let inverse = inverse_transform(&vec![COEFFICIENT_ABS_MAX; count], size).unwrap();
        let negated_inverse = inverse_transform(&vec![-COEFFICIENT_ABS_MAX; count], size).unwrap();
        let mut compared = 0_usize;
        for (index, (&positive, &negative)) in
            inverse.iter().zip(negated_inverse.iter()).enumerate()
        {
            let at_bound = |sample: i32| [RECONSTRUCTION_MIN, RECONSTRUCTION_MAX].contains(&sample);
            if at_bound(positive) || at_bound(negative) {
                continue;
            }
            compared += 1;
            assert_eq!(
                positive, -negative,
                "inverse stage rounding is asymmetric at {size:?} index {index}"
            );
        }
        assert!(
            compared > 0,
            "inverse symmetry check is vacuous at {size:?}: every sample saturated"
        );

        // Beyond-legal coefficients must land exactly on a documented bound at
        // every size. This is the evidence that the clamp is real rather than
        // an accident of inputs that never reach it.
        for (extreme, bound) in [
            (i32::MAX, RECONSTRUCTION_MAX),
            (i32::MIN, RECONSTRUCTION_MIN),
        ] {
            let output = inverse_transform(&vec![extreme; count], size).unwrap();
            assert!(
                output.contains(&bound),
                "no sample reached the documented clamp {bound} at {size:?} for {extreme}"
            );
        }

        // The stages are pure: a second evaluation of the same input produces
        // the same bytes, so no accumulator state survives a call.
        assert_eq!(
            forward_transform(&alternating, size).unwrap(),
            coefficients,
            "forward transform is not reproducible at {size:?}"
        );
    }
}

#[test]
fn trap_stage_width_overflow_in_quantization() {
    for qp in [0_u8, 1, 31, 62, 63] {
        // Quantization narrows through i64 and clamps at the coefficient cap.
        // An i32 intermediate would wrap long before this input.
        assert_eq!(quantize(i32::MAX, qp).unwrap(), COEFFICIENT_ABS_MAX);
        assert_eq!(quantize(i32::MIN, qp).unwrap(), -COEFFICIENT_ABS_MAX);
        assert_eq!(quantize(0, qp).unwrap(), 0);

        // Dequantization of the cap stays inside i32 at every QP, and the
        // magnitude check fires before the arithmetic rather than after it.
        let high = dequantize(COEFFICIENT_ABS_MAX, qp).unwrap();
        let low = dequantize(-COEFFICIENT_ABS_MAX, qp).unwrap();
        assert_eq!(high, -low, "dequantization is asymmetric at qp {qp}");
        assert!(dequantize(COEFFICIENT_ABS_MAX + 1, qp).is_err());
        assert!(dequantize(-COEFFICIENT_ABS_MAX - 1, qp).is_err());
    }
}

//! BD-rate: the average bitrate difference between two rate–quality curves.
//!
//! The definition is pinned because BD-rate is a recipe, not a quantity, and
//! two implementations that disagree about the interpolant disagree about the
//! answer. This one fits monotone PCHIP to `ln(rate)` against quality,
//! integrates both curves analytically over the quality interval they share,
//! and reports `100 · (exp(mean difference) − 1)`.
//!
//! Monotone matters. A cubic spline through rate–quality points overshoots
//! between them, and an overshoot is a claim that some quality is cheaper than
//! any measurement said it was. PCHIP cannot overshoot, so the curve stays
//! inside the evidence.
//!
//! Refusing matters too. Fewer than four distinct finite points, or curves that
//! never overlap in quality, produce a named error rather than a number: an
//! extrapolated BD-rate is a number about a region nobody measured.

/// One measured point on a rate–quality curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RatePoint {
    /// Bits per second, or any consistent rate unit. Only ratios matter.
    pub rate: f64,
    /// The quality metric, higher being better.
    pub quality: f64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BdRateError {
    /// A curve had fewer than four distinct finite points.
    TooFewPoints { curve: &'static str, points: usize },
    /// A rate was zero or negative, so its logarithm is not a number.
    NonPositiveRate { curve: &'static str },
    /// The curves share no quality interval, so any comparison would be an
    /// extrapolation.
    NoOverlap,
}

impl core::fmt::Display for BdRateError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooFewPoints { curve, points } => write!(
                formatter,
                "the {curve} curve has {points} distinct finite point(s); four are needed"
            ),
            Self::NonPositiveRate { curve } => {
                write!(formatter, "the {curve} curve has a rate at or below zero")
            }
            Self::NoOverlap => formatter.write_str(
                "the curves share no quality interval, so a difference would be extrapolated",
            ),
        }
    }
}

impl std::error::Error for BdRateError {}

/// The average bitrate difference of `candidate` against `baseline`, as a
/// percentage. Negative means the candidate spends fewer bits for the same
/// quality.
pub fn bd_rate(baseline: &[RatePoint], candidate: &[RatePoint]) -> Result<f64, BdRateError> {
    let left = prepare(baseline, "baseline")?;
    let right = prepare(candidate, "candidate")?;

    let low = left
        .first()
        .map(|point| point.0)
        .unwrap_or_default()
        .max(right.first().map(|point| point.0).unwrap_or_default());
    let high = left
        .last()
        .map(|point| point.0)
        .unwrap_or_default()
        .min(right.last().map(|point| point.0).unwrap_or_default());
    // Written as an explicit ordering check rather than `high <= low` so that a
    // non-comparable bound refuses rather than silently passing.
    if !matches!(high.partial_cmp(&low), Some(core::cmp::Ordering::Greater)) {
        return Err(BdRateError::NoOverlap);
    }

    let difference = (integrate(&right, low, high) - integrate(&left, low, high)) / (high - low);
    Ok(100.0 * (difference.exp() - 1.0))
}

/// Sorts by quality, drops duplicates, and takes the natural logarithm of the
/// rate. Duplicate qualities are dropped rather than averaged: two different
/// rates at one quality is a measurement problem, and silently averaging it
/// away would hide it inside a plausible-looking curve.
fn prepare(points: &[RatePoint], curve: &'static str) -> Result<Vec<(f64, f64)>, BdRateError> {
    let mut prepared: Vec<(f64, f64)> = Vec::with_capacity(points.len());
    for point in points {
        if !point.rate.is_finite() || !point.quality.is_finite() {
            continue;
        }
        if point.rate <= 0.0 {
            return Err(BdRateError::NonPositiveRate { curve });
        }
        prepared.push((point.quality, point.rate.ln()));
    }
    prepared.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(core::cmp::Ordering::Equal));
    prepared.dedup_by(|a, b| a.0 == b.0);
    if prepared.len() < 4 {
        return Err(BdRateError::TooFewPoints {
            curve,
            points: prepared.len(),
        });
    }
    Ok(prepared)
}

/// PCHIP slopes: the Fritsch–Carlson rule, which is what makes the
/// interpolant monotone on every interval where the data is.
fn slopes(points: &[(f64, f64)]) -> Vec<f64> {
    let count = points.len();
    let mut secants = Vec::with_capacity(count - 1);
    let mut widths = Vec::with_capacity(count - 1);
    for window in points.windows(2) {
        let width = window[1].0 - window[0].0;
        widths.push(width);
        secants.push((window[1].1 - window[0].1) / width);
    }

    let mut derivatives = vec![0.0_f64; count];
    for index in 1..count - 1 {
        let (before, after) = (secants[index - 1], secants[index]);
        if before * after <= 0.0 {
            // A local extremum. A nonzero slope here is exactly what would
            // create an overshoot, so the curve is flattened through it.
            derivatives[index] = 0.0;
        } else {
            let (w1, w2) = (
                2.0 * widths[index] + widths[index - 1],
                widths[index] + 2.0 * widths[index - 1],
            );
            derivatives[index] = (w1 + w2) / (w1 / before + w2 / after);
        }
    }
    derivatives[0] = endpoint_slope(
        secants[0],
        secants.get(1).copied(),
        widths[0],
        widths.get(1).copied(),
    );
    let last = count - 1;
    derivatives[last] = endpoint_slope(
        secants[last - 1],
        secants.get(last.wrapping_sub(2)).copied(),
        widths[last - 1],
        widths.get(last.wrapping_sub(2)).copied(),
    );
    derivatives
}

/// The one-sided endpoint rule, clamped so an endpoint cannot introduce an
/// overshoot the interior rule just prevented.
fn endpoint_slope(near: f64, far: Option<f64>, near_width: f64, far_width: Option<f64>) -> f64 {
    let (Some(far), Some(far_width)) = (far, far_width) else {
        return near;
    };
    let estimate =
        ((2.0 * near_width + far_width) * near - near_width * far) / (near_width + far_width);
    if estimate * near <= 0.0 {
        return 0.0;
    }
    if near * far <= 0.0 && estimate.abs() > (3.0 * near).abs() {
        return 3.0 * near;
    }
    estimate
}

/// The exact integral of the cubic Hermite interpolant from `low` to `high`.
///
/// Analytic rather than sampled: a Simpson or trapezoid sum over the same
/// curve would introduce an error that depends on how many samples someone
/// chose, which is a knob nobody should have on a published number.
fn integrate(points: &[(f64, f64)], low: f64, high: f64) -> f64 {
    let derivatives = slopes(points);
    let mut total = 0.0_f64;
    for index in 0..points.len() - 1 {
        let (x0, y0) = points[index];
        let (x1, y1) = points[index + 1];
        let start = x0.max(low);
        let end = x1.min(high);
        if !matches!(end.partial_cmp(&start), Some(core::cmp::Ordering::Greater)) {
            continue;
        }
        let width = x1 - x0;
        let (d0, d1) = (derivatives[index], derivatives[index + 1]);
        // Integrate the Hermite basis in normalized coordinates and scale back.
        let to_unit = |x: f64| (x - x0) / width;
        total += width
            * (hermite_integral(to_unit(end), y0, y1, d0, d1, width)
                - hermite_integral(to_unit(start), y0, y1, d0, d1, width));
    }
    total
}

/// The antiderivative of the cubic Hermite basis at `t` in `[0, 1]`.
fn hermite_integral(t: f64, y0: f64, y1: f64, d0: f64, d1: f64, width: f64) -> f64 {
    let t2 = t * t;
    let t3 = t2 * t;
    let t4 = t3 * t;
    // h00 = 2t^3 - 3t^2 + 1, h10 = t^3 - 2t^2 + t,
    // h01 = -2t^3 + 3t^2,     h11 = t^3 - t^2
    let h00 = t4 / 2.0 - t3 + t;
    let h10 = t4 / 4.0 - 2.0 * t3 / 3.0 + t2 / 2.0;
    let h01 = -t4 / 2.0 + t3;
    let h11 = t4 / 4.0 - t3 / 3.0;
    y0 * h00 + width * d0 * h10 + y1 * h01 + width * d1 * h11
}

#[cfg(test)]
mod tests {
    use super::{BdRateError, RatePoint, bd_rate, integrate, slopes};

    fn curve(rates: [f64; 5]) -> Vec<RatePoint> {
        let qualities = [30.0, 33.0, 36.0, 39.0, 42.0];
        rates
            .iter()
            .zip(qualities)
            .map(|(rate, quality)| RatePoint {
                rate: *rate,
                quality,
            })
            .collect()
    }

    #[test]
    fn a_curve_against_itself_is_zero() {
        let points = curve([100.0, 200.0, 400.0, 800.0, 1600.0]);
        let result = bd_rate(&points, &points).unwrap();
        assert!(result.abs() < 1e-9, "{result}");
    }

    #[test]
    fn halving_every_rate_is_a_fifty_percent_saving() {
        let baseline = curve([100.0, 200.0, 400.0, 800.0, 1600.0]);
        let candidate = curve([50.0, 100.0, 200.0, 400.0, 800.0]);
        let result = bd_rate(&baseline, &candidate).unwrap();
        assert!((result + 50.0).abs() < 1e-9, "{result}");
    }

    #[test]
    fn spending_more_for_the_same_quality_is_positive() {
        let baseline = curve([100.0, 200.0, 400.0, 800.0, 1600.0]);
        let candidate = curve([120.0, 240.0, 480.0, 960.0, 1920.0]);
        let result = bd_rate(&baseline, &candidate).unwrap();
        assert!((result - 20.0).abs() < 1e-9, "{result}");
    }

    #[test]
    fn three_points_are_refused() {
        let short: Vec<RatePoint> = curve([100.0, 200.0, 400.0, 800.0, 1600.0])
            .into_iter()
            .take(3)
            .collect();
        let full = curve([100.0, 200.0, 400.0, 800.0, 1600.0]);
        assert_eq!(
            bd_rate(&short, &full),
            Err(BdRateError::TooFewPoints {
                curve: "baseline",
                points: 3
            })
        );
    }

    #[test]
    fn duplicate_qualities_do_not_count_towards_the_minimum() {
        let repeated = vec![
            RatePoint {
                rate: 100.0,
                quality: 30.0,
            },
            RatePoint {
                rate: 110.0,
                quality: 30.0,
            },
            RatePoint {
                rate: 200.0,
                quality: 33.0,
            },
            RatePoint {
                rate: 400.0,
                quality: 36.0,
            },
        ];
        let full = curve([100.0, 200.0, 400.0, 800.0, 1600.0]);
        assert!(matches!(
            bd_rate(&repeated, &full),
            Err(BdRateError::TooFewPoints { points: 3, .. })
        ));
    }

    #[test]
    fn curves_that_never_meet_in_quality_are_refused() {
        let low = curve([100.0, 200.0, 400.0, 800.0, 1600.0]);
        let high: Vec<RatePoint> = low
            .iter()
            .map(|point| RatePoint {
                rate: point.rate,
                quality: point.quality + 100.0,
            })
            .collect();
        assert_eq!(bd_rate(&low, &high), Err(BdRateError::NoOverlap));
    }

    #[test]
    fn a_rate_of_zero_is_refused_rather_than_logged() {
        let mut points = curve([100.0, 200.0, 400.0, 800.0, 1600.0]);
        points[2].rate = 0.0;
        let full = curve([100.0, 200.0, 400.0, 800.0, 1600.0]);
        assert_eq!(
            bd_rate(&points, &full),
            Err(BdRateError::NonPositiveRate { curve: "baseline" })
        );
    }

    #[test]
    fn the_interpolant_does_not_overshoot_a_flat_run() {
        // A plateau followed by a rise. A plain cubic spline dips below the
        // plateau before climbing; the monotone rule must not.
        let points = vec![(0.0, 1.0), (1.0, 1.0), (2.0, 1.0), (3.0, 5.0)];
        for (index, slope) in slopes(&points).iter().enumerate().take(3) {
            assert!(
                slope.abs() < 1e-12,
                "slope {index} is {slope}, which would dip below the plateau"
            );
        }
    }

    #[test]
    fn integrating_a_straight_line_gives_its_area() {
        // y = 2x on [0, 4] has area 16.
        let points = vec![(0.0, 0.0), (1.0, 2.0), (2.0, 4.0), (4.0, 8.0)];
        let area = integrate(&points, 0.0, 4.0);
        assert!((area - 16.0).abs() < 1e-9, "{area}");
    }

    #[test]
    fn integrating_a_sub_interval_uses_only_that_interval() {
        let points = vec![(0.0, 0.0), (1.0, 2.0), (2.0, 4.0), (4.0, 8.0)];
        // The integral of 2x from 1 to 3 is 9 - 1 = 8.
        let area = integrate(&points, 1.0, 3.0);
        assert!((area - 8.0).abs() < 1e-9, "{area}");
    }

    /// The interpolant itself, against the independent oracle.
    ///
    /// A BD-rate percentage rolls a slope table and an area into one number,
    /// and two different interpolants can land close enough there to hide a
    /// real disagreement. These check the slopes and the integral directly,
    /// which is why they reach into the private functions rather than going
    /// through `bd_rate`.
    #[test]
    fn the_interpolant_matches_the_oracle_slope_for_slope() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../bench/metric-vectors.json");
        let text = std::fs::read_to_string(&path).expect("the vector file is readable");
        let document = crate::Json::parse(&text).expect("the vector file is JSON");
        let tolerance = document
            .get("tolerance")
            .and_then(crate::Json::as_f64)
            .expect("the vectors name their tolerance");
        let numbers = |value: &crate::Json| -> Vec<f64> {
            value
                .as_array()
                .expect("an array")
                .iter()
                .map(|entry| entry.as_f64().expect("a number"))
                .collect()
        };

        let cases = document
            .get("pchip")
            .and_then(crate::Json::as_array)
            .expect("the vectors carry interpolant cases");
        assert!(cases.len() >= 3, "the interpolant vector set has shrunk");

        for case in cases {
            let label = case
                .get("name")
                .and_then(crate::Json::as_str)
                .unwrap_or("unnamed");
            let xs = numbers(case.get("x").expect("abscissae"));
            let ys = numbers(case.get("y").expect("ordinates"));
            let points: Vec<(f64, f64)> = xs.iter().copied().zip(ys.iter().copied()).collect();

            let expected = numbers(case.get("slopes").expect("slopes"));
            let measured = slopes(&points);
            assert_eq!(measured.len(), expected.len(), "{label}");
            for (index, (got, want)) in measured.iter().zip(&expected).enumerate() {
                assert!(
                    (got - want).abs() <= tolerance * got.abs().max(want.abs()).max(1.0),
                    "{label}: slope {index} is {got} against the oracle's {want}"
                );
            }

            let low = case
                .get("low")
                .and_then(crate::Json::as_f64)
                .expect("a lower bound");
            let high = case
                .get("high")
                .and_then(crate::Json::as_f64)
                .expect("an upper bound");
            let want = case
                .get("integral")
                .and_then(crate::Json::as_f64)
                .expect("an integral");
            let got = integrate(&points, low, high);
            assert!(
                (got - want).abs() <= tolerance * got.abs().max(want.abs()).max(1.0),
                "{label}: the area over [{low}, {high}] is {got} against the oracle's {want}"
            );
        }
    }
}

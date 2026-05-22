//! The committed campaign receipt, checked for the things prose cannot enforce.
//!
//! Two rules in the benchmark conventions are easy to state and easy to break
//! silently, so they are asserted against the real file rather than trusted:
//!
//!   * The three average-bitrate targets are reported separately from the
//!     rate–quality curves and must never be integrated as one. They are
//!     answering a different question — the quality at an ABR point is an
//!     outcome, not a setting — so a curve fitted through them would look like
//!     a rate–distortion result and be nothing of the kind.
//!   * Every curve carries the full quality ladder. A curve missing a point
//!     still produces a BD-rate, over a shorter interval, without saying so.
//!   * Every ablation curve states its bitrate difference against the full
//!     toolset, and names the metric that difference is measured in. The
//!     projection room draws these rather than deriving them, so a curve that
//!     silently carried none would reach the page as an empty cell.

use std::{fs, path::PathBuf};

use kf_tools::{Json, RatePoint, bd_rate};

fn receipt() -> Json {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench/results/rd-campaign.json");
    let text = fs::read_to_string(&path).expect("the campaign receipt is in the repository");
    Json::parse(&text).expect("the receipt is JSON")
}

fn array<'a>(value: &'a Json, key: &str) -> &'a [Json] {
    value
        .get(key)
        .and_then(Json::as_array)
        .unwrap_or_else(|| panic!("expected an array at {key}"))
}

#[test]
fn every_curve_carries_the_whole_ladder() {
    let document = receipt();
    let ladder: Vec<f64> = array(document.get("settings").expect("settings"), "qp_ladder")
        .iter()
        .map(|value| value.as_f64().expect("a QP"))
        .collect();
    assert!(ladder.len() >= 4, "the ladder is too short to integrate");

    for curve in array(&document, "curves") {
        let label = format!(
            "{}/{}",
            curve.get("clip").and_then(Json::as_str).unwrap_or("?"),
            curve.get("toolset").and_then(Json::as_str).unwrap_or("?")
        );
        let qps: Vec<f64> = array(curve, "points")
            .iter()
            .map(|point| point.get("qp").and_then(Json::as_f64).expect("a QP"))
            .collect();
        assert_eq!(qps, ladder, "{label} does not carry the full ladder");

        for point in array(curve, "points") {
            assert_eq!(
                point.get("lossless"),
                Some(&Json::Bool(false)),
                "{label} has a lossless point, whose quality figure is unbounded"
            );
        }
    }
}

#[test]
fn the_average_bitrate_targets_cannot_be_read_as_a_curve() {
    // Structural, not advisory. An ABR entry carries `target_bps` and
    // `achieved_bps`; it deliberately has no `rate`/`quality` pair, which is
    // what every reader of a curve looks for. Someone who points the BD-rate
    // path at these gets a named refusal rather than a number.
    let document = receipt();
    let control = document
        .get("rate_control")
        .expect("the receipt reports rate-control accuracy");
    let clips = array(control, "clips");
    assert!(!clips.is_empty(), "no clip reports rate-control accuracy");

    for entry in clips {
        let clip = entry.get("clip").and_then(Json::as_str).unwrap_or("?");
        let targets = array(entry, "targets");
        assert_eq!(targets.len(), 3, "{clip} does not report three ABR targets");

        for target in targets {
            assert!(
                target.get("rate").is_none() && target.get("quality").is_none(),
                "{clip} has an ABR entry shaped like a rate-quality point, which is \
                 exactly how three of them end up integrated as a curve"
            );
            assert!(
                target.get("target_bps").and_then(Json::as_f64).is_some(),
                "{clip} has an ABR entry with no target"
            );

            // The accuracy claim, held to the measured envelope rather than to
            // the round number. The rate-control gate holds the controller to
            // ±5% at two disclosed operating points and it meets that; across
            // the wider sweep of aggressive targets the worst case is 5.3%,
            // because the quantizer moves in steps of two and at some targets
            // no achievable sequence lands inside 5%. Six is the envelope the
            // design actually delivers, and asserting the envelope is what
            // makes this a check rather than an aspiration.
            let error = target
                .get("error_percent")
                .and_then(Json::as_f64)
                .expect("an accuracy figure");
            assert!(
                error.abs() <= 6.0,
                "{clip} missed an average-bitrate target by {error:.2}%, outside the \
                 envelope recorded in docs/LIMITATIONS.md"
            );
        }

        // And the positive half: the three targets really are three different
        // operating points, so the report is not three runs at one bitrate.
        let mut rates: Vec<f64> = targets
            .iter()
            .map(|target| {
                target
                    .get("target_bps")
                    .and_then(Json::as_f64)
                    .expect("a target")
            })
            .collect();
        rates.sort_by(|a, b| a.partial_cmp(b).expect("finite targets"));
        rates.dedup();
        assert_eq!(
            rates.len(),
            3,
            "{clip} reports the same target more than once"
        );
    }
}

#[test]
fn three_ablation_points_are_refused_as_a_curve() {
    // The failure this is all guarding against, demonstrated. Three points is
    // one fewer than the integrator accepts, so even if someone did assemble
    // the ABR entries into rate-quality pairs, the answer would be a refusal.
    let three = [
        RatePoint {
            rate: 100.0,
            quality: 30.0,
        },
        RatePoint {
            rate: 200.0,
            quality: 34.0,
        },
        RatePoint {
            rate: 400.0,
            quality: 38.0,
        },
    ];
    let full = [
        RatePoint {
            rate: 100.0,
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
        RatePoint {
            rate: 800.0,
            quality: 39.0,
        },
        RatePoint {
            rate: 1600.0,
            quality: 42.0,
        },
    ];
    assert!(
        bd_rate(&three, &full).is_err(),
        "three points produced a bitrate difference"
    );
}

#[test]
fn every_ablation_curve_states_its_bitrate_difference() {
    // The page draws what the receipt says. A curve with neither field would
    // render as "not recorded", which is honest but is not a measurement, and a
    // curve with both would leave the page choosing between two answers.
    let document = receipt();
    let metric = document
        .get("metrics")
        .and_then(|metrics| metrics.get("bd_rate"))
        .and_then(Json::as_str);
    assert_eq!(
        metric,
        Some("psnr-y"),
        "the receipt does not name the metric its bitrate differences are measured in"
    );

    let mut ablations = 0_usize;
    for curve in array(&document, "curves") {
        let toolset = curve.get("toolset").and_then(Json::as_str).unwrap_or("?");
        let label = format!(
            "{}/{toolset}",
            curve.get("clip").and_then(Json::as_str).unwrap_or("?")
        );
        let percent = curve.get("bd_rate_percent").and_then(Json::as_f64);
        let refused = curve.get("bd_rate_refused").and_then(Json::as_str);

        if toolset == "full" {
            assert!(
                percent.is_none() && refused.is_none(),
                "{label} is the baseline and carries a bitrate difference against itself"
            );
            continue;
        }

        assert!(
            percent.is_some() != refused.is_some(),
            "{label} states {} bitrate differences, not one",
            usize::from(percent.is_some()) + usize::from(refused.is_some())
        );
        if let Some(value) = percent {
            assert!(value.is_finite(), "{label} records a non-finite figure");
        }
        ablations += 1;
    }
    assert!(
        ablations >= 5,
        "the receipt carries {ablations} ablation curve(s), too few to be the campaign"
    );
}

#[test]
fn every_recorded_bitrate_difference_follows_from_the_recorded_points() {
    // Two different questions get two different checks. `rd_verify` re-encodes
    // and asks whether the points still come out of the encoder, which costs
    // minutes and belongs in a gate. This asks whether the figure the receipt
    // publishes follows from the points the receipt publishes, which is
    // arithmetic over a committed file and costs nothing — so the failure where
    // a figure and its own curve part company is caught by `cargo test` rather
    // than only by the gate that re-encodes.
    let document = receipt();
    let curves = array(&document, "curves");
    let points_of = |curve: &Json| -> Vec<RatePoint> {
        array(curve, "points")
            .iter()
            .map(|point| RatePoint {
                rate: point.get("rate").and_then(Json::as_f64).expect("a rate"),
                quality: point
                    .get("quality")
                    .and_then(Json::as_f64)
                    .expect("a quality"),
            })
            .collect()
    };
    let named = |curve: &Json, key: &str| -> String {
        curve
            .get(key)
            .and_then(Json::as_str)
            .unwrap_or("?")
            .to_owned()
    };

    let mut compared = 0_usize;
    for curve in curves {
        let toolset = named(curve, "toolset");
        if toolset == "full" {
            continue;
        }
        let clip = named(curve, "clip");
        let baseline = curves
            .iter()
            .find(|other| named(other, "clip") == clip && named(other, "toolset") == "full")
            .unwrap_or_else(|| {
                panic!("{clip}/{toolset} has no baseline curve to be measured against")
            });

        let recorded = curve
            .get("bd_rate_percent")
            .and_then(Json::as_f64)
            .unwrap_or_else(|| panic!("{clip}/{toolset} records no bitrate difference"));
        let found = bd_rate(&points_of(baseline), &points_of(curve))
            .unwrap_or_else(|error| panic!("{clip}/{toolset}: {error}"));
        let tolerance = 1e-9 * recorded.abs().max(found.abs()).max(1.0);
        assert!(
            (recorded - found).abs() <= tolerance,
            "{clip}/{toolset}: the receipt records {recorded} and its own points give {found}"
        );
        compared += 1;
    }
    assert!(compared >= 5, "only {compared} figure(s) were re-derived");
}

#[test]
fn a_bitrate_difference_that_does_not_follow_from_its_points_is_caught() {
    // The check above with the answer moved, because a check that cannot fail
    // is decoration. One point of one ablation curve is shifted and the figure
    // recorded beside it is required to stop following from it.
    let document = receipt();
    let curves = array(&document, "curves");
    let points_of = |curve: &Json| -> Vec<RatePoint> {
        array(curve, "points")
            .iter()
            .map(|point| RatePoint {
                rate: point.get("rate").and_then(Json::as_f64).expect("a rate"),
                quality: point
                    .get("quality")
                    .and_then(Json::as_f64)
                    .expect("a quality"),
            })
            .collect()
    };
    fn toolset_of(curve: &Json) -> &str {
        curve.get("toolset").and_then(Json::as_str).unwrap_or("?")
    }

    let baseline = curves
        .iter()
        .find(|curve| toolset_of(curve) == "full")
        .expect("a baseline curve");
    let ablation = curves
        .iter()
        .find(|curve| {
            toolset_of(curve) != "full"
                && curve.get("clip").and_then(Json::as_str)
                    == baseline.get("clip").and_then(Json::as_str)
        })
        .expect("an ablation curve on the same clip");

    let recorded = ablation
        .get("bd_rate_percent")
        .and_then(Json::as_f64)
        .expect("a recorded figure");
    let mut moved = points_of(ablation);
    moved[0].rate *= 1.10;
    let found = bd_rate(&points_of(baseline), &moved).expect("the moved curve still integrates");
    assert!(
        (recorded - found).abs() > 1e-9 * recorded.abs().max(found.abs()).max(1.0),
        "a ten percent shift in a measured rate left the bitrate difference unchanged at {recorded}"
    );
}

#[test]
fn the_published_average_bitrate_envelope_is_the_receipt_s_own() {
    // `docs/LIMITATIONS.md` states a mean, a worst case, and a count of points
    // over five percent. Those numbers used to be a memory of a sweep run once,
    // against three encoder variants two of which no longer exist, and nothing
    // recomputed them. They are the receipt's aggregate now, and this is what
    // makes the page and the file agree: an encoder change that moves the
    // controller fails here, and the sentence has to be rewritten in the same
    // commit rather than quietly stop being true.
    const PUBLISHED_MEAN: f64 = 2.32;
    const PUBLISHED_WORST: f64 = 5.32;
    const PUBLISHED_OVER_FIVE: usize = 1;

    let document = receipt();
    let control = document
        .get("rate_control")
        .expect("the receipt reports rate-control accuracy");
    let errors: Vec<f64> = array(control, "clips")
        .iter()
        .flat_map(|clip| array(clip, "targets"))
        .map(|target| {
            target
                .get("error_percent")
                .and_then(Json::as_f64)
                .expect("an accuracy figure")
                .abs()
        })
        .collect();
    assert!(
        errors.len() >= 6,
        "the receipt carries {} operating point(s), too few to be the sweep the page describes",
        errors.len()
    );

    let mean = errors.iter().sum::<f64>() / errors.len() as f64;
    let worst = errors.iter().copied().fold(0.0_f64, f64::max);
    let over_five = errors.iter().filter(|error| **error > 5.0).count();

    // Two decimals, because that is the precision the page publishes at.
    assert!(
        (mean - PUBLISHED_MEAN).abs() < 0.005,
        "the page publishes a mean absolute error of {PUBLISHED_MEAN}%, the receipt gives {mean:.4}%"
    );
    assert!(
        (worst - PUBLISHED_WORST).abs() < 0.005,
        "the page publishes a worst case of {PUBLISHED_WORST}%, the receipt gives {worst:.4}%"
    );
    assert_eq!(
        over_five, PUBLISHED_OVER_FIVE,
        "the page publishes {PUBLISHED_OVER_FIVE} point(s) over five percent, the receipt has {over_five}"
    );
}

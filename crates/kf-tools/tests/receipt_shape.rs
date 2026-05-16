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

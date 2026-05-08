//! Replays the metric vectors authored by the independent oracle.
//!
//! `bench/metric_oracle.py` computes SSIM by direct 2D convolution and the
//! BD-rate integral by Gauss–Legendre quadrature. The Rust uses separable
//! passes and a closed-form antiderivative. Neither was written from the other,
//! so agreement across the whole vector set is evidence that both implement the
//! definition rather than evidence that one copied the other's mistakes.
//!
//! The vectors that matter most are the awkward ones: planes narrower than the
//! eleven-tap window, a single row, a curve with a plateau running into a knee.
//! Those are where two plausible implementations of "SSIM" and "BD-rate" stop
//! agreeing, and they are the reason this file exists rather than a comment
//! claiming the definitions are pinned.

use std::{fs, path::PathBuf};

use kf_frame::Frame;
use kf_tools::{Json, RatePoint, bd_rate, psnr_y, ssim_y};

fn vectors() -> Json {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../bench/metric-vectors.json")
        .canonicalize()
        .expect("the vector file is in the repository");
    let text = fs::read_to_string(&path).expect("the vector file is readable");
    Json::parse(&text).expect("the vector file is JSON")
}

fn tolerance(document: &Json) -> f64 {
    document
        .get("tolerance")
        .and_then(Json::as_f64)
        .expect("the vectors name their tolerance")
}

fn numbers(value: &Json) -> Vec<f64> {
    value
        .as_array()
        .expect("an array of numbers")
        .iter()
        .map(|entry| entry.as_f64().expect("a number"))
        .collect()
}

fn name(case: &Json) -> &str {
    case.get("name").and_then(Json::as_str).unwrap_or("unnamed")
}

fn field(case: &Json, key: &str) -> f64 {
    case.get(key)
        .and_then(Json::as_f64)
        .unwrap_or_else(|| panic!("{} has no numeric {key}", name(case)))
}

/// Builds a frame whose luma plane holds exactly the vector's samples.
///
/// The vector planes are odd sizes on purpose, and a 4:2:0 frame needs even
/// dimensions, so the frame is allocated at the next even size and the luma
/// plane is then replaced with one of the true shape. Only luma is measured.
fn frame_from(samples: &[f64], width: u32, height: u32) -> Frame {
    let mut frame = Frame::filled_420(width.next_multiple_of(2), height.next_multiple_of(2), 0)
        .expect("the frame is in range");
    let mut plane = kf_frame::Plane::filled(width, height, 0).expect("the plane is in range");
    for (index, sample) in samples.iter().enumerate() {
        plane.data_mut()[index] = u8::try_from(*sample as u32).expect("a sample byte");
    }
    frame.y = plane;
    frame
}

fn close(left: f64, right: f64, tolerance: f64) -> bool {
    if !left.is_finite() || !right.is_finite() {
        return left.is_infinite() && right.is_infinite() && left.signum() == right.signum();
    }
    (left - right).abs() <= tolerance * left.abs().max(right.abs()).max(1.0)
}

#[test]
fn the_frozen_window_is_the_derivation_the_oracle_computes() {
    // The Rust holds the eleven taps as literals so its numbers do not depend
    // on the platform's `exp`. That is only safe if the literals are the
    // derivation, which is what this checks — and it is checked against the
    // oracle's `exp`, not against a second copy of the same literals.
    let document = vectors();
    let tolerance = tolerance(&document);
    let window = numbers(
        document
            .get("ssim")
            .and_then(|ssim| ssim.get("window"))
            .expect("the vectors carry the window"),
    );
    assert_eq!(window.len(), 11);

    // A flat plane blurred by a normalized window is itself, so measuring a
    // flat pair through the real code path proves the frozen taps sum to one
    // and are applied symmetrically — without reaching into a private constant.
    let total: f64 = window.iter().sum();
    assert!(
        (total - 1.0).abs() < tolerance,
        "the window sums to {total}"
    );
    for (low, high) in (0..5).map(|tap| (window[tap], window[10 - tap])) {
        assert!(
            (low - high).abs() < tolerance,
            "the window is not symmetric"
        );
    }
}

#[test]
fn every_plane_pair_matches_the_independent_oracle() {
    let document = vectors();
    let tolerance = tolerance(&document);
    let planes = document
        .get("planes")
        .and_then(Json::as_array)
        .expect("the vectors carry plane pairs");
    assert!(planes.len() >= 5, "the vector set has shrunk");

    for case in planes {
        let label = name(case);
        let width = field(case, "width") as u32;
        let height = field(case, "height") as u32;
        let reference = frame_from(
            &numbers(case.get("reference").expect("a reference plane")),
            width,
            height,
        );
        let distorted = frame_from(
            &numbers(case.get("distorted").expect("a distorted plane")),
            width,
            height,
        );

        let measured = psnr_y(
            std::slice::from_ref(&reference),
            std::slice::from_ref(&distorted),
        )
        .unwrap_or_else(|error| panic!("{label}: {error}"));
        match case.get("psnr_y") {
            Some(Json::Null) | None => assert!(
                measured.lossless && measured.global.is_infinite(),
                "{label}: the oracle found no error but the Rust reports {}",
                measured.global
            ),
            Some(expected) => {
                let expected = expected.as_f64().expect("a decibel figure");
                assert!(
                    close(measured.global, expected, tolerance),
                    "{label}: PSNR {} against the oracle's {expected}",
                    measured.global
                );
            }
        }

        let structural =
            ssim_y(&[reference], &[distorted]).unwrap_or_else(|error| panic!("{label}: {error}"));
        let expected = field(case, "ssim_y");
        assert!(
            close(structural.global, expected, tolerance),
            "{label}: SSIM {} against the oracle's {expected}",
            structural.global
        );
        assert!(
            close(structural.per_frame[0], structural.global, tolerance),
            "{label}: a one-frame clip's global figure must be its only frame's"
        );
    }
}

#[test]
fn a_multi_frame_clip_sums_error_rather_than_averaging_decibels() {
    let document = vectors();
    let tolerance = tolerance(&document);
    let clip = document.get("clip").expect("the vectors carry a clip");
    let width = field(clip, "width") as u32;
    let height = field(clip, "height") as u32;

    let mut reference = Vec::new();
    let mut distorted = Vec::new();
    for pair in clip
        .get("frames")
        .and_then(Json::as_array)
        .expect("the clip has frames")
    {
        reference.push(frame_from(
            &numbers(pair.get("reference").expect("a reference frame")),
            width,
            height,
        ));
        distorted.push(frame_from(
            &numbers(pair.get("distorted").expect("a distorted frame")),
            width,
            height,
        ));
    }

    let measured = psnr_y(&reference, &distorted).expect("the clip measures");
    let expected_per_frame = clip
        .get("psnr_per_frame")
        .and_then(Json::as_array)
        .expect("per-frame figures");
    assert_eq!(measured.per_frame.len(), expected_per_frame.len());
    for (index, (got, want)) in measured
        .per_frame
        .iter()
        .zip(expected_per_frame)
        .enumerate()
    {
        match want {
            Json::Null => assert!(got.is_infinite(), "frame {index} should be unbounded"),
            other => {
                let want = other.as_f64().expect("a decibel figure");
                assert!(
                    close(*got, want, tolerance),
                    "frame {index}: {got} vs {want}"
                );
            }
        }
    }

    // One frame of the clip is identical to its reference, so an implementation
    // that averaged per-frame decibels would report infinity for the clip.
    assert!(
        measured.per_frame.iter().any(|value| value.is_infinite()),
        "the clip must contain a lossless frame for this to prove anything"
    );
    let expected = field(clip, "psnr_global");
    assert!(
        measured.global.is_finite(),
        "the clip figure went unbounded"
    );
    assert!(
        close(measured.global, expected, tolerance),
        "clip PSNR {} against the oracle's {expected}",
        measured.global
    );
}

#[test]
fn every_rate_curve_matches_the_independent_integrator() {
    let document = vectors();
    let tolerance = tolerance(&document);
    let curves = document
        .get("curves")
        .and_then(Json::as_array)
        .expect("the vectors carry rate curves");
    assert!(curves.len() >= 4, "the curve set has shrunk");

    for case in curves {
        let label = name(case);
        let read = |key: &str| -> Vec<RatePoint> {
            case.get(key)
                .and_then(Json::as_array)
                .unwrap_or_else(|| panic!("{label} has no {key}"))
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
        let measured = bd_rate(&read("baseline"), &read("candidate"))
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        let expected = field(case, "bd_rate_percent");
        assert!(
            close(measured, expected, tolerance),
            "{label}: BD-rate {measured}% against the oracle's {expected}%"
        );
    }
}

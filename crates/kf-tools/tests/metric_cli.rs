//! The metric tool's contract: JSON a reader can check, and a report that says
//! how to regenerate itself.

use std::{fs, process::Command};

use kf_frame::Frame;
use kf_tools::{Json, Y4mStream, encode_y4m};

fn write_clip(path: &std::path::Path, shift: i32) {
    let mut frames = Vec::new();
    for index in 0..3_u32 {
        let mut frame = Frame::filled_420(64, 64, 0).expect("the frame is in range");
        for y in 0..64 {
            for x in 0..64 {
                let base = i32::try_from((x * 3 + y * 5 + index * 7) % 256).expect("in range");
                let value = u8::try_from((base + shift).clamp(0, 255)).expect("clamped");
                frame.y.set(x, y, value).expect("the sample is in range");
            }
        }
        frames.push(frame);
    }
    let bytes = encode_y4m(&Y4mStream {
        width: 64,
        height: 64,
        fps_num: 24,
        fps_den: 1,
        frames,
    })
    .expect("the clip encodes");
    fs::write(path, bytes).expect("the clip is written");
}

fn run(arguments: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_kfmetric"))
        .args(arguments)
        .output()
        .expect("kfmetric runs");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// A directory this test alone owns.
///
/// The clock is not enough on its own. These tests run in parallel threads of
/// one process, and two that read the clock inside the same tick get the same
/// path — after which one of them deletes the other's files halfway through and
/// the failure lands on whichever test was unlucky rather than on the bug. The
/// counter is what actually makes the name unique; the clock only keeps two
/// separate runs apart.
fn temp_dir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);

    let directory = std::env::temp_dir().join(format!(
        "kf-metric-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&directory).expect("the directory is created");
    directory
}

#[test]
fn an_identical_pair_reports_lossless_rather_than_a_number() {
    let directory = temp_dir();
    let clip = directory.join("clip.y4m");
    write_clip(&clip, 0);

    let (ok, out, err) = run(&["psnr", clip.to_str().unwrap(), clip.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(out.contains("\"lossless\":true"), "{out}");
    assert!(out.contains("\"global\":null"), "{out}");
    assert!(out.contains("\"frames\":3"), "{out}");

    let (ok, out, err) = run(&["ssim", clip.to_str().unwrap(), clip.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(out.contains("\"metric\":\"ssim-y\""), "{out}");
    assert!(out.contains("\"lossless\":true"), "{out}");
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_degraded_pair_reports_a_finite_figure_and_a_config_hash() {
    let directory = temp_dir();
    let reference = directory.join("reference.y4m");
    let distorted = directory.join("distorted.y4m");
    write_clip(&reference, 0);
    write_clip(&distorted, 6);

    let (ok, out, err) = run(&[
        "psnr",
        reference.to_str().unwrap(),
        distorted.to_str().unwrap(),
    ]);
    assert!(ok, "{err}");
    assert!(out.contains("\"lossless\":false"), "{out}");
    assert!(!out.contains("\"global\":null"), "{out}");
    assert!(out.contains("\"config_sha256\":\""), "{out}");

    // The report has to reproduce, not merely describe how it might.
    let report = directory.join("report.json");
    fs::write(&report, &out).expect("the report is written");
    let (ok, repro, err) = run(&["repro", report.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(repro.contains("psnr-y reproduces"), "{repro}");
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_report_that_no_longer_matches_its_inputs_fails_to_reproduce() {
    // The whole point of a receipt is that it can be wrong. A figure edited
    // after the fact, or an input that changed under a report, has to be an
    // exit code rather than a number nobody rechecked.
    let directory = temp_dir();
    let reference = directory.join("reference.y4m");
    let distorted = directory.join("distorted.y4m");
    write_clip(&reference, 0);
    write_clip(&distorted, 6);

    let (ok, out, err) = run(&[
        "psnr",
        reference.to_str().unwrap(),
        distorted.to_str().unwrap(),
    ]);
    assert!(ok, "{err}");

    let report = directory.join("report.json");
    let tampered = out.replace("\"lossless\":false", "\"lossless\":true");
    assert_ne!(tampered, out, "the substitution has to actually apply");
    fs::write(&report, &tampered).expect("the report is written");
    let (ok, _, err) = run(&["repro", report.to_str().unwrap()]);
    assert!(!ok, "a tampered figure reproduced");
    assert!(err.contains("does not reproduce"), "{err}");
    assert!(err.contains("lossless"), "{err}");

    // An input that has moved is named by hash, since that is the only handle
    // on it that a path change does not break.
    fs::write(&report, &out).expect("the report is written");
    fs::remove_file(&distorted).expect("the input is removed");
    let (ok, _, err) = run(&["repro", report.to_str().unwrap()]);
    assert!(!ok, "a report with a missing input reproduced");
    assert!(err.contains("sha256"), "{err}");
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn the_config_hash_follows_the_clips_not_their_names() {
    let directory = temp_dir();
    let first = directory.join("first.y4m");
    let renamed = directory.join("renamed.y4m");
    write_clip(&first, 0);
    write_clip(&renamed, 0);

    let (_, a, _) = run(&["psnr", first.to_str().unwrap(), first.to_str().unwrap()]);
    let (_, b, _) = run(&["psnr", renamed.to_str().unwrap(), renamed.to_str().unwrap()]);
    let hash = |text: &str| {
        let at = text
            .find("\"config_sha256\":\"")
            .expect("a hash is present")
            + 17;
        text[at..at + 64].to_owned()
    };
    assert_eq!(hash(&a), hash(&b), "identical clips must hash identically");
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_curve_pair_reports_a_bitrate_difference() {
    let directory = temp_dir();
    let baseline = directory.join("baseline.json");
    let candidate = directory.join("candidate.json");
    fs::write(
        &baseline,
        r#"[{"rate":100,"quality":30},{"rate":200,"quality":33},{"rate":400,"quality":36},{"rate":800,"quality":39},{"rate":1600,"quality":42}]"#,
    )
    .expect("written");
    fs::write(
        &candidate,
        r#"[{"rate":50,"quality":30},{"rate":100,"quality":33},{"rate":200,"quality":36},{"rate":400,"quality":39},{"rate":800,"quality":42}]"#,
    )
    .expect("written");

    let (ok, out, err) = run(&[
        "bdrate",
        baseline.to_str().unwrap(),
        candidate.to_str().unwrap(),
    ]);
    assert!(ok, "{err}");
    let report = Json::parse(out.trim()).expect("the report is JSON");
    let percent = report
        .get("bd_rate_percent")
        .and_then(Json::as_f64)
        .expect("a bitrate difference");
    assert!((percent + 50.0).abs() < 1e-9, "{percent}");
    assert!(out.contains("\"baseline_points\":5"), "{out}");

    // The same points wrapped in an object, with the settings that produced
    // them alongside, have to read as the same curve.
    let wrapped = directory.join("wrapped.json");
    fs::write(
        &wrapped,
        r#"{"clip":"synthetic","note":"a rate of one","points":[{"rate":50,"quality":30},{"rate":100,"quality":33},{"rate":200,"quality":36},{"rate":400,"quality":39},{"rate":800,"quality":42}]}"#,
    )
    .expect("written");
    let (ok, wrapped_out, err) = run(&[
        "bdrate",
        baseline.to_str().unwrap(),
        wrapped.to_str().unwrap(),
    ]);
    assert!(ok, "{err}");
    let wrapped_report = Json::parse(wrapped_out.trim()).expect("the report is JSON");
    assert_eq!(
        wrapped_report.get("bd_rate_percent"),
        report.get("bd_rate_percent")
    );
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn too_few_points_is_a_named_refusal_not_a_number() {
    let directory = temp_dir();
    let short = directory.join("short.json");
    let full = directory.join("full.json");
    fs::write(
        &short,
        r#"[{"rate":100,"quality":30},{"rate":200,"quality":33},{"rate":400,"quality":36}]"#,
    )
    .expect("written");
    fs::write(
        &full,
        r#"[{"rate":50,"quality":30},{"rate":100,"quality":33},{"rate":200,"quality":36},{"rate":400,"quality":39}]"#,
    )
    .expect("written");

    let (ok, _, err) = run(&["bdrate", short.to_str().unwrap(), full.to_str().unwrap()]);
    assert!(!ok);
    assert!(err.contains("four are needed"), "{err}");
    fs::remove_dir_all(&directory).ok();
}

#[test]
fn an_unknown_command_is_refused_with_the_usage() {
    let (ok, _, err) = run(&["bogus"]);
    assert!(!ok);
    assert!(err.contains("unknown command bogus"), "{err}");
    assert!(err.contains("usage:"), "{err}");
}

//! Quality measurement, with every number carrying the means to check it.
//!
//! A published rate–distortion figure is only worth as much as the reader's
//! ability to regenerate it. Each report here names the metric, the exact
//! inputs by content hash, and the paths they were read from, and `repro` takes
//! that report and computes the whole thing again — it does not merely print
//! the command that would. A report that no longer reproduces is a failure with
//! an exit code, not a footnote.

use std::{env, fs, path::Path, process::ExitCode};

use kf_tools::{
    Json, RatePoint, bd_rate, decode_y4m,
    json::{number, object, string},
    psnr_y, sha256_hex, ssim_y,
};

const USAGE: &str = "\
usage:
  kfmetric psnr   <reference.y4m> <distorted.y4m>
  kfmetric ssim   <reference.y4m> <distorted.y4m>
  kfmetric bdrate <baseline.json> <candidate.json>
  kfmetric repro  <report.json>

psnr and ssim report luma-only figures per frame and for the clip, as JSON.
bdrate reads two rate-quality curve files, each a JSON array of
{\"rate\": <bits per second>, \"quality\": <metric>} objects.
repro recomputes a report from its recorded inputs and fails if any figure,
input hash, or configuration hash has moved.";

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match run(&arguments) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("kfmetric: {message}");
            eprintln!("{USAGE}");
            ExitCode::from(1)
        }
    }
}

fn run(arguments: &[String]) -> Result<String, String> {
    match arguments.first().map(String::as_str) {
        Some("psnr") => quality("psnr", arguments.get(1), arguments.get(2)),
        Some("ssim") => quality("ssim", arguments.get(1), arguments.get(2)),
        Some("bdrate") => rate_difference(arguments.get(1), arguments.get(2)),
        Some("repro") => repro(arguments.get(1)),
        Some(other) => Err(format!("unknown command {other}")),
        None => Err("no command given".to_owned()),
    }
}

fn quality(
    metric: &str,
    reference_path: Option<&String>,
    distorted_path: Option<&String>,
) -> Result<String, String> {
    let reference_path = reference_path.ok_or("no reference clip given")?;
    let distorted_path = distorted_path.ok_or("no distorted clip given")?;
    Ok(measure(metric, reference_path, distorted_path)?.to_compact())
}

/// Builds a full quality report for one clip pair.
///
/// The configuration hash covers exactly the inputs that determine the numbers:
/// the metric name and both clips by content. Hashing the file names instead
/// would agree across two different clips that happened to share a path, which
/// is the one case where agreement is worthless.
fn measure(metric: &str, reference_path: &str, distorted_path: &str) -> Result<Json, String> {
    let reference_bytes = fs::read(reference_path).map_err(|e| format!("{reference_path}: {e}"))?;
    let distorted_bytes = fs::read(distorted_path).map_err(|e| format!("{distorted_path}: {e}"))?;
    let reference = decode_y4m(&reference_bytes).map_err(|e| e.to_string())?;
    let distorted = decode_y4m(&distorted_bytes).map_err(|e| e.to_string())?;

    let result = if metric == "psnr" {
        psnr_y(&reference.frames, &distorted.frames)
    } else {
        ssim_y(&reference.frames, &distorted.frames)
    }
    .map_err(|error| error.to_string())?;

    let reference_hash = sha256_hex(&reference_bytes);
    let distorted_hash = sha256_hex(&distorted_bytes);
    let config = format!("{metric}\n{reference_hash}\n{distorted_hash}");
    Ok(object(vec![
        ("metric", string(format!("{metric}-y"))),
        ("config_sha256", string(sha256_hex(config.as_bytes()))),
        ("reference_path", string(reference_path)),
        ("distorted_path", string(distorted_path)),
        ("reference_sha256", string(reference_hash)),
        ("distorted_sha256", string(distorted_hash)),
        ("frames", number(result.per_frame.len() as f64)),
        ("lossless", Json::Bool(result.lossless)),
        ("global", number(result.global)),
        (
            "per_frame",
            Json::Array(result.per_frame.iter().copied().map(number).collect()),
        ),
    ]))
}

fn rate_difference(
    baseline_path: Option<&String>,
    candidate_path: Option<&String>,
) -> Result<String, String> {
    let baseline_path = baseline_path.ok_or("no baseline curve given")?;
    let candidate_path = candidate_path.ok_or("no candidate curve given")?;
    Ok(compare_curves(baseline_path, candidate_path)?.to_compact())
}

fn compare_curves(baseline_path: &str, candidate_path: &str) -> Result<Json, String> {
    let baseline_text =
        fs::read_to_string(baseline_path).map_err(|e| format!("{baseline_path}: {e}"))?;
    let candidate_text =
        fs::read_to_string(candidate_path).map_err(|e| format!("{candidate_path}: {e}"))?;
    let baseline = parse_curve(&baseline_text).map_err(|e| format!("{baseline_path}: {e}"))?;
    let candidate = parse_curve(&candidate_text).map_err(|e| format!("{candidate_path}: {e}"))?;
    let result = bd_rate(&baseline, &candidate).map_err(|error| error.to_string())?;
    let baseline_hash = sha256_hex(baseline_text.as_bytes());
    let candidate_hash = sha256_hex(candidate_text.as_bytes());
    let config = format!("bdrate\n{baseline_hash}\n{candidate_hash}");
    Ok(object(vec![
        ("metric", string("bd-rate")),
        ("config_sha256", string(sha256_hex(config.as_bytes()))),
        ("reference_path", string(baseline_path)),
        ("distorted_path", string(candidate_path)),
        ("reference_sha256", string(baseline_hash)),
        ("distorted_sha256", string(candidate_hash)),
        ("baseline_points", number(baseline.len() as f64)),
        ("candidate_points", number(candidate.len() as f64)),
        ("bd_rate_percent", number(result)),
    ]))
}

/// Reads a rate–quality curve file.
///
/// The file may be a bare array of points or an object with a `points` array,
/// and may carry whatever else the run that produced it wants to record —
/// settings, commit, clip name — all of which this ignores. What it will not do
/// is guess: a point missing either field is an error rather than a default.
fn parse_curve(text: &str) -> Result<Vec<RatePoint>, String> {
    let document = Json::parse(text).map_err(|error| error.to_string())?;
    let entries = document
        .get("points")
        .unwrap_or(&document)
        .as_array()
        .ok_or("expected an array of points, or an object with a points array")?;
    let mut points = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let rate = entry
            .get("rate")
            .and_then(Json::as_f64)
            .ok_or_else(|| format!("point {index} has no numeric rate"))?;
        let quality = entry
            .get("quality")
            .and_then(Json::as_f64)
            .ok_or_else(|| format!("point {index} has no numeric quality"))?;
        points.push(RatePoint { rate, quality });
    }
    if points.is_empty() {
        return Err("no rate-quality points found".to_owned());
    }
    Ok(points)
}

/// Recomputes a report and compares it, field for field, against what it says.
///
/// Reproduction is checked against the recorded paths. If the inputs have moved
/// this says so and names them by content hash, which is the only identifier
/// that survives a file being moved.
fn repro(path: Option<&String>) -> Result<String, String> {
    let path = path.ok_or("no report given")?;
    let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let report = Json::parse(&text).map_err(|error| format!("{path}: {error}"))?;
    let metric = field(&report, "metric")?;
    let reference_path = field(&report, "reference_path")?;
    let distorted_path = field(&report, "distorted_path")?;

    for (label, input) in [
        ("reference", reference_path.as_str()),
        ("distorted", distorted_path.as_str()),
    ] {
        if !Path::new(input).exists() {
            let hash = field(&report, &format!("{label}_sha256"))?;
            return Err(format!(
                "the {label} input is not at {input}; it is the file with sha256 {hash}"
            ));
        }
    }

    let fresh = match metric.as_str() {
        "psnr-y" | "ssim-y" => measure(
            metric.trim_end_matches("-y"),
            &reference_path,
            &distorted_path,
        )?,
        "bd-rate" => compare_curves(&reference_path, &distorted_path)?,
        other => return Err(format!("{other} is not a metric this tool produces")),
    };

    let moved = differences(&report, &fresh);
    if !moved.is_empty() {
        return Err(format!(
            "the report does not reproduce:\n  {}",
            moved.join("\n  ")
        ));
    }
    Ok(format!(
        "kfmetric: {metric} reproduces from {reference_path} and {distorted_path} at config_sha256 {}",
        field(&report, "config_sha256")?
    ))
}

/// Every field that moved, rather than the first, so one run finds all of them.
fn differences(recorded: &Json, fresh: &Json) -> Vec<String> {
    let (Json::Object(recorded), Json::Object(fresh)) = (recorded, fresh) else {
        return vec!["the report is not an object".to_owned()];
    };
    let mut moved = Vec::new();
    for (key, value) in fresh {
        match recorded.iter().find(|(name, _)| name == key) {
            None => moved.push(format!("{key}: absent from the report")),
            Some((_, was)) if was != value => moved.push(format!(
                "{key}: recorded {} against {}",
                was.to_compact(),
                value.to_compact()
            )),
            Some(_) => {}
        }
    }
    moved
}

fn field(report: &Json, key: &str) -> Result<String, String> {
    report
        .get(key)
        .and_then(Json::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("the report has no {key}"))
}

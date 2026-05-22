//! Re-runs a committed campaign receipt and checks that it still reproduces.
//!
//! The receipts in `bench/results/` are the numbers this repository publishes.
//! A number nobody rechecks drifts: the encoder changes, the curve does not,
//! and a chart quietly starts describing a build that no longer exists. This
//! runs the same encodes again and compares every recorded field — coded size,
//! stream hash, both metrics — and exits non-zero on the first that has moved.
//!
//! The average-bitrate half of the receipt is re-encoded the same way. It was
//! not: the receipt has always carried a coded size, an achieved bitrate, an
//! accuracy figure, both metrics, and a stream hash for every average-bitrate
//! target, and nothing recomputed any of them, while `docs/LIMITATIONS.md`
//! published the envelope those figures describe.
//!
//! It also recomputes the BD-rate of each ablation against the full toolset and
//! compares it against the figure the receipt records. That figure is the one
//! the charts page draws — the page states what the campaign measured rather
//! than re-deriving it — so it is checked here like every other published
//! number, including when what the campaign recorded was a refusal.

use std::{env, fs, process::ExitCode};

use kf_bitstream::{SEQUENCE_HEADER_SIZE, SequenceHeader};
use kf_dec::FastDecoder;
use kf_enc::{Encoder, Toolset};
use kf_frame::Frame;
use kf_ref::ReferenceDecoder;
use kf_tools::{Json, RatePoint, bd_rate, decode_y4m, psnr_y, sha256_hex, ssim_y};

const USAGE: &str = "\
usage: rd_verify --receipts <campaign.json> [options]

  --clip-dir DIR    where the source clips live, default corpus/clips
  --toolset NAME    verify only this toolset; repeatable, defaults to all
  --quick           verify the ends of each quality ladder and the most
                    aggressive average-bitrate target on each clip
  --baseline FILE   also check the shipping curves against a previous release
  --allow PERCENT   how much worse the baseline check tolerates, default 2

Every figure in the receipt is recomputed and compared. --quick samples rather
than checking every point; it is a faster smoke test and not a substitute for
the full run.";

/// How much a release may regress before the check refuses it.
///
/// Two percent is the number the benchmark rules fix. It is a real threshold
/// rather than a formality: an encoder change that costs more than that has to
/// be argued for in the commit that makes it, not absorbed silently.
const DEFAULT_ALLOWANCE: f64 = 2.0;

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("rd-verify: {message}");
            ExitCode::from(1)
        }
    }
}

fn run(arguments: Vec<String>) -> Result<String, String> {
    let receipts = value(&arguments, "--receipts").ok_or_else(|| USAGE.to_owned())?;
    let clip_dir = value(&arguments, "--clip-dir").unwrap_or_else(|| "corpus/clips".to_owned());
    let quick = arguments.iter().any(|argument| argument == "--quick");
    let baseline_path = value(&arguments, "--baseline");
    let allowance = match value(&arguments, "--allow") {
        None => DEFAULT_ALLOWANCE,
        Some(text) => text
            .parse::<f64>()
            .map_err(|_| format!("--allow wants a percentage, not {text}"))?,
    };
    let wanted = repeated(&arguments, "--toolset");

    let text = fs::read_to_string(&receipts).map_err(|error| format!("{receipts}: {error}"))?;
    let document = Json::parse(&text).map_err(|error| format!("{receipts}: {error}"))?;
    let settings = document
        .get("settings")
        .ok_or("the receipt has no settings")?;
    let frames = integer(settings, "frames")? as usize;
    let keyframe = integer(settings, "keyframe_interval")? as u16;
    let golden = u8::try_from(integer(settings, "golden_interval")?)
        .map_err(|_| "the golden interval does not fit a byte".to_owned())?;

    let curves = document
        .get("curves")
        .and_then(Json::as_array)
        .ok_or("the receipt has no curves")?;

    let mut checked = 0_usize;
    let mut differences = Vec::new();
    // Curves are grouped by clip so that each source is read and decoded once
    // rather than once per toolset.
    let mut loaded: Option<(String, Vec<Frame>, u16, u16, u16, u16)> = None;

    for curve in curves {
        let clip_name = text_field(curve, "clip")?;
        let toolset_name = text_field(curve, "toolset")?;
        if !wanted.is_empty() && !wanted.contains(&toolset_name) {
            continue;
        }
        let toolset = Toolset::named(&toolset_name)
            .ok_or_else(|| format!("{toolset_name} is not a toolset this build knows"))?;

        if loaded.as_ref().is_none_or(|(name, ..)| *name != clip_name) {
            let path = format!("{clip_dir}/{clip_name}.y4m");
            let bytes = fs::read(&path).map_err(|error| format!("{path}: {error}"))?;
            let stream = decode_y4m(&bytes).map_err(|error| format!("{path}: {error}"))?;
            let taken = stream
                .frames
                .get(..frames)
                .ok_or_else(|| format!("{path} has fewer than {frames} frames"))?
                .to_vec();

            // The receipt names its source by a hash over the path, the frame
            // count, and the file. If the clip has changed under it, everything
            // downstream is meaningless, so this stops here rather than
            // reporting a wall of moved figures.
            let identity = format!(
                "corpus/clips/{clip_name}.y4m\n{frames}\n{}",
                sha256_hex(&bytes)
            );
            let recorded = text_field(curve, "clip_sha256")?;
            let actual = sha256_hex(identity.as_bytes());
            if recorded != actual {
                return Err(format!(
                    "{clip_name}: the source has changed since the receipt was written \
                     (recorded {recorded}, found {actual})"
                ));
            }
            loaded = Some((
                clip_name.clone(),
                taken,
                stream.width,
                stream.height,
                stream.fps_num,
                stream.fps_den,
            ));
        }
        let (_, clip, width, height, fps_num, fps_den) =
            loaded.as_ref().expect("the clip was just loaded");

        let points = curve
            .get("points")
            .and_then(Json::as_array)
            .ok_or_else(|| format!("{clip_name}/{toolset_name} has no points"))?;
        let selected: Vec<usize> = if quick && points.len() > 2 {
            vec![0, points.len() - 1]
        } else {
            (0..points.len()).collect()
        };

        for index in selected {
            let point = &points[index];
            let qp = u8::try_from(integer(point, "qp")?)
                .map_err(|_| "a QP does not fit a byte".to_owned())?;
            let label = format!("{clip_name}/{toolset_name}/qp{qp}");

            let sequence =
                SequenceHeader::new(*width, *height, *fps_num, *fps_den, keyframe, golden)
                    .map_err(|error| format!("{label}: {error}"))?;
            let encoded = Encoder::new(sequence, qp)
                .map_err(|error| format!("{label}: {error}"))?
                .with_toolset(toolset)
                .encode(clip)
                .map_err(|error| format!("{label}: {error}"))?;

            let fast = FastDecoder::new()
                .decode_stream(&encoded.bytes)
                .map_err(|error| format!("{label}: the fast decoder refused it: {error}"))?;
            let reference = ReferenceDecoder::new()
                .decode_stream(&encoded.bytes)
                .map_err(|error| format!("{label}: the reference decoder refused it: {error}"))?;
            if fast != reference {
                return Err(format!("{label}: the two decoders disagree"));
            }

            let psnr = psnr_y(clip, &fast).map_err(|error| format!("{label}: {error}"))?;
            let ssim = ssim_y(clip, &fast).map_err(|error| format!("{label}: {error}"))?;
            let fps = f64::from(*fps_num) / f64::from(*fps_den);
            let rate = (encoded.bytes.len() as f64 * 8.0) / (clip.len() as f64 / fps);

            let mut moved = |field: &str, recorded: f64, fresh: f64| {
                // The metrics are floating point and the encode is not, so the
                // byte count and the hash have to match exactly while the
                // decibels are allowed a rounding error's slack.
                let tolerance = 1e-9 * recorded.abs().max(fresh.abs()).max(1.0);
                if (recorded - fresh).abs() > tolerance {
                    differences.push(format!(
                        "{label}: {field} recorded {recorded}, found {fresh}"
                    ));
                }
            };
            moved("bytes", real(point, "bytes")?, encoded.bytes.len() as f64);
            moved("rate", real(point, "rate")?, rate);
            moved("psnr_y", real(point, "psnr_y")?, psnr.global);
            moved("ssim_y", real(point, "ssim_y")?, ssim.global);

            let recorded_hash = text_field(point, "stream_sha256")?;
            let fresh_hash = sha256_hex(&encoded.bytes);
            if recorded_hash != fresh_hash {
                differences.push(format!(
                    "{label}: the coded stream is not the one the receipt names \
                     (recorded {recorded_hash}, found {fresh_hash})"
                ));
            }
            checked += 1;
        }
    }

    if !differences.is_empty() {
        return Err(format!(
            "{} figure(s) no longer reproduce:\n  {}",
            differences.len(),
            differences.join("\n  ")
        ));
    }

    let summary = bd_rates(curves)?;
    let (abr_checked, abr_summary) =
        verify_rate_control(&document, &clip_dir, quick, keyframe, golden)?;
    let regression = match baseline_path {
        None => String::new(),
        Some(path) => {
            let text = fs::read_to_string(&path).map_err(|error| format!("{path}: {error}"))?;
            let baseline = Json::parse(&text).map_err(|error| format!("{path}: {error}"))?;
            check_regression(&document, &baseline, allowance)?
        }
    };

    Ok(format!(
        "rd-verify: OK — {checked} rate-quality point(s) and {abr_checked} average-bitrate \
         point(s) reproduce exactly{}{summary}{abr_summary}{regression}",
        if quick { " (sampled)" } else { "" }
    ))
}

/// The average-bitrate half of the receipt, re-encoded and compared.
///
/// The curves were rechecked on every run and these were not: the receipt has
/// always carried a coded size, an achieved bitrate, an accuracy figure, both
/// metrics, and a stream hash for each average-bitrate target, and nothing
/// recomputed any of them. `docs/LIMITATIONS.md` publishes the envelope those
/// figures describe, so an encoder change that moved them would have left the
/// document describing a controller that no longer exists — which is the exact
/// failure the rate-distortion half of this file was written to prevent.
///
/// `--quick` verifies the lowest and highest target on each clip rather than
/// every one, for the same reason it verifies the ends of each quality ladder:
/// a gate slow enough to be skipped protects nothing, and the nightly job runs
/// the whole receipt.
fn verify_rate_control(
    document: &Json,
    clip_dir: &str,
    quick: bool,
    keyframe: u16,
    golden: u8,
) -> Result<(usize, String), String> {
    let Some(control) = document.get("rate_control") else {
        return Ok((0, String::new()));
    };
    let clips = control
        .get("clips")
        .and_then(Json::as_array)
        .ok_or_else(|| "the receipt's rate-control section has no clips".to_owned())?;

    let mut checked = 0_usize;
    let mut differences = Vec::new();
    let mut lines = String::from("\n  average-bitrate accuracy:");

    for entry in clips {
        let clip_name = text_field(entry, "clip")?;
        let frames = integer(entry, "frames")? as usize;
        let path = format!("{clip_dir}/{clip_name}.y4m");
        let bytes = fs::read(&path).map_err(|error| format!("{path}: {error}"))?;
        let stream = decode_y4m(&bytes).map_err(|error| format!("{path}: {error}"))?;
        let clip = stream
            .frames
            .get(..frames)
            .ok_or_else(|| format!("{path} has fewer than {frames} frames"))?
            .to_vec();

        // The same identity the campaign recorded: the path, the frame count
        // actually measured, and the file. The window here is longer than the
        // curves use, so this is a different hash over the same file and has to
        // be checked separately.
        let identity = format!(
            "corpus/clips/{clip_name}.y4m\n{frames}\n{}",
            sha256_hex(&bytes)
        );
        let recorded = text_field(entry, "clip_sha256")?;
        let actual = sha256_hex(identity.as_bytes());
        if recorded != actual {
            return Err(format!(
                "{clip_name}: the average-bitrate source has changed since the receipt was \
                 written (recorded {recorded}, found {actual})"
            ));
        }

        let targets = entry
            .get("targets")
            .and_then(Json::as_array)
            .ok_or_else(|| format!("{clip_name} reports no average-bitrate targets"))?;
        // `--quick` checks the most aggressive target on each clip rather than
        // every one. That is a different sample from the ladder ends the curves
        // use, and deliberately: the controller is a leaky bucket, and the
        // lowest target is where it is under the most pressure and where a
        // change to it shows first. The nightly job runs every target.
        let selected: Vec<usize> = if quick && !targets.is_empty() {
            vec![0]
        } else {
            (0..targets.len()).collect()
        };

        let fps = f64::from(stream.fps_num) / f64::from(stream.fps_den);
        let seconds = clip.len() as f64 / fps;

        for index in selected {
            let target = &targets[index];
            let target_bps = integer(target, "target_bps")?;
            let label = format!("{clip_name}/abr{target_bps}");

            let sequence = SequenceHeader::new(
                stream.width,
                stream.height,
                stream.fps_num,
                stream.fps_den,
                keyframe,
                golden,
            )
            .map_err(|error| format!("{label}: {error}"))?;
            let encoder = Encoder::with_bitrate(sequence, target_bps)
                .map_err(|error| format!("{label}: {error}"))?;
            let encoded = encoder
                .encode(&clip)
                .map_err(|error| format!("{label}: {error}"))?;

            let fast = FastDecoder::new()
                .decode_stream(&encoded.bytes)
                .map_err(|error| format!("{label}: the fast decoder refused it: {error}"))?;
            let reference = ReferenceDecoder::new()
                .decode_stream(&encoded.bytes)
                .map_err(|error| format!("{label}: the reference decoder refused it: {error}"))?;
            if fast != reference {
                return Err(format!("{label}: the two decoders disagree"));
            }

            // The controller is charged for the payload only. Counting the
            // sequence header would charge it for bytes it never emits, and the
            // campaign does not, so neither does this.
            let coded_bits =
                (encoded.bytes.len().saturating_sub(SEQUENCE_HEADER_SIZE) as f64) * 8.0;
            let achieved_bps = coded_bits / seconds;
            let error_percent =
                100.0 * (achieved_bps - f64::from(target_bps)) / f64::from(target_bps);
            let psnr = psnr_y(&clip, &fast).map_err(|error| format!("{label}: {error}"))?;
            let ssim = ssim_y(&clip, &fast).map_err(|error| format!("{label}: {error}"))?;

            let mut moved = |field: &str, recorded: f64, fresh: f64| {
                let tolerance = 1e-9 * recorded.abs().max(fresh.abs()).max(1.0);
                if (recorded - fresh).abs() > tolerance {
                    differences.push(format!(
                        "{label}: {field} recorded {recorded}, found {fresh}"
                    ));
                }
            };
            moved("bytes", real(target, "bytes")?, encoded.bytes.len() as f64);
            moved("achieved_bps", real(target, "achieved_bps")?, achieved_bps);
            moved(
                "error_percent",
                real(target, "error_percent")?,
                error_percent,
            );
            moved("psnr_y", real(target, "psnr_y")?, psnr.global);
            moved("ssim_y", real(target, "ssim_y")?, ssim.global);

            let recorded_hash = text_field(target, "stream_sha256")?;
            let fresh_hash = sha256_hex(&encoded.bytes);
            if recorded_hash != fresh_hash {
                differences.push(format!(
                    "{label}: the coded stream is not the one the receipt names \
                     (recorded {recorded_hash}, found {fresh_hash})"
                ));
            }

            lines.push_str(&format!(
                "\n    {clip_name} at {target_bps} bps: {error_percent:+.2}%"
            ));
            checked += 1;
        }
    }

    if !differences.is_empty() {
        return Err(format!(
            "{} average-bitrate figure(s) no longer reproduce:\n  {}",
            differences.len(),
            differences.join("\n  ")
        ));
    }
    Ok((checked, lines))
}

/// The shipping toolset against a previous release, on every clip they share.
///
/// Only the full toolset is checked. The ablation curves are supposed to move
/// when the encoder changes — that is what they measure — so holding them to a
/// regression threshold would fail the build for the encoder getting better.
fn check_regression(current: &Json, baseline: &Json, allowance: f64) -> Result<String, String> {
    let curves_of = |document: &Json| -> Result<Vec<(String, Vec<RatePoint>)>, String> {
        let mut found = Vec::new();
        for curve in document
            .get("curves")
            .and_then(Json::as_array)
            .ok_or_else(|| "a receipt has no curves".to_owned())?
        {
            if text_field(curve, "toolset")? == "full" {
                found.push((text_field(curve, "clip")?, rate_points(curve)?));
            }
        }
        Ok(found)
    };

    let now = curves_of(current)?;
    let before = curves_of(baseline)?;
    if before.is_empty() {
        return Err("the baseline has no shipping curve to compare against".to_owned());
    }

    let mut lines = String::from("\n  against the baseline:");
    let mut regressions = Vec::new();
    let mut compared = 0_usize;
    for (clip, points) in &now {
        let Some((_, was)) = before.iter().find(|(name, _)| name == clip) else {
            // A clip the baseline never measured is not a regression, but it is
            // worth saying so: a silent skip is how a comparison quietly stops
            // covering anything.
            lines.push_str(&format!(
                "\n    {clip}: absent from the baseline, not compared"
            ));
            continue;
        };
        let percent = bd_rate(was, points).map_err(|error| format!("{clip}: {error}"))?;
        compared += 1;
        lines.push_str(&format!("\n    {clip}: {percent:+.2}% bitrate"));
        if percent > allowance {
            regressions.push(format!(
                "{clip} costs {percent:+.2}% more bitrate than the baseline, over the {allowance:.2}% allowance"
            ));
        }
    }

    if compared == 0 {
        return Err(
            "the baseline and the receipt share no clip, so nothing was checked".to_owned(),
        );
    }
    if !regressions.is_empty() {
        return Err(format!(
            "the shipping encoder regressed:\n  {}\n  If this is intended, say so in the change that causes it and move the baseline.",
            regressions.join("\n  ")
        ));
    }
    Ok(lines)
}

/// Each ablation against the full toolset on the same clip.
///
/// A positive figure means the ablation spends more bits for the same quality,
/// which is what turning a working tool off is supposed to do.
fn bd_rates(curves: &[Json]) -> Result<String, String> {
    let mut lines = String::new();
    let mut moved = Vec::new();
    for curve in curves {
        let clip = text_field(curve, "clip")?;
        let toolset = text_field(curve, "toolset")?;
        let recorded_percent = curve.get("bd_rate_percent").and_then(Json::as_f64);
        let recorded_refusal = curve.get("bd_rate_refused").and_then(Json::as_str);

        // The baseline is not compared against itself, so it records nothing. A
        // figure sitting on it would be a comparison against something this
        // never checks, which is the quietest way for a wrong number to survive.
        if toolset == "full" {
            if recorded_percent.is_some() || recorded_refusal.is_some() {
                moved.push(format!(
                    "{clip}/{toolset}: the baseline curve records a bitrate difference \
                     against itself"
                ));
            }
            continue;
        }
        let Some(baseline) = curves.iter().find(|other| {
            other.get("clip").and_then(Json::as_str) == Some(clip.as_str())
                && other.get("toolset").and_then(Json::as_str) == Some("full")
        }) else {
            continue;
        };

        // The receipt carries this figure because the projection room draws it
        // rather than deriving it. A published number nothing re-derives is a
        // number that stops being true quietly, so it is checked here like
        // every other figure in the document — and a recorded refusal is
        // checked the same way, because "these curves no longer overlap" is a
        // claim about the encoder too.
        match bd_rate(&rate_points(baseline)?, &rate_points(curve)?) {
            Ok(percent) => {
                let Some(recorded) = recorded_percent else {
                    moved.push(match recorded_refusal {
                        Some(reason) => format!(
                            "{clip}/{toolset}: the receipt records the refusal {reason:?}, \
                             but the figure is computable and is {percent}"
                        ),
                        None => {
                            format!("{clip}/{toolset}: the receipt records no bitrate difference")
                        }
                    });
                    continue;
                };
                let tolerance = 1e-9 * recorded.abs().max(percent.abs()).max(1.0);
                if (recorded - percent).abs() > tolerance {
                    moved.push(format!(
                        "{clip}/{toolset}: bd_rate_percent recorded {recorded}, found {percent}"
                    ));
                }
                lines.push_str(&format!("\n  {clip} {toolset}: {percent:+.2}% bitrate"));
            }
            Err(error) => {
                let found = error.to_string();
                match recorded_refusal {
                    Some(reason) if reason == found => {
                        lines.push_str(&format!("\n  {clip} {toolset}: refused — {found}"));
                    }
                    Some(reason) => moved.push(format!(
                        "{clip}/{toolset}: bd_rate_refused recorded {reason:?}, found {found:?}"
                    )),
                    None => moved.push(format!(
                        "{clip}/{toolset}: the figure is no longer computable ({found}), \
                         but the receipt states one"
                    )),
                }
            }
        }
    }
    if !moved.is_empty() {
        return Err(format!(
            "{} bitrate difference(s) no longer reproduce:\n  {}",
            moved.len(),
            moved.join("\n  ")
        ));
    }
    Ok(lines)
}

fn rate_points(curve: &Json) -> Result<Vec<RatePoint>, String> {
    curve
        .get("points")
        .and_then(Json::as_array)
        .ok_or_else(|| "a curve has no points".to_owned())?
        .iter()
        .map(|point| {
            Ok(RatePoint {
                rate: real(point, "rate")?,
                quality: real(point, "quality")?,
            })
        })
        .collect()
}

fn real(value: &Json, key: &str) -> Result<f64, String> {
    value
        .get(key)
        .and_then(Json::as_f64)
        .ok_or_else(|| format!("expected a numeric {key}"))
}

fn integer(value: &Json, key: &str) -> Result<u32, String> {
    let number = real(value, key)?;
    if number < 0.0 || number.fract() != 0.0 {
        return Err(format!("{key} is {number}, which is not a whole count"));
    }
    Ok(number as u32)
}

fn text_field(value: &Json, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Json::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("expected a {key} string"))
}

fn value(arguments: &[String], flag: &str) -> Option<String> {
    repeated(arguments, flag).into_iter().next_back()
}

fn repeated(arguments: &[String], flag: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut index = 0;
    while index + 1 < arguments.len() {
        if arguments[index] == flag {
            values.push(arguments[index + 1].clone());
            index += 2;
        } else {
            index += 1;
        }
    }
    values
}

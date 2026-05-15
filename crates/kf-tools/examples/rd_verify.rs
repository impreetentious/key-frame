//! Re-runs a committed campaign receipt and checks that it still reproduces.
//!
//! The receipts in `bench/results/` are the numbers this repository publishes.
//! A number nobody rechecks drifts: the encoder changes, the curve does not,
//! and a chart quietly starts describing a build that no longer exists. This
//! runs the same encodes again and compares every recorded field — coded size,
//! stream hash, both metrics — and exits non-zero on the first that has moved.
//!
//! It also recomputes the BD-rate of each ablation against the full toolset,
//! which is the figure the charts page actually draws. That number is derived
//! rather than stored, so it cannot go stale on its own; what it can do is stop
//! being computable, which is what the refusals in the integrator are for.

use std::{env, fs, process::ExitCode};

use kf_bitstream::SequenceHeader;
use kf_dec::FastDecoder;
use kf_enc::{Encoder, Toolset};
use kf_frame::Frame;
use kf_ref::ReferenceDecoder;
use kf_tools::{Json, RatePoint, bd_rate, decode_y4m, psnr_y, sha256_hex, ssim_y};

const USAGE: &str = "\
usage: rd_verify --receipts <campaign.json> [options]

  --clip-dir DIR   where the source clips live, default corpus/clips
  --toolset NAME   verify only this toolset; repeatable, defaults to all
  --quick          verify the lowest and highest quality point of each curve

Every figure in the receipt is recomputed and compared. --quick checks the ends
of each ladder rather than all five points; it is a faster smoke test and not a
substitute for the full run.";

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
    Ok(format!(
        "rd-verify: OK — {checked} point(s) reproduce exactly{}{summary}",
        if quick { " (ladder ends only)" } else { "" }
    ))
}

/// Each ablation against the full toolset on the same clip.
///
/// A positive figure means the ablation spends more bits for the same quality,
/// which is what turning a working tool off is supposed to do.
fn bd_rates(curves: &[Json]) -> Result<String, String> {
    let mut lines = String::new();
    for curve in curves {
        let clip = text_field(curve, "clip")?;
        let toolset = text_field(curve, "toolset")?;
        if toolset == "full" {
            continue;
        }
        let Some(baseline) = curves.iter().find(|other| {
            other.get("clip").and_then(Json::as_str) == Some(clip.as_str())
                && other.get("toolset").and_then(Json::as_str) == Some("full")
        }) else {
            continue;
        };
        let percent = bd_rate(&rate_points(baseline)?, &rate_points(curve)?)
            .map_err(|error| format!("{clip}/{toolset}: {error}"))?;
        lines.push_str(&format!("\n  {clip} {toolset}: {percent:+.2}% bitrate"));
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

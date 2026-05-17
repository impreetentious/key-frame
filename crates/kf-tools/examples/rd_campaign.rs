//! Runs the rate–distortion and ablation campaign and writes its receipts.
//!
//! One clip, one toolset, five QPs is one curve. Every point on it records the
//! coded size, the decoded quality under both pinned metrics, and the exact
//! inputs that produced it, so a reader with the repository can regenerate any
//! number here rather than take it on trust.
//!
//! Two rules are load-bearing and are enforced rather than described:
//!
//!   * Quality is measured against the *decoded* stream, never against the
//!     encoder's own reconstruction. Those are required to be identical, and
//!     the campaign checks that they are — but measuring the encoder's copy
//!     would make a drift bug invisible in exactly the numbers a reader would
//!     use to judge the codec.
//!   * Every stream is decoded twice, by both decoders, and the two must agree
//!     sample for sample. A published curve is a claim about the format, and a
//!     claim about the format that only one decoder can reproduce is a claim
//!     about one program.

use std::{env, fs, path::PathBuf, process::ExitCode};

use kf_bitstream::{SEQUENCE_HEADER_SIZE, SequenceHeader};
use kf_dec::FastDecoder;
use kf_enc::{Encoder, Toolset};
use kf_frame::Frame;
use kf_ref::ReferenceDecoder;
use kf_tools::{
    Json, decode_y4m,
    json::{number, object, string},
    psnr_y, sha256_hex, ssim_y,
};

/// The quality ladder every curve is built from.
///
/// Five points is the minimum the BD-rate integrator will accept, and they span
/// the usable range rather than clustering where the codec looks best.
const QP_LADDER: [u8; 5] = [22, 27, 32, 37, 42];

/// The average-bitrate targets, as fractions of what constant-QP 32 spends.
///
/// Expressed relative to the clip rather than as absolute bitrates because a
/// figure that is easy for one clip is impossible for another, and a rate
/// controller judged against an impossible target is being judged on the
/// target. Halving, matching, and doubling the QP-32 rate asks it to hit three
/// genuinely different operating points on every clip.
const ABR_FRACTIONS: [(u32, u32); 3] = [(1, 2), (1, 1), (2, 1)];

/// Frames the average-bitrate sweep runs over.
///
/// Longer than the rate–distortion ladder uses, and deliberately so. The rate
/// controller is a single-pass leaky bucket: it starts from an initial fill and
/// converges, so a window shorter than its convergence time measures the
/// transient rather than the controller. Measured on the pinned corpus, the
/// error at 24 frames runs from 5% to 13% and settles inside 3% by 48.
///
/// Choosing the longer window because it flatters the result would be exactly
/// the kind of quiet choice the benchmark rules exist to prevent, so the count
/// is recorded in the receipt and the short-clip behaviour is named in
/// `docs/LIMITATIONS.md` rather than left for someone to rediscover.
const ABR_FRAMES: usize = 48;

const USAGE: &str = "\
usage: rd_campaign --output <receipts.json> [options]

  --clip PATH        a source clip; repeatable, defaults to the pinned corpus
  --frames N         frames to take from each clip, default 24
  --toolset NAME     an encoder toolset; repeatable, defaults to all of them
  --keyframe N       keyframe interval, default 120
  --golden N         golden refresh interval, default 16";

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("rd-campaign: {message}");
            eprintln!("{USAGE}");
            ExitCode::from(1)
        }
    }
}

fn run(arguments: Vec<String>) -> Result<String, String> {
    let output = single(&arguments, "--output").ok_or("no --output given")?;
    let frames = single(&arguments, "--frames")
        .as_deref()
        .unwrap_or("24")
        .parse::<usize>()
        .map_err(|_| "--frames must be a positive integer".to_owned())?;
    if frames == 0 {
        return Err("a campaign needs at least one frame".to_owned());
    }
    let keyframe = single(&arguments, "--keyframe")
        .as_deref()
        .unwrap_or("120")
        .parse::<u16>()
        .map_err(|_| "--keyframe must be an integer".to_owned())?;
    let golden = single(&arguments, "--golden")
        .as_deref()
        .unwrap_or("16")
        .parse::<u8>()
        .map_err(|_| "--golden must be an integer below 256".to_owned())?;

    let clips = {
        let named = repeated(&arguments, "--clip");
        if named.is_empty() {
            // The corpus manifest, not a list written here. Naming the clips in
            // this file made the campaign cover whichever clips were pinned on
            // the day it was written: a clip added to the corpus would be
            // fetched, checked for bit-exactness, and measured for rate, and
            // then quietly left out of the published curves, which say
            // "corpus" and would have meant a subset of it.
            kf_tools::pinned_clips(
                &fs::read_to_string("corpus/manifest.toml")
                    .map_err(|error| format!("corpus/manifest.toml could not be read: {error}"))?,
            )?
            .into_iter()
            .map(|clip| format!("corpus/clips/{}", clip.file))
            .collect()
        } else {
            named
        }
    };
    let toolsets = {
        let named = repeated(&arguments, "--toolset");
        if named.is_empty() {
            Toolset::names()
                .iter()
                .map(|name| (*name).to_owned())
                .collect()
        } else {
            named
        }
    };
    for name in &toolsets {
        if Toolset::named(name).is_none() {
            return Err(format!(
                "{name} is not a toolset; the names are {}",
                Toolset::names().join(", ")
            ));
        }
    }

    let mut curves = Vec::new();
    let mut rate_control = Vec::new();
    let mut points_run = 0_usize;
    for clip_path in &clips {
        let clip = load(clip_path, frames)?;
        // The sweep wants a longer window than the curves do, so it loads its
        // own prefix of the clip rather than reusing the one above.
        let abr_clip = load(clip_path, ABR_FRAMES.max(frames))?;
        rate_control.push(measure_rate_control(&abr_clip, keyframe, golden)?);
        for name in &toolsets {
            let toolset = Toolset::named(name).expect("checked above");
            let mut points = Vec::new();
            for qp in QP_LADDER {
                points.push(measure_point(&clip, toolset, qp, keyframe, golden)?);
                points_run += 1;
            }
            curves.push(object(vec![
                ("clip", string(&clip.name)),
                ("clip_sha256", string(&clip.sha256)),
                ("width", number(f64::from(clip.width))),
                ("height", number(f64::from(clip.height))),
                ("frames", number(clip.frames.len() as f64)),
                ("toolset", string(name.as_str())),
                ("points", Json::Array(points)),
            ]));
        }
    }

    // The configuration hash covers what a reader would have to match to get
    // these numbers: the ladder, the clips by content, the toolsets, and the GOP
    // settings. Two campaigns with the same hash asked the same question.
    //
    // The repository version is recorded beside it but deliberately not hashed.
    // A version bump that does not touch the encoder would otherwise invalidate
    // every receipt and force a five-minute regeneration to change one string,
    // which trains everyone to regenerate without reading. Whether the encoder
    // still produces these numbers is not a question a version string can
    // answer anyway — `rd_verify` re-encodes and compares, and that is the
    // check that means something.
    let mut config = format!(
        "key-frame-rd-campaign-v1\nframes {frames}\nkeyframe {keyframe}\ngolden {golden}\nladder {}\n",
        QP_LADDER
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    for curve in &curves {
        let clip = curve
            .get("clip_sha256")
            .and_then(Json::as_str)
            .unwrap_or("");
        let toolset = curve.get("toolset").and_then(Json::as_str).unwrap_or("");
        config.push_str(&format!("curve {clip} {toolset}\n"));
    }

    let document = object(vec![
        ("format", string("key-frame-rd-campaign-v1")),
        ("encoder_version", string(env!("CARGO_PKG_VERSION"))),
        ("config_sha256", string(sha256_hex(config.as_bytes()))),
        (
            "metrics",
            object(vec![
                ("psnr", string("psnr-y")),
                ("ssim", string("ssim-y")),
                (
                    "note",
                    string(
                        "Luma only, cropped to the displayed picture, defined in \
                         bench/metric_oracle.py and checked against it on every run.",
                    ),
                ),
            ]),
        ),
        (
            "settings",
            object(vec![
                ("frames", number(frames as f64)),
                ("keyframe_interval", number(f64::from(keyframe))),
                ("golden_interval", number(f64::from(golden))),
                (
                    "qp_ladder",
                    Json::Array(QP_LADDER.iter().map(|qp| number(f64::from(*qp))).collect()),
                ),
            ]),
        ),
        ("curves", Json::Array(curves)),
        (
            "rate_control",
            object(vec![
                (
                    "note",
                    string(
                        "Average-bitrate accuracy, reported separately from the curves \
                         above and deliberately not shaped like a rate-quality point. \
                         These entries carry target_bps and achieved_bps rather than a \
                         rate/quality pair, so the BD-rate reader refuses them by name \
                         instead of quietly integrating three points that were never a \
                         curve. Measured over a longer window than the curves, because \
                         the controller is a leaky bucket that has to converge; the \
                         frame count is recorded per clip and the short-clip transient \
                         is described in docs/LIMITATIONS.md.",
                    ),
                ),
                ("clips", Json::Array(rate_control)),
            ]),
        ),
    ]);

    let path = PathBuf::from(&output);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    fs::write(&path, document.to_pretty(2)).map_err(|error| format!("{output}: {error}"))?;
    Ok(format!(
        "rd-campaign: wrote {output} — {points_run} points across {} curves",
        document
            .get("curves")
            .and_then(Json::as_array)
            .map_or(0, <[Json]>::len)
    ))
}

struct Clip {
    name: String,
    sha256: String,
    width: u16,
    height: u16,
    fps_num: u16,
    fps_den: u16,
    frames: Vec<Frame>,
}

fn load(path: &str, frames: usize) -> Result<Clip, String> {
    let bytes = fs::read(path).map_err(|error| format!("{path}: {error}"))?;
    let stream = decode_y4m(&bytes).map_err(|error| format!("{path}: {error}"))?;
    if stream.frames.len() < frames {
        return Err(format!(
            "{path} has {} frames, fewer than the {frames} requested",
            stream.frames.len()
        ));
    }
    // The clip is identified by the bytes actually measured, not by the whole
    // file: a receipt that hashed the full clip but measured a prefix would
    // reproduce only for someone who guessed the same prefix.
    let taken: Vec<Frame> = stream.frames[..frames].to_vec();
    let mut identity = format!("{path}\n{frames}\n");
    identity.push_str(&sha256_hex(&bytes));
    Ok(Clip {
        name: PathBuf::from(path).file_stem().map_or_else(
            || path.to_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        ),
        sha256: sha256_hex(identity.as_bytes()),
        width: stream.width,
        height: stream.height,
        fps_num: stream.fps_num,
        fps_den: stream.fps_den,
        frames: taken,
    })
}

/// Encodes one point, decodes it twice, and measures the decoded result.
fn measure_point(
    clip: &Clip,
    toolset: Toolset,
    qp: u8,
    keyframe: u16,
    golden: u8,
) -> Result<Json, String> {
    let sequence = SequenceHeader::new(
        clip.width,
        clip.height,
        clip.fps_num,
        clip.fps_den,
        keyframe,
        golden,
    )
    .map_err(|error| format!("{}: {error}", clip.name))?;
    let encoder = Encoder::new(sequence, qp)
        .map_err(|error| format!("{}: {error}", clip.name))?
        .with_toolset(toolset);
    let encoded = encoder
        .encode(&clip.frames)
        .map_err(|error| format!("{} qp {qp}: {error}", clip.name))?;

    let fast = FastDecoder::new()
        .decode_stream(&encoded.bytes)
        .map_err(|error| {
            format!(
                "{} qp {qp}: the fast decoder refused it: {error}",
                clip.name
            )
        })?;
    let reference = ReferenceDecoder::new()
        .decode_stream(&encoded.bytes)
        .map_err(|error| {
            format!(
                "{} qp {qp}: the reference decoder refused it: {error}",
                clip.name
            )
        })?;
    if fast != reference {
        return Err(format!(
            "{} qp {qp}: the two decoders disagree, so no number here means anything",
            clip.name
        ));
    }
    if fast != encoded.reconstructed_frames {
        return Err(format!(
            "{} qp {qp}: the encoder's reconstruction is not what decodes",
            clip.name
        ));
    }

    let psnr = psnr_y(&clip.frames, &fast).map_err(|error| error.to_string())?;
    let ssim = ssim_y(&clip.frames, &fast).map_err(|error| error.to_string())?;

    // Bits per second at the clip's own frame rate. Only ratios enter BD-rate,
    // but a rate in a unit a reader recognizes is worth more than a byte count.
    let fps = f64::from(clip.fps_num) / f64::from(clip.fps_den);
    let seconds = clip.frames.len() as f64 / fps;
    let bitrate = (encoded.bytes.len() as f64 * 8.0) / seconds;

    Ok(object(vec![
        ("qp", number(f64::from(qp))),
        ("bytes", number(encoded.bytes.len() as f64)),
        ("rate", number(bitrate)),
        ("quality", number(psnr.global)),
        ("psnr_y", number(psnr.global)),
        ("ssim_y", number(ssim.global)),
        ("lossless", Json::Bool(psnr.lossless)),
        ("stream_sha256", string(sha256_hex(&encoded.bytes))),
    ]))
}

/// Average-bitrate accuracy for one clip, at three targets.
///
/// Reported separately from the rate–quality curves and shaped so it cannot be
/// mistaken for them. A rate controller aiming at a bitrate and a search aiming
/// at a quantizer are answering different questions; three ABR points fitted as
/// a curve would look like a rate–distortion result and would be nothing of the
/// kind, because the quality at each point is an outcome rather than a setting.
fn measure_rate_control(clip: &Clip, keyframe: u16, golden: u8) -> Result<Json, String> {
    let sequence = SequenceHeader::new(
        clip.width,
        clip.height,
        clip.fps_num,
        clip.fps_den,
        keyframe,
        golden,
    )
    .map_err(|error| format!("{}: {error}", clip.name))?;

    // The reference point the targets are scaled from: what constant-QP 32
    // spends on this clip, in bits per second.
    let anchor = Encoder::new(sequence, 32)
        .map_err(|error| format!("{}: {error}", clip.name))?
        .encode(&clip.frames)
        .map_err(|error| format!("{}: {error}", clip.name))?;
    let fps = f64::from(clip.fps_num) / f64::from(clip.fps_den);
    let seconds = clip.frames.len() as f64 / fps;
    let anchor_bps = (anchor.bytes.len() as f64 * 8.0) / seconds;

    let mut entries = Vec::new();
    for (numerator, denominator) in ABR_FRACTIONS {
        let target = (anchor_bps * f64::from(numerator) / f64::from(denominator)).round();
        let target_bps = u32::try_from(target as i64)
            .map_err(|_| format!("{}: the ABR target does not fit", clip.name))?;

        let encoder = Encoder::with_bitrate(sequence, target_bps)
            .map_err(|error| format!("{} abr {target_bps}: {error}", clip.name))?;
        let encoded = encoder
            .encode(&clip.frames)
            .map_err(|error| format!("{} abr {target_bps}: {error}", clip.name))?;

        // Determinism is part of the claim: an average-bitrate encoder that
        // wandered between runs would make the accuracy figure meaningless.
        if encoder
            .encode(&clip.frames)
            .map_err(|error| error.to_string())?
            .bytes
            != encoded.bytes
        {
            return Err(format!(
                "{} abr {target_bps}: the encode is not deterministic",
                clip.name
            ));
        }

        let fast = FastDecoder::new()
            .decode_stream(&encoded.bytes)
            .map_err(|error| format!("{} abr {target_bps}: {error}", clip.name))?;
        let reference = ReferenceDecoder::new()
            .decode_stream(&encoded.bytes)
            .map_err(|error| format!("{} abr {target_bps}: {error}", clip.name))?;
        if fast != reference {
            return Err(format!(
                "{} abr {target_bps}: the two decoders disagree",
                clip.name
            ));
        }

        // The payload, excluding the sequence header, against the budget the
        // controller was actually given. Counting the header would charge the
        // controller for bytes it never sees.
        let coded_bits = (encoded.bytes.len().saturating_sub(SEQUENCE_HEADER_SIZE) as f64) * 8.0;
        let achieved_bps = coded_bits / seconds;
        let error_percent = 100.0 * (achieved_bps - f64::from(target_bps)) / f64::from(target_bps);

        let psnr = psnr_y(&clip.frames, &fast).map_err(|error| error.to_string())?;
        let ssim = ssim_y(&clip.frames, &fast).map_err(|error| error.to_string())?;

        entries.push(object(vec![
            ("target_bps", number(f64::from(target_bps))),
            ("achieved_bps", number(achieved_bps)),
            ("error_percent", number(error_percent)),
            ("bytes", number(encoded.bytes.len() as f64)),
            ("psnr_y", number(psnr.global)),
            ("ssim_y", number(ssim.global)),
            ("stream_sha256", string(sha256_hex(&encoded.bytes))),
        ]));
    }

    Ok(object(vec![
        ("clip", string(&clip.name)),
        ("clip_sha256", string(&clip.sha256)),
        ("frames", number(clip.frames.len() as f64)),
        ("anchor_qp", number(32.0)),
        ("targets", Json::Array(entries)),
    ]))
}

fn single(arguments: &[String], flag: &str) -> Option<String> {
    repeated(arguments, flag).into_iter().next_back()
}

/// Every value given for a repeatable flag, in the order given.
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

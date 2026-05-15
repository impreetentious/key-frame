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

use kf_bitstream::SequenceHeader;
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
            vec![
                "corpus/clips/akiyo_qcif.y4m".to_owned(),
                "corpus/clips/foreman_qcif.y4m".to_owned(),
            ]
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
    let mut points_run = 0_usize;
    for clip_path in &clips {
        let clip = load(clip_path, frames)?;
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

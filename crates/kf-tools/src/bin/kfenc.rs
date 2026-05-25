//! `kfenc`: Y4M in, a Key Frame stream out, and a receipt for what it did.
//!
//! The per-frame table has always shown what each frame cost in bytes. What it
//! could not show is what those bytes bought, because quality is a comparison
//! and a comparison needs the decoded picture — which an encoder does not
//! produce. It produces a reconstruction, and the two are required to be
//! identical, which is exactly why measuring against the reconstruction would
//! be the wrong thing to do: a drift bug would then be invisible in the numbers
//! a reader would use to judge the codec.
//!
//! So `--stats` decodes the stream it just wrote, with the shipping decoder,
//! and measures that. It costs a decode, which is why it is a flag rather than
//! the default, and it writes a receipt carrying the source by content hash,
//! the settings, the coded stream's hash, and every per-frame figure — enough
//! for someone else to run the same encode and compare.

use std::{env, fs, process::ExitCode};

use kf_bitstream::SequenceHeader;
use kf_dec::FastDecoder;
use kf_enc::Encoder;
use kf_tools::{
    Json, decode_y4m,
    json::{number, object, string},
    psnr_y, sha256_hex,
};

const USAGE: &str = "\
usage: kfenc --input <clip.y4m> --output <stream.kfv> [options]

  --qp N               constant quantizer, 0 through 63, default 32
  --bitrate BPS        average-bitrate target instead of a constant quantizer
  --kf-interval N      keyframe interval, default 120
  --golden-interval N  golden refresh interval, default 16
  --stats FILE         decode the stream and write a receipt: the source by
                       content hash, the settings, the coded stream's hash, and
                       each frame's payload and luma PSNR";

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("kfenc: {message}");
            ExitCode::from(1)
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    let input = value(&arguments, "--input")?;
    let output = value(&arguments, "--output")?;
    let stats_path = optional_value(&arguments, "--stats");
    let qp_flag = optional_value(&arguments, "--qp");
    let bitrate_flag = optional_value(&arguments, "--bitrate");
    if qp_flag.is_some() && bitrate_flag.is_some() {
        return Err("--qp and --bitrate are mutually exclusive".to_owned());
    }
    let keyframe_interval = optional_value(&arguments, "--kf-interval")
        .unwrap_or(&declared_default("default_keyframe_interval"))
        .parse::<u16>()
        .map_err(|_| "--kf-interval must be a nonzero u16".to_owned())?;
    let golden_interval = optional_value(&arguments, "--golden-interval")
        .unwrap_or(&declared_default("default_golden_interval"))
        .parse::<u8>()
        .map_err(|_| "--golden-interval must be a nonzero u8".to_owned())?;
    reject_unknown(&arguments)?;

    let source_bytes = fs::read(input).map_err(|error| format!("{input}: {error}"))?;
    let y4m = decode_y4m(&source_bytes).map_err(|error| error.to_string())?;
    let sequence = SequenceHeader::new(
        y4m.width,
        y4m.height,
        y4m.fps_num,
        y4m.fps_den,
        keyframe_interval,
        golden_interval,
    )
    .map_err(|error| error.to_string())?;
    // Parsed once, so the receipt records the value the encoder was built from
    // rather than the text a caller typed.
    let rate = match bitrate_flag {
        Some(bitrate) => Rate::Average(
            bitrate
                .parse::<u32>()
                .map_err(|_| "--bitrate must be a positive integer".to_owned())?,
        ),
        None => Rate::ConstantQp(
            qp_flag
                .unwrap_or("32")
                .parse::<u8>()
                .map_err(|_| "--qp must be an integer from 0 through 63".to_owned())?,
        ),
    };
    let encoder = match rate {
        Rate::Average(bitrate) => {
            Encoder::with_bitrate(sequence, bitrate).map_err(|error| error.to_string())?
        }
        Rate::ConstantQp(qp) => Encoder::new(sequence, qp).map_err(|error| error.to_string())?,
    };
    let encoded = encoder
        .encode(&y4m.frames)
        .map_err(|error| error.to_string())?;
    fs::write(output, &encoded.bytes).map_err(|error| format!("{output}: {error}"))?;

    // Measured against what decodes, never against the encoder's own copy. The
    // two are required to be identical and every gate in this repository checks
    // that they are; measuring the copy anyway would make the one bug that
    // matters most invisible in exactly the figures a reader would trust.
    let quality = match stats_path {
        None => None,
        Some(_) => {
            let decoded = FastDecoder::new()
                .decode_stream(&encoded.bytes)
                .map_err(|error| {
                    format!("the stream this encode produced did not decode: {error}")
                })?;
            Some(psnr_y(&y4m.frames, &decoded).map_err(|error| error.to_string())?)
        }
    };

    for (position, frame) in encoded.frames.iter().enumerate() {
        // Per frame, not per clip: one frame can be coded losslessly in a clip
        // that is not, and its PSNR is then unbounded. A large finite number in
        // its place would be a lie with a decimal point.
        let psnr = quality.as_ref().map_or_else(String::new, |quality| {
            match quality.per_frame.get(position) {
                Some(value) if value.is_finite() => format!("  psnr {value:>6.2} dB"),
                _ => "  psnr lossless".to_owned(),
            }
        });
        println!(
            "frame {:>5}  type {}  qp {:>2}  payload {:>8} bytes{psnr}",
            frame.frame_index,
            if frame.key { "K" } else { "P" },
            frame.qp,
            frame.payload_len
        );
    }
    println!(
        "encoded {} frame(s), {}x{}, {} bytes",
        encoded.frames.len(),
        y4m.width,
        y4m.height,
        encoded.bytes.len()
    );

    if let (Some(path), Some(quality)) = (stats_path, quality.as_ref()) {
        let document = receipt(&Receipt {
            source: input,
            source_bytes: &source_bytes,
            y4m: &y4m,
            encoded: &encoded,
            quality,
            rate,
            keyframe_interval,
            golden_interval,
        });
        fs::write(path, document.to_pretty(2)).map_err(|error| format!("{path}: {error}"))?;
        println!("wrote {path}");
    }
    Ok(())
}

/// Which knob the encode was driven by, as the encoder received it.
#[derive(Clone, Copy)]
enum Rate {
    ConstantQp(u8),
    Average(u32),
}

struct Receipt<'a> {
    source: &'a str,
    source_bytes: &'a [u8],
    y4m: &'a kf_tools::Y4mStream,
    encoded: &'a kf_enc::EncodedStream,
    quality: &'a kf_tools::Quality,
    rate: Rate,
    keyframe_interval: u16,
    golden_interval: u8,
}

/// Everything a second person needs to run this encode again and compare.
///
/// The source is named by content hash rather than by path, because a path is a
/// fact about one machine. The coded stream is hashed too: an encode that
/// reproduced every figure here from different bytes would be a coincidence
/// worth knowing about.
fn receipt(receipt: &Receipt<'_>) -> Json {
    let Receipt {
        source,
        source_bytes,
        y4m,
        encoded,
        quality,
        rate,
        keyframe_interval,
        golden_interval,
    } = receipt;

    let mut settings = vec![
        ("frames", number(y4m.frames.len() as f64)),
        ("width", number(f64::from(y4m.width))),
        ("height", number(f64::from(y4m.height))),
        ("keyframe_interval", number(f64::from(*keyframe_interval))),
        ("golden_interval", number(f64::from(*golden_interval))),
    ];
    match rate {
        Rate::ConstantQp(qp) => settings.push(("qp", number(f64::from(*qp)))),
        Rate::Average(bitrate) => settings.push(("bitrate_bps", number(f64::from(*bitrate)))),
    }

    let frames: Vec<Json> = encoded
        .frames
        .iter()
        .enumerate()
        .map(|(position, frame)| {
            let mut members = vec![
                ("frame_index", number(f64::from(frame.frame_index))),
                ("key", Json::Bool(frame.key)),
                ("golden_refresh", Json::Bool(frame.golden_refresh)),
                ("qp", number(f64::from(frame.qp))),
                ("payload_len", number(frame.payload_len as f64)),
                ("frame_flush_bytes", number(frame.frame_flush_bytes as f64)),
            ];
            // A losslessly coded frame has unbounded PSNR. `number` writes that
            // as `null`, which is the convention this JSON module states and the
            // only honest answer: a large finite number would be a lie with a
            // decimal point.
            members.push((
                "psnr_y",
                quality
                    .per_frame
                    .get(position)
                    .copied()
                    .map_or(Json::Null, number),
            ));
            object(members)
        })
        .collect();

    let fps = f64::from(y4m.fps_num) / f64::from(y4m.fps_den);
    let seconds = y4m.frames.len() as f64 / fps;
    object(vec![
        ("format", string("key-frame-encode-stats-v1")),
        ("encoder_version", string(env!("CARGO_PKG_VERSION"))),
        ("source", string(*source)),
        ("source_sha256", string(sha256_hex(source_bytes))),
        (
            "metrics",
            object(vec![
                ("psnr", string("psnr-y")),
                (
                    "note",
                    string(
                        "Luma only, cropped to the displayed picture, measured against the \
                         decoded stream rather than the encoder's own reconstruction.",
                    ),
                ),
            ]),
        ),
        ("settings", object(settings)),
        (
            "stream",
            object(vec![
                ("bytes", number(encoded.bytes.len() as f64)),
                ("sha256", string(sha256_hex(&encoded.bytes))),
                (
                    "rate_bps",
                    number((encoded.bytes.len() as f64 * 8.0) / seconds),
                ),
            ]),
        ),
        (
            "quality",
            object(vec![
                ("lossless", Json::Bool(quality.lossless)),
                ("psnr_y", number(quality.global)),
            ]),
        ),
        ("frames", Json::Array(frames)),
    ])
}

/// A declared default from the frozen constants.
///
/// The tools carried `120` and `16` in their argument parsing, which is the
/// specification's own defaults written a third and fourth time. A campaign
/// receipt records the intervals it used, so the settings a reader compares
/// against the document agreed with it only because the same numbers were typed
/// in four places.
fn declared_default(key: &str) -> String {
    kf_spec::V1_ASSETS
        .iter()
        .find(|asset| asset.name == "constants.toml")
        .expect("the specification exposes constants.toml")
        .contents
        .lines()
        .find_map(|line| Some(line.strip_prefix(&format!("{key} = "))?.trim().to_owned()))
        .unwrap_or_else(|| panic!("constants.toml declares no {key}"))
}

fn value<'a>(arguments: &'a [String], flag: &str) -> Result<&'a str, String> {
    optional_value(arguments, flag).ok_or_else(|| format!("missing {flag}\n\n{USAGE}"))
}

fn optional_value<'a>(arguments: &'a [String], flag: &str) -> Option<&'a str> {
    arguments
        .iter()
        .position(|argument| argument == flag)
        .and_then(|index| arguments.get(index + 1))
        .map(String::as_str)
}

fn reject_unknown(arguments: &[String]) -> Result<(), String> {
    let known = [
        "--input",
        "--output",
        "--qp",
        "--bitrate",
        "--kf-interval",
        "--golden-interval",
        "--stats",
    ];
    let mut index = 0;
    while index < arguments.len() {
        if !known.contains(&arguments[index].as_str()) || index + 1 >= arguments.len() {
            return Err(format!(
                "unknown or incomplete argument {}\n\n{USAGE}",
                arguments[index]
            ));
        }
        index += 2;
    }
    Ok(())
}

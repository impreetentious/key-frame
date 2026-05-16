use std::{env, fs, process::ExitCode};

use kf_bitstream::{SEQUENCE_HEADER_SIZE, SequenceHeader};
use kf_enc::Encoder;
use kf_tools::decode_y4m;

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("measure-rate: {message}");
            ExitCode::from(1)
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    let input = flag(&arguments, "--input")?;
    let bitrate = flag(&arguments, "--bitrate")?
        .parse::<u32>()
        .map_err(|_| "--bitrate must be a positive integer".to_owned())?;
    let frame_count = flag(&arguments, "--frames")?
        .parse::<usize>()
        .map_err(|_| "--frames must be a positive integer".to_owned())?;
    if bitrate == 0 || frame_count == 0 {
        return Err("bitrate and frame count must be positive".to_owned());
    }

    let y4m = decode_y4m(&fs::read(input).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let frames = y4m
        .frames
        .get(..frame_count)
        .ok_or_else(|| format!("{input} has fewer than {frame_count} frames"))?;
    let sequence = SequenceHeader::new(y4m.width, y4m.height, y4m.fps_num, y4m.fps_den, 120, 16)
        .map_err(|error| error.to_string())?;
    let encoder = Encoder::with_bitrate(sequence, bitrate).map_err(|error| error.to_string())?;
    let first = encoder.encode(frames).map_err(|error| error.to_string())?;
    let second = encoder.encode(frames).map_err(|error| error.to_string())?;
    if first.bytes != second.bytes {
        return Err("ABR encode is not deterministic".to_owned());
    }

    let coded_bits = u64::try_from(first.bytes.len().saturating_sub(SEQUENCE_HEADER_SIZE))
        .unwrap_or(u64::MAX)
        .saturating_mul(8);
    let target_bits = u64::from(bitrate)
        .saturating_mul(u64::try_from(frame_count).unwrap_or(u64::MAX))
        .saturating_mul(u64::from(y4m.fps_den))
        / u64::from(y4m.fps_num);
    if target_bits == 0 {
        return Err("target budget is zero".to_owned());
    }
    let delta = coded_bits.abs_diff(target_bits);
    let hundredths = delta.saturating_mul(10_000) / target_bits;
    let qps: Vec<String> = first
        .frames
        .iter()
        .map(|frame| frame.qp.to_string())
        .collect();
    println!(
        "measure-rate: {input} frames={frame_count} bitrate={bitrate} coded={coded_bits} target={target_bits} error={}.{:02}% qp=[{}]",
        hundredths / 100,
        hundredths % 100,
        qps.join(",")
    );
    // The tolerance is the one the constant table declares, not one this
    // example chose. A threshold written here would be a second, competing
    // statement of what "within tolerance" means.
    let tolerance = declared_tolerance_percent();
    if delta.saturating_mul(100) > target_bits.saturating_mul(u64::from(tolerance)) {
        return Err(format!(
            "rate error {}.{}% exceeds {tolerance}%",
            hundredths / 100,
            hundredths % 100
        ));
    }
    Ok(())
}

/// The declared average-bitrate tolerance, in percent.
fn declared_tolerance_percent() -> u32 {
    kf_spec::V1_ASSETS
        .iter()
        .find(|asset| asset.name == "constants.toml")
        .expect("invariant: kf-spec exposes constants.toml")
        .contents
        .lines()
        .find_map(|line| line.strip_prefix("abr_tolerance_percent = "))
        .expect("invariant: checked constant table declares the bitrate tolerance")
        .trim()
        .parse()
        .expect("invariant: checked tolerance is a percentage")
}

fn flag<'a>(arguments: &'a [String], name: &str) -> Result<&'a str, String> {
    arguments
        .iter()
        .position(|argument| argument == name)
        .and_then(|index| arguments.get(index + 1).map(String::as_str))
        .ok_or_else(|| format!("missing {name}"))
}

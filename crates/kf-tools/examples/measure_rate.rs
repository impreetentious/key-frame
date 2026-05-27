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

/// One measured operating point: the error in hundredths of a percent.
struct Measured {
    coded_bits: u64,
    target_bits: u64,
    delta: u64,
    hundredths: u64,
    /// `+` when the encode overshot its budget, `-` when it undershot.
    ///
    /// The magnitude alone was all this printed, and a page describing the
    /// controller concluded from it that a short clip undershoots. One of the
    /// two pinned clips overshoots. A signed figure makes the direction
    /// something a reader sees rather than something a writer assumed.
    sign: char,
    qps: Vec<String>,
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    let input = flag(&arguments, "--input")?;
    let bitrate = flag(&arguments, "--bitrate")?
        .parse::<u32>()
        .map_err(|_| "--bitrate must be a positive integer".to_owned())?;
    let frame_count = flag(&arguments, "--frames")?
        .parse::<usize>()
        .map_err(|_| "--frames must be a positive integer".to_owned())?;
    // The short prefix the settled measurement is compared against. Optional,
    // because most callers want one operating point; the rate gate passes it
    // so the convergence `docs/LIMITATIONS.md` describes is measured rather
    // than remembered. That page used to publish a range — "5% to 13% over 24
    // frames" — that no receipt, gate, test, or decision record produced, and
    // that measurement does not support.
    let prefix_count = optional_flag(&arguments, "--converges-from")
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|_| "--converges-from must be a positive integer".to_owned())
        })
        .transpose()?;
    if bitrate == 0 || frame_count == 0 || prefix_count == Some(0) {
        return Err("bitrate and frame counts must be positive".to_owned());
    }
    if let Some(prefix) = prefix_count
        && prefix >= frame_count
    {
        return Err("--converges-from must name a shorter prefix than --frames".to_owned());
    }

    let y4m = decode_y4m(&fs::read(input).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let sequence = SequenceHeader::new(y4m.width, y4m.height, y4m.fps_num, y4m.fps_den, 120, 16)
        .map_err(|error| error.to_string())?;
    let encoder = Encoder::with_bitrate(sequence, bitrate).map_err(|error| error.to_string())?;

    let measure = |count: usize| -> Result<Measured, String> {
        let frames = y4m
            .frames
            .get(..count)
            .ok_or_else(|| format!("{input} has fewer than {count} frames"))?;
        let first = encoder.encode(frames).map_err(|error| error.to_string())?;
        let second = encoder.encode(frames).map_err(|error| error.to_string())?;
        if first.bytes != second.bytes {
            return Err("ABR encode is not deterministic".to_owned());
        }
        let coded_bits = u64::try_from(first.bytes.len().saturating_sub(SEQUENCE_HEADER_SIZE))
            .unwrap_or(u64::MAX)
            .saturating_mul(8);
        let target_bits = u64::from(bitrate)
            .saturating_mul(u64::try_from(count).unwrap_or(u64::MAX))
            .saturating_mul(u64::from(y4m.fps_den))
            / u64::from(y4m.fps_num);
        if target_bits == 0 {
            return Err("target budget is zero".to_owned());
        }
        let delta = coded_bits.abs_diff(target_bits);
        Ok(Measured {
            coded_bits,
            target_bits,
            delta,
            hundredths: delta.saturating_mul(10_000) / target_bits,
            sign: if coded_bits >= target_bits { '+' } else { '-' },
            qps: first
                .frames
                .iter()
                .map(|frame| frame.qp.to_string())
                .collect(),
        })
    };

    let settled = measure(frame_count)?;
    println!(
        "measure-rate: {input} frames={frame_count} bitrate={bitrate} coded={} target={} error={}{}.{:02}% qp=[{}]",
        settled.coded_bits,
        settled.target_bits,
        settled.sign,
        settled.hundredths / 100,
        settled.hundredths % 100,
        settled.qps.join(",")
    );

    // The tolerance is the one the constant table declares, not one this
    // example chose. A threshold written here would be a second, competing
    // statement of what "within tolerance" means.
    let tolerance = declared_tolerance_percent();
    if settled.delta.saturating_mul(100) > settled.target_bits.saturating_mul(u64::from(tolerance))
    {
        return Err(format!(
            "rate error {}.{}% exceeds {tolerance}%",
            settled.hundredths / 100,
            settled.hundredths % 100
        ));
    }

    if let Some(prefix) = prefix_count {
        // The short prefix is deliberately not held to the tolerance: the
        // claim is that a single-pass controller is worse before it converges,
        // and requiring it to be within tolerance anyway would be requiring
        // the opposite of what is being described.
        let short = measure(prefix)?;
        println!(
            "measure-rate: {input} frames={prefix} bitrate={bitrate} error={}{}.{:02}% (before convergence)",
            short.sign,
            short.hundredths / 100,
            short.hundredths % 100
        );
        if short.hundredths <= settled.hundredths {
            return Err(format!(
                "the {prefix}-frame error {}.{:02}% is not worse than the {frame_count}-frame error {}.{:02}%, \
                 so the convergence this measurement exists to show did not happen",
                short.hundredths / 100,
                short.hundredths % 100,
                settled.hundredths / 100,
                settled.hundredths % 100
            ));
        }
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
    optional_flag(arguments, name).ok_or_else(|| format!("missing {name}"))
}

fn optional_flag<'a>(arguments: &'a [String], name: &str) -> Option<&'a str> {
    arguments
        .iter()
        .position(|argument| argument == name)
        .and_then(|index| arguments.get(index + 1).map(String::as_str))
}

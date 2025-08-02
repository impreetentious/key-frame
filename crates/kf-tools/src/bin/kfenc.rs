use std::{env, fs, process::ExitCode};

use kf_bitstream::SequenceHeader;
use kf_enc::Encoder;
use kf_tools::decode_y4m;

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
    let qp = optional_value(&arguments, "--qp")
        .unwrap_or("32")
        .parse::<u8>()
        .map_err(|_| "--qp must be an integer from 0 through 63".to_owned())?;
    let keyframe_interval = optional_value(&arguments, "--kf-interval")
        .unwrap_or("120")
        .parse::<u16>()
        .map_err(|_| "--kf-interval must be a nonzero u16".to_owned())?;
    let golden_interval = optional_value(&arguments, "--golden-interval")
        .unwrap_or("16")
        .parse::<u8>()
        .map_err(|_| "--golden-interval must be a nonzero u8".to_owned())?;
    reject_unknown(&arguments)?;
    let y4m = decode_y4m(&fs::read(input).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let sequence = SequenceHeader::new(
        y4m.width,
        y4m.height,
        y4m.fps_num,
        y4m.fps_den,
        keyframe_interval,
        golden_interval,
    )
    .map_err(|error| error.to_string())?;
    let encoded = Encoder::new(sequence, qp)
        .map_err(|error| error.to_string())?
        .encode(&y4m.frames)
        .map_err(|error| error.to_string())?;
    fs::write(output, &encoded.bytes).map_err(|error| error.to_string())?;
    for frame in &encoded.frames {
        println!(
            "frame {:>5}  type {}  qp {:>2}  payload {:>8} bytes",
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
    Ok(())
}

fn value<'a>(arguments: &'a [String], flag: &str) -> Result<&'a str, String> {
    optional_value(arguments, flag).ok_or_else(|| format!("missing {flag}"))
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
        "--kf-interval",
        "--golden-interval",
    ];
    let mut index = 0;
    while index < arguments.len() {
        if !known.contains(&arguments[index].as_str()) || index + 1 >= arguments.len() {
            return Err(format!(
                "unknown or incomplete argument {}",
                arguments[index]
            ));
        }
        index += 2;
    }
    Ok(())
}

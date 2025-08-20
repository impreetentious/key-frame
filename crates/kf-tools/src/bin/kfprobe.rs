use std::{env, fs, process::ExitCode};

use kf_tools::probe_frame;

const USAGE: &str = "usage: kfprobe <stream.kfv> [--frame N]";

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let (path, frame) = match parse(&arguments) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("kfprobe: {message}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("kfprobe: cannot read {path}: {error}");
            return ExitCode::from(1);
        }
    };
    match probe_frame(&bytes, frame) {
        Ok(report) => {
            println!("{}", report.to_json());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("kfprobe: {error}");
            ExitCode::from(1)
        }
    }
}

/// Returns the stream path and the frame to report, which defaults to the
/// first. A frame the flag names but cannot parse is an error rather than a
/// silent fall back to zero: reporting a different frame than the one asked for
/// is the one answer worse than refusing.
fn parse(arguments: &[String]) -> Result<(&str, u32), String> {
    let mut path = None;
    let mut frame = 0_u32;
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        match argument {
            "--frame" => {
                let value = arguments
                    .get(index + 1)
                    .ok_or_else(|| "--frame needs a frame number".to_owned())?;
                frame = value
                    .parse::<u32>()
                    .map_err(|_| format!("{value} is not a frame number"))?;
                index += 2;
            }
            _ if argument.starts_with('-') => {
                return Err(format!("unknown option {argument}"));
            }
            _ => {
                if path.is_some() {
                    return Err("only one stream may be probed at a time".to_owned());
                }
                path = Some(argument);
                index += 1;
            }
        }
    }
    Ok((path.ok_or_else(|| "no stream given".to_owned())?, frame))
}

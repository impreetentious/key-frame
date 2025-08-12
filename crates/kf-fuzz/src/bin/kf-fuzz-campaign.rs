use std::{env, process::ExitCode};

use kf_fuzz::{FuzzTarget, NIGHTLY_ITERATIONS, run_campaign};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("kf-fuzz-campaign: {message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let iterations = iterations_from_args()?;
    for target in FuzzTarget::ALL {
        println!("kf-fuzz: {} {iterations}", target.name());
        run_campaign(target, iterations)?;
    }
    println!("kf-fuzz: OK — {iterations} iterations on four targets");
    Ok(())
}

fn iterations_from_args() -> Result<u32, String> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    if arguments.is_empty() {
        return Ok(NIGHTLY_ITERATIONS);
    }
    if arguments.len() == 2 && arguments[0] == "--iterations" {
        return arguments[1]
            .parse::<u32>()
            .map_err(|_| "--iterations must be a positive integer".to_owned())
            .and_then(|value| {
                if value == 0 {
                    Err("--iterations must be a positive integer".to_owned())
                } else {
                    Ok(value)
                }
            });
    }
    Err("usage: kf-fuzz-campaign [--iterations N]".to_owned())
}

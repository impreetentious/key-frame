use std::{env, process::ExitCode};

use kf_fuzz::{FuzzTarget, NIGHTLY_ITERATIONS, overflow_is_checked, run_campaign};

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
    // A campaign that cannot see an overflow is a campaign reporting on a
    // decoder it did not fully test, and it would report success. Refusing is
    // the only outcome that stays honest: the profile is part of what the run
    // is worth, so a build without checked arithmetic does not get to claim it
    // ran the campaign.
    if !overflow_is_checked() {
        return Err(
            "this build wraps on integer overflow; the campaign needs a profile with \
             overflow-checks on, which the workspace manifest sets for release"
                .to_owned(),
        );
    }
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

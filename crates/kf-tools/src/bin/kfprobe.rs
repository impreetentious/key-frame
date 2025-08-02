use std::{env, fs, process::ExitCode};

use kf_tools::probe_stream;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: kfprobe <stream.kfv>");
        return ExitCode::from(2);
    };
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("kfprobe: cannot read {path}: {error}");
            return ExitCode::from(1);
        }
    };
    match probe_stream(&bytes) {
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

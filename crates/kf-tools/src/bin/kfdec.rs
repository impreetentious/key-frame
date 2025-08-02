use std::{env, fs, process::ExitCode};

use kf_bitstream::SequenceHeader;
use kf_dec::FastDecoder;
use kf_tools::{Y4mStream, encode_y4m};

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("kfdec: {message}");
            ExitCode::from(1)
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    let (input, output) = paths(&arguments)?;
    let bytes = fs::read(input).map_err(|error| error.to_string())?;
    let sequence = SequenceHeader::decode(&bytes).map_err(|error| error.to_string())?;
    let frames = FastDecoder::new()
        .decode_stream(&bytes)
        .map_err(|error| error.to_string())?;
    let frame_count = frames.len();
    let y4m = encode_y4m(&Y4mStream {
        width: sequence.width,
        height: sequence.height,
        fps_num: sequence.fps_num,
        fps_den: sequence.fps_den,
        frames,
    })
    .map_err(|error| error.to_string())?;
    fs::write(output, y4m).map_err(|error| error.to_string())?;
    println!("magic                 KFV1");
    println!("bitstream version     1");
    println!(
        "dimensions            {}x{}",
        sequence.width, sequence.height
    );
    println!("chroma / depth        4:2:0 JPEG / 8-bit");
    println!(
        "frame rate            {}/{}",
        sequence.fps_num, sequence.fps_den
    );
    println!("decoded {frame_count} frame(s)");
    Ok(())
}

fn paths(arguments: &[String]) -> Result<(&str, &str), String> {
    let output_index = arguments
        .iter()
        .position(|argument| argument == "--output")
        .ok_or_else(|| "missing --output".to_owned())?;
    let output = arguments
        .get(output_index + 1)
        .ok_or_else(|| "missing --output value".to_owned())?;
    let input =
        if let Some(input_index) = arguments.iter().position(|argument| argument == "--input") {
            arguments
                .get(input_index + 1)
                .ok_or_else(|| "missing --input value".to_owned())?
        } else {
            arguments
                .first()
                .filter(|argument| !argument.starts_with('-'))
                .ok_or_else(|| "usage: kfdec INPUT.kfv --output OUTPUT.y4m".to_owned())?
        };
    Ok((input, output))
}

//! Encodes a pinned corpus clip at a spread of QPs and proves that both
//! decoders and the encoder's own closed loop agree on every reconstructed
//! sample. Synthetic sources cannot exercise real motion, so this is the only
//! place where natural content drives the full intra/inter/filter path.

use std::{env, fs, process::ExitCode};

use kf_bitstream::SequenceHeader;
use kf_dec::FastDecoder;
use kf_enc::Encoder;
use kf_frame::Frame;
use kf_ref::ReferenceDecoder;
use kf_tools::{Y4mStream, decode_y4m, encode_y4m, sha256_hex};

/// The QP ladder the corpus is proven across, spanning the legal range.
const QP_LADDER: [u8; 5] = [8, 20, 32, 44, 60];

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("corpus-bitexact: {message}");
            ExitCode::from(1)
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    let input = flag(&arguments, "--input")?;
    let frame_count = flag(&arguments, "--frames")?
        .parse::<usize>()
        .map_err(|_| "--frames must be a positive integer".to_owned())?;
    if frame_count == 0 {
        return Err("frame count must be positive".to_owned());
    }

    let y4m = decode_y4m(&fs::read(input).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let frames = y4m
        .frames
        .get(..frame_count)
        .ok_or_else(|| format!("{input} has fewer than {frame_count} frames"))?;
    let sequence = SequenceHeader::new(y4m.width, y4m.height, y4m.fps_num, y4m.fps_den, 120, 16)
        .map_err(|error| error.to_string())?;

    for qp in QP_LADDER {
        let encoder = Encoder::new(sequence, qp).map_err(|error| error.to_string())?;
        let first = encoder.encode(frames).map_err(|error| error.to_string())?;
        let second = encoder.encode(frames).map_err(|error| error.to_string())?;
        if first.bytes != second.bytes {
            return Err(format!("qp {qp}: encode is not deterministic"));
        }

        let fast = FastDecoder::new()
            .decode_stream(&first.bytes)
            .map_err(|error| format!("qp {qp}: fast decoder rejected the stream ({error})"))?;
        let reference = ReferenceDecoder::new()
            .decode_stream(&first.bytes)
            .map_err(|error| format!("qp {qp}: reference decoder rejected the stream ({error})"))?;

        if fast.len() != frame_count || reference.len() != frame_count {
            return Err(format!(
                "qp {qp}: decoded {} and {} frames, expected {frame_count}",
                fast.len(),
                reference.len()
            ));
        }
        compare(qp, "decoders disagree", &fast, &reference)?;
        compare(
            qp,
            "encoder closed loop drifted from decode",
            &fast,
            &first.reconstructed_frames,
        )?;

        let decoded = Y4mStream {
            width: y4m.width,
            height: y4m.height,
            fps_num: y4m.fps_num,
            fps_den: y4m.fps_den,
            frames: fast,
        };
        let bytes = encode_y4m(&decoded).map_err(|error| error.to_string())?;
        println!(
            "corpus-bitexact: {input} qp={qp} frames={frame_count} bytes={} sha256={}",
            first.bytes.len(),
            sha256_hex(&bytes)
        );
    }
    Ok(())
}

/// Reports the first differing sample rather than only that a mismatch exists,
/// so a failure names the frame and plane that broke.
fn compare(qp: u8, what: &str, left: &[Frame], right: &[Frame]) -> Result<(), String> {
    for (index, (left_frame, right_frame)) in left.iter().zip(right).enumerate() {
        if left_frame == right_frame {
            continue;
        }
        let planes = [
            ("y", left_frame.y.data(), right_frame.y.data()),
            ("cb", left_frame.cb.data(), right_frame.cb.data()),
            ("cr", left_frame.cr.data(), right_frame.cr.data()),
        ];
        for (name, left_plane, right_plane) in planes {
            if let Some(offset) = left_plane
                .iter()
                .zip(right_plane)
                .position(|(left, right)| left != right)
            {
                return Err(format!(
                    "qp {qp}: {what} at frame {index} plane {name} offset {offset} ({} vs {})",
                    left_plane[offset], right_plane[offset]
                ));
            }
        }
        return Err(format!("qp {qp}: {what} at frame {index}"));
    }
    Ok(())
}

fn flag<'a>(arguments: &'a [String], name: &str) -> Result<&'a str, String> {
    arguments
        .iter()
        .position(|argument| argument == name)
        .and_then(|index| arguments.get(index + 1).map(String::as_str))
        .ok_or_else(|| format!("missing {name}"))
}

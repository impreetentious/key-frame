//! `kfprobe`: one frame's syntax, as JSON or as something a person reads.
//!
//! The JSON is the wire format the projection room draws from, and it is the
//! default because a tool whose output is consumed by another program should
//! not have to be asked for the machine-readable form.
//!
//! `--summary` is the other half: the partition tree drawn as the quadtree it
//! is, each leaf's prediction, and the two accounting figures side by side with
//! neither of them labelled as the block's cost. That rendering existed — as
//! twenty lines of Python inside `scripts/demo.sh`, reaching into the JSON with
//! unchecked dictionary lookups. It was a second reader of the probe report,
//! validated by nothing, in a shell script; here it reads the typed report and
//! never becomes JSON at all.

use std::{env, fs, process::ExitCode};

use kf_tools::{BlockProbe, ProbeReport, SuperblockProbe, probe_frame};

const USAGE: &str = "\
usage: kfprobe <stream.kfv> [--frame N] [--summary] [--superblock N]

  --frame N       the frame to report, default the first
  --summary       a readable rendering instead of JSON: the partition tree, each
                  leaf's prediction, and both accounting figures
  --superblock N  which superblock --summary details, default the first";

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let (path, frame, summary, superblock) = match parse(&arguments) {
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
            if summary {
                match render_summary(&report, superblock) {
                    Ok(text) => print!("{text}"),
                    Err(message) => {
                        eprintln!("kfprobe: {message}");
                        return ExitCode::from(1);
                    }
                }
            } else {
                println!("{}", report.to_json());
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("kfprobe: {error}");
            ExitCode::from(1)
        }
    }
}

/// Returns the stream path, the frame to report, whether to render it, and
/// which superblock to detail.
///
/// A number a flag names but cannot parse is an error rather than a silent fall
/// back to zero: reporting a different frame than the one asked for is the one
/// answer worse than refusing.
fn parse(arguments: &[String]) -> Result<(&str, u32, bool, usize), String> {
    let mut path = None;
    let mut frame = 0_u32;
    let mut summary = false;
    let mut superblock = 0_usize;
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        match argument {
            "--frame" | "--superblock" => {
                let value = arguments
                    .get(index + 1)
                    .ok_or_else(|| format!("{argument} needs a number"))?;
                if argument == "--frame" {
                    frame = value
                        .parse::<u32>()
                        .map_err(|_| format!("{value} is not a frame number"))?;
                } else {
                    superblock = value
                        .parse::<usize>()
                        .map_err(|_| format!("{value} is not a superblock number"))?;
                }
                index += 2;
            }
            "--summary" => {
                summary = true;
                index += 1;
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
    Ok((
        path.ok_or_else(|| "no stream given".to_owned())?,
        frame,
        summary,
        superblock,
    ))
}

/// The readable rendering: what coded this frame, and what it is not safe to
/// say about the cost of any of it.
fn render_summary(report: &ProbeReport, wanted: usize) -> Result<String, String> {
    let blocks: usize = report
        .superblocks
        .iter()
        .map(|superblock| superblock.blocks.len())
        .sum();
    let mut out = format!(
        "frame {}, {}, qp {}, {} superblock(s), {blocks} coding block(s)\n",
        report.frame_index,
        if report.key { "key" } else { "inter" },
        report.frame_qp,
        report.superblocks.len(),
    );

    let superblock = report.superblocks.get(wanted).ok_or_else(|| {
        format!(
            "this frame has {} superblock(s), so there is no superblock {wanted}",
            report.superblocks.len()
        )
    })?;
    out.push_str(&render_superblock(superblock));

    // The two accounting figures, kept apart. Adding them would produce a
    // number that looks like "the bits this block cost" and is not one: an
    // adaptive coder does not give any block a private set of bits to occupy.
    out.push_str("\n  accounting, the two quantities never added:\n");
    out.push_str("    modeled entropy   what the encoder's model predicted\n");
    out.push_str("    emission-time     when the range coder happened to flush a byte\n");
    for block in superblock.blocks.iter().take(4) {
        out.push_str(&format!(
            "      {:2}x{:<2} at ({:3},{:3})  modeled {:>8} bits   emitted {:3} bytes\n",
            block.size,
            block.size,
            block.x,
            block.y,
            modeled_bits(block.modeled_entropy_q16),
            block.emitted_payload_bytes,
        ));
    }
    out.push_str("    Neither is 'this block's bit count'. That question has no answer.\n");

    out.push_str(&format!(
        "\n  input payload {} bytes, canonical replay {} bytes, match: {}\n",
        report.input_payload_len,
        report.canonical_replay_payload_len,
        report.canonical_payload_match,
    ));
    if let Some(offset) = report.first_mismatch_offset {
        out.push_str(&format!(
            "  first mismatch at byte {offset}; the replay's timing is shown and no input byte \
             is attributed to a block\n"
        ));
    }
    Ok(out)
}

/// One superblock's quadtree, drawn so a split reads as a staircase.
fn render_superblock(superblock: &SuperblockProbe) -> String {
    let mut out = format!(
        "\n  superblock at ({}, {}) partitions to:\n",
        superblock.x, superblock.y
    );
    for block in &superblock.blocks {
        // The bar is as long as the block is wide, so the shape of the tree is
        // visible before any of the numbers are read.
        let bar = "#".repeat(usize::from(block.size / 8).max(1));
        out.push_str(&format!(
            "    {bar:<8} {:2}x{:<2} at ({:3},{:3})  {}\n",
            block.size,
            block.size,
            block.x,
            block.y,
            describe(block),
        ));
    }
    out
}

/// What predicted one block, in the vocabulary the report uses.
fn describe(block: &BlockProbe) -> String {
    match (block.reference, block.motion_vector_q4) {
        (None, _) => format!("intra {}", block.mode),
        (Some(reference), None) => format!("skip {reference}"),
        (Some(reference), Some([x_q4, y_q4])) => {
            format!("inter {reference} mv [{x_q4}, {y_q4}]")
        }
    }
}

/// Q16.16 modeled entropy, written to two decimals without leaving fixed point.
///
/// The figure is exact in the report and there is no reason for it to stop
/// being exact on the way to a terminal: dividing by 65536 in floating point
/// would round twice, once into the double and once into the format.
fn modeled_bits(modeled_entropy_q16: u64) -> String {
    let whole = modeled_entropy_q16 >> 16;
    let hundredths = ((modeled_entropy_q16 & 0xFFFF) * 100 + (1 << 15)) >> 16;
    if hundredths == 100 {
        format!("{}.00", whole + 1)
    } else {
        format!("{whole}.{hundredths:02}")
    }
}

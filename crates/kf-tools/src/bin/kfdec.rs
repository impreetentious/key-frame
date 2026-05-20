//! `kfdec`: turn a Key Frame stream back into Y4M, or find out what a decoder
//! makes of one.
//!
//! Four things a reader wants from a decoder, and the library has had all four
//! for some time while this tool offered only the first:
//!
//!   * the whole stream, which is what a decoder is for;
//!   * one frame, reached the way a player reaches it — by seeking back to the
//!     keyframe that governs it and decoding forward;
//!   * the checksums, recomputed and reported packet by packet, without
//!     decoding anything at all;
//!   * a damaged stream walked to its end instead of abandoned at the first
//!     fault, which is the normative behaviour and the one mode that shows what
//!     the corruption rules actually do.
//!
//! The last of those is also where this tool owes something the library
//! deliberately does not provide. `decode_stream_resilient` reports one status
//! per packet and hands back only the images that were decoded; the normative
//! rule is that a lost frame *repeats the last shown image, for display only*,
//! and that nothing is shown before the first frame that decoded. A Y4M file is
//! display, so that rule is applied here — the timeline this writes has one
//! frame per packet from the first shown one onward, and a decoder that dropped
//! them instead would hand a player a clip that silently runs short and early.

use std::{env, fs, process::ExitCode};

use kf_bitstream::{PacketScanner, SEQUENCE_HEADER_SIZE, ScanEvent, SequenceHeader};
use kf_dec::{FastDecoder, FrameStatus, Recovery};
use kf_frame::Frame;
use kf_tools::{Y4mStream, encode_y4m};

const USAGE: &str = "\
usage: kfdec <stream.kfv> --output <out.y4m> [options]

  --frame N     write only frame N, reached by seeking from the keyframe that
                governs it; reports the entry point and what it cost
  --seek N      begin the output at frame N and decode forward to the end
  --verify      recompute every checksum and report each packet; decodes no
                picture and needs no --output
  --tolerate    walk a damaged stream to its end rather than stopping at the
                first fault, and write the display timeline the corruption
                rules define

--frame and --seek are mutually exclusive. Without --tolerate, a stream with any
damaged packet exits non-zero.";

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("kfdec: {message}");
            ExitCode::from(1)
        }
    }
}

/// What this invocation was asked to produce.
enum Mode {
    /// Every frame, refusing the stream on the first fault.
    Whole,
    /// One frame, by random access.
    Single(u32),
    /// From one frame to the end.
    From(u32),
    /// Checksums only.
    Verify,
    /// Every packet classified, and the display timeline written.
    Tolerate,
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    let options = Options::parse(&arguments)?;
    let bytes = fs::read(&options.input).map_err(|error| format!("{}: {error}", options.input))?;
    let sequence = SequenceHeader::decode(&bytes).map_err(|error| error.to_string())?;
    print_header(&sequence);

    let frames = match options.mode {
        Mode::Verify => return verify(&bytes),
        Mode::Whole => FastDecoder::new()
            .decode_stream(&bytes)
            .map_err(|error| error.to_string())?,
        Mode::Single(index) => {
            let outcome = FastDecoder::new()
                .seek_frame(&bytes, index)
                .map_err(|error| error.to_string())?;
            println!(
                "frame {index} reached from keyframe {}, {} frame(s) decoded",
                outcome.keyframe_index, outcome.frames_decoded
            );
            vec![outcome.frame]
        }
        Mode::From(index) => {
            let decoded = FastDecoder::new()
                .decode_from(&bytes, index)
                .map_err(|error| error.to_string())?;
            println!("{} frame(s) from frame {index} onward", decoded.len());
            decoded
        }
        Mode::Tolerate => tolerate(&bytes)?,
    };

    let output = options
        .output
        .as_deref()
        .ok_or("missing --output".to_owned())?;
    let count = frames.len();
    let y4m = encode_y4m(&Y4mStream {
        width: sequence.width,
        height: sequence.height,
        fps_num: sequence.fps_num,
        fps_den: sequence.fps_den,
        frames,
    })
    .map_err(|error| error.to_string())?;
    fs::write(output, y4m).map_err(|error| format!("{output}: {error}"))?;
    println!("decoded {count} frame(s)");
    Ok(())
}

/// Walks a damaged stream and builds the timeline a player would show.
///
/// The normative rule, applied here rather than in the library: a packet that
/// produced no image repeats the last shown one, and nothing is repeated before
/// the first shown frame, because there is no image to repeat. The report says
/// which packets those were, so a hold is never mistaken for a decode.
fn tolerate(bytes: &[u8]) -> Result<Vec<Frame>, String> {
    let report = FastDecoder::new()
        .decode_stream_resilient(bytes)
        .map_err(|error| error.to_string())?;

    let mut timeline = Vec::with_capacity(report.statuses.len());
    let mut decoded = report.frames.into_iter();
    let mut last_shown: Option<Frame> = None;
    let mut held = 0_usize;
    let mut dropped_before_first_show = 0_usize;

    for (position, status) in report.statuses.iter().enumerate() {
        let note = match status {
            FrameStatus::Shown => "shown",
            FrameStatus::Corrupt => "corrupt, references and contexts discarded",
            FrameStatus::DependencyLost => "dependency lost, not entropy-decoded",
            FrameStatus::RecoveredKeyframe(Recovery::Gap) => "keyframe, recovered after a gap",
            FrameStatus::RecoveredKeyframe(Recovery::LeadingLoss) => {
                "keyframe, recovered after leading loss"
            }
        };
        println!("packet {position:>5}  {note}");

        if status.produced_image() {
            let frame = decoded
                .next()
                .ok_or("the decoder reported an image it did not hand back")?;
            last_shown = Some(frame.clone());
            timeline.push(frame);
        } else if let Some(previous) = last_shown.clone() {
            timeline.push(previous);
            held += 1;
        } else {
            dropped_before_first_show += 1;
        }
    }

    println!(
        "{} packet(s): {} decoded, {held} held on the last shown image, \
         {dropped_before_first_show} before any image existed",
        report.statuses.len(),
        timeline.len() - held
    );
    Ok(timeline)
}

/// Recomputes every checksum and reports each packet, decoding no picture.
///
/// The header checksum is validated by decoding the sequence header, which has
/// already happened by the time this runs. What is left is the packet region,
/// and the scanner reports each outcome without entropy-decoding anything — so
/// this answers "is this file intact" without answering "does it decode", which
/// are different questions and take very different amounts of time.
fn verify(bytes: &[u8]) -> Result<(), String> {
    let mut scanner = PacketScanner::new(&bytes[SEQUENCE_HEADER_SIZE..]);
    let mut intact = 0_usize;
    let mut damaged = 0_usize;
    loop {
        match scanner.next_event() {
            ScanEvent::Packet(packet) => {
                intact += 1;
                println!(
                    "frame {:>5}  {}  qp {:>2}  payload {:>8} bytes  checksums intact",
                    packet.frame_index,
                    if packet.flags.key { "key  " } else { "inter" },
                    packet.frame_qp,
                    packet.payload.len()
                );
            }
            ScanEvent::PayloadCorrupt(header) => {
                damaged += 1;
                println!(
                    "frame {:>5}  {}  qp {:>2}  payload {:>8} bytes  PAYLOAD CHECKSUM FAILED",
                    header.frame_index,
                    if header.flags.key { "key  " } else { "inter" },
                    header.frame_qp,
                    header.payload_len
                );
            }
            ScanEvent::Truncated(header) => {
                damaged += 1;
                println!(
                    "frame {:>5}  {}  qp {:>2}  payload {:>8} bytes  TRUNCATED",
                    header.frame_index,
                    if header.flags.key { "key  " } else { "inter" },
                    header.frame_qp,
                    header.payload_len
                );
            }
            ScanEvent::End => break,
        }
    }

    println!("{intact} intact packet(s), {damaged} damaged");
    if damaged > 0 {
        return Err(format!("{damaged} packet(s) failed verification"));
    }
    if intact == 0 {
        return Err("the packet region holds no packet at all".to_owned());
    }
    Ok(())
}

fn print_header(sequence: &SequenceHeader) {
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
}

struct Options {
    input: String,
    output: Option<String>,
    mode: Mode,
}

impl Options {
    fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut input: Option<String> = None;
        let mut output: Option<String> = None;
        let mut frame: Option<u32> = None;
        let mut seek: Option<u32> = None;
        let mut verify = false;
        let mut tolerate = false;

        let mut index = 0;
        while index < arguments.len() {
            let argument = arguments[index].as_str();
            let value = |flag: &str| -> Result<String, String> {
                arguments
                    .get(index + 1)
                    .cloned()
                    .ok_or_else(|| format!("{flag} needs a value"))
            };
            match argument {
                "--output" => {
                    output = Some(value("--output")?);
                    index += 2;
                }
                "--input" => {
                    input = Some(value("--input")?);
                    index += 2;
                }
                "--frame" | "--seek" => {
                    let raw = value(argument)?;
                    let parsed = raw
                        .parse::<u32>()
                        .map_err(|_| format!("{argument} wants a frame number, not {raw}"))?;
                    if argument == "--frame" {
                        frame = Some(parsed);
                    } else {
                        seek = Some(parsed);
                    }
                    index += 2;
                }
                "--verify" => {
                    verify = true;
                    index += 1;
                }
                "--tolerate" => {
                    tolerate = true;
                    index += 1;
                }
                "-h" | "--help" => return Err(USAGE.to_owned()),
                _ if argument.starts_with('-') => {
                    return Err(format!("unknown option {argument}\n\n{USAGE}"));
                }
                _ => {
                    if input.is_some() {
                        return Err("only one stream may be decoded at a time".to_owned());
                    }
                    input = Some(argument.to_owned());
                    index += 1;
                }
            }
        }

        // Every combination that would leave the tool guessing is refused by
        // name. A tool that quietly ignored one of two contradictory flags
        // would produce a file nobody asked for and say nothing about it.
        if frame.is_some() && seek.is_some() {
            return Err("--frame and --seek ask for different outputs".to_owned());
        }
        if verify && (frame.is_some() || seek.is_some() || tolerate) {
            return Err("--verify decodes no picture, so it takes no other mode".to_owned());
        }
        if tolerate && (frame.is_some() || seek.is_some()) {
            return Err(
                "--tolerate walks the whole stream, so it cannot start at one frame".to_owned(),
            );
        }

        let mode = if verify {
            Mode::Verify
        } else if tolerate {
            Mode::Tolerate
        } else if let Some(index) = frame {
            Mode::Single(index)
        } else if let Some(index) = seek {
            Mode::From(index)
        } else {
            Mode::Whole
        };

        if output.is_none() && !matches!(mode, Mode::Verify) {
            return Err(format!("missing --output\n\n{USAGE}"));
        }

        Ok(Self {
            input: input.ok_or_else(|| format!("no stream given\n\n{USAGE}"))?,
            output,
            mode,
        })
    }
}

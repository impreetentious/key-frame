//! `kfdec`'s modes, driven as a reader would drive them.
//!
//! Three of the four exist because the library has had the capability since the
//! error matrix was written and the tool exposed none of it: random access, a
//! checksum report, and a walk through a damaged stream. The fourth is the
//! whole-stream decode that was always there, kept here as the answer the other
//! three are checked against.
//!
//! The damaged-stream case is the one that carries a rule of its own. The
//! normative document says a frame that did not decode repeats the last shown
//! image for display, and that nothing is repeated before the first image
//! exists. `decode_stream_resilient` deliberately hands back only the images it
//! decoded, so that rule lives in the tool that writes a display file — and
//! until it did, nothing in this repository implemented a sentence the
//! specification states.

use std::{fs, path::PathBuf, process::Command};

use kf_bitstream::{FRAME_HEADER_SIZE, SEQUENCE_HEADER_SIZE, SequenceHeader};
use kf_enc::Encoder;
use kf_frame::Frame;
use kf_tools::{Y4mStream, decode_y4m};

const WIDTH: u16 = 64;
const HEIGHT: u16 = 64;
/// Keys at frames 0, 3, and 6, so a mid-stream keyframe is always a recovery
/// point and a seek always has somewhere earlier to start from.
const KEY_INTERVAL: u16 = 3;
const FRAME_COUNT: usize = 7;

fn source_frames() -> Vec<Frame> {
    (0..FRAME_COUNT)
        .map(|index| {
            let mut frame = Frame::filled_420(u32::from(WIDTH), u32::from(HEIGHT), 0).unwrap();
            let shift = u32::try_from(index).unwrap() * 3;
            for y in 0..u32::from(HEIGHT) {
                for x in 0..u32::from(WIDTH) {
                    frame
                        .y
                        .set(x, y, u8::try_from((x + shift) * 2 % 256).unwrap())
                        .unwrap();
                }
            }
            frame
        })
        .collect()
}

fn clean_stream() -> Vec<u8> {
    let sequence = SequenceHeader::new(WIDTH, HEIGHT, 24, 1, KEY_INTERVAL, 16).unwrap();
    Encoder::new(sequence, 28)
        .unwrap()
        .encode(&source_frames())
        .unwrap()
        .bytes
}

fn packet_offsets(bytes: &[u8]) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut offset = SEQUENCE_HEADER_SIZE;
    while offset + FRAME_HEADER_SIZE <= bytes.len() {
        if &bytes[offset..offset + 4] != b"KFP1" {
            offset += 1;
            continue;
        }
        let payload_len =
            u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        offsets.push(offset);
        offset += FRAME_HEADER_SIZE + payload_len;
    }
    offsets
}

/// A scratch directory of this test's own, so parallel tests cannot collide.
fn scratch(label: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("key-frame-kfdec-{label}-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("a scratch directory");
    directory
}

struct Run {
    status: i32,
    stdout: String,
}

fn kfdec(arguments: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_kfdec"))
        .args(arguments)
        .output()
        .expect("kfdec runs");
    Run {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
    }
}

fn frames_of(path: &std::path::Path) -> Y4mStream {
    decode_y4m(&fs::read(path).expect("the tool wrote a file")).expect("it wrote Y4M")
}

#[test]
fn a_single_frame_matches_the_same_frame_of_a_whole_decode() {
    let directory = scratch("single");
    let stream = directory.join("clean.kfv");
    fs::write(&stream, clean_stream()).unwrap();

    let whole = directory.join("whole.y4m");
    assert_eq!(
        kfdec(&[
            stream.to_str().unwrap(),
            "--output",
            whole.to_str().unwrap()
        ])
        .status,
        0
    );
    let all = frames_of(&whole);
    assert_eq!(all.frames.len(), FRAME_COUNT);

    // Frame five is governed by the keyframe at three, so reaching it exercises
    // the entry point rather than falling out of a decode that started at zero.
    let one = directory.join("one.y4m");
    let run = kfdec(&[
        stream.to_str().unwrap(),
        "--frame",
        "5",
        "--output",
        one.to_str().unwrap(),
    ]);
    assert_eq!(run.status, 0, "{}", run.stdout);
    assert!(
        run.stdout.contains("reached from keyframe 3"),
        "the entry point was not reported: {}",
        run.stdout
    );

    let single = frames_of(&one);
    assert_eq!(single.frames.len(), 1);
    assert_eq!(single.frames[0], all.frames[5]);

    fs::remove_dir_all(&directory).ok();
}

#[test]
fn seeking_writes_the_tail_of_the_same_decode() {
    let directory = scratch("seek");
    let stream = directory.join("clean.kfv");
    fs::write(&stream, clean_stream()).unwrap();

    let whole = directory.join("whole.y4m");
    kfdec(&[
        stream.to_str().unwrap(),
        "--output",
        whole.to_str().unwrap(),
    ]);
    let all = frames_of(&whole);

    let tail = directory.join("tail.y4m");
    let run = kfdec(&[
        stream.to_str().unwrap(),
        "--seek",
        "4",
        "--output",
        tail.to_str().unwrap(),
    ]);
    assert_eq!(run.status, 0, "{}", run.stdout);
    let from_four = frames_of(&tail);
    assert_eq!(from_four.frames, all.frames[4..].to_vec());

    fs::remove_dir_all(&directory).ok();
}

#[test]
fn verification_reports_every_packet_and_decodes_none() {
    let directory = scratch("verify");
    let stream = directory.join("clean.kfv");
    let bytes = clean_stream();
    fs::write(&stream, &bytes).unwrap();

    let run = kfdec(&[stream.to_str().unwrap(), "--verify"]);
    assert_eq!(run.status, 0, "{}", run.stdout);
    assert_eq!(
        run.stdout.matches("checksums intact").count(),
        packet_offsets(&bytes).len()
    );
    assert!(run.stdout.contains("0 damaged"), "{}", run.stdout);

    // A payload checksum broken in place: the header still admits the packet,
    // so this is the case a header-only scan would call intact.
    let mut damaged = bytes.clone();
    let offsets = packet_offsets(&damaged);
    damaged[offsets[2] + FRAME_HEADER_SIZE] ^= 0xFF;
    let broken = directory.join("broken.kfv");
    fs::write(&broken, &damaged).unwrap();

    let run = kfdec(&[broken.to_str().unwrap(), "--verify"]);
    assert_ne!(run.status, 0, "a damaged stream verified clean");
    assert!(
        run.stdout.contains("PAYLOAD CHECKSUM FAILED"),
        "{}",
        run.stdout
    );

    fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_damaged_stream_is_refused_unless_it_is_tolerated() {
    let directory = scratch("tolerate");
    let mut damaged = clean_stream();
    let offsets = packet_offsets(&damaged);
    // Frame four's payload, which is neither a keyframe nor the first frame:
    // it goes corrupt, frame five loses its dependency, and frame six is a
    // keyframe that recovers.
    damaged[offsets[4] + FRAME_HEADER_SIZE + 2] ^= 0xFF;
    let stream = directory.join("damaged.kfv");
    fs::write(&stream, &damaged).unwrap();

    let refused = directory.join("refused.y4m");
    let run = kfdec(&[
        stream.to_str().unwrap(),
        "--output",
        refused.to_str().unwrap(),
    ]);
    assert_ne!(
        run.status, 0,
        "a damaged stream decoded as though it were not"
    );
    assert!(!refused.exists(), "a refused decode still wrote a file");

    let held = directory.join("held.y4m");
    let run = kfdec(&[
        stream.to_str().unwrap(),
        "--tolerate",
        "--output",
        held.to_str().unwrap(),
    ]);
    assert_eq!(run.status, 0, "{}", run.stdout);
    assert!(run.stdout.contains("corrupt"), "{}", run.stdout);

    // The display rule: one frame per accepted packet, because an image existed
    // before the damage. A decoder that dropped the lost frames would hand a
    // player a clip that runs short and early.
    let shown = frames_of(&held);
    assert_eq!(
        shown.frames.len(),
        offsets.len(),
        "the display timeline is not one frame per packet: {}",
        run.stdout
    );

    // And the hold is a repeat rather than an invention: the frames on either
    // side of the damage are the last one that decoded.
    assert_eq!(shown.frames[4], shown.frames[3]);
    assert_eq!(shown.frames[5], shown.frames[3]);
    assert_ne!(shown.frames[6], shown.frames[3]);

    fs::remove_dir_all(&directory).ok();
}

#[test]
fn contradictory_modes_are_refused_by_name() {
    let directory = scratch("modes");
    let stream = directory.join("clean.kfv");
    fs::write(&stream, clean_stream()).unwrap();
    let out = directory.join("out.y4m");
    let path = stream.to_str().unwrap();
    let output = out.to_str().unwrap();

    for arguments in [
        vec![path, "--frame", "1", "--seek", "2", "--output", output],
        vec![path, "--verify", "--tolerate"],
        vec![path, "--tolerate", "--frame", "1", "--output", output],
        vec![path, "--frame", "not-a-number", "--output", output],
        vec![path, "--nonsense", "--output", output],
        vec![path],
    ] {
        let run = kfdec(&arguments);
        assert_ne!(run.status, 0, "{arguments:?} was accepted");
    }

    fs::remove_dir_all(&directory).ok();
}

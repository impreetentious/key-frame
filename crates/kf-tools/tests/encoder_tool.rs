//! `kfenc --stats`, the receipt an encode leaves behind.
//!
//! The per-frame table has always shown what each frame cost. What it could not
//! show is what those bytes bought, because quality is a comparison against the
//! decoded picture — and an encoder produces a reconstruction, not a decode.
//! The two are required to be identical, which is exactly why measuring the
//! reconstruction would be wrong: the one bug that matters most would become
//! invisible in the figures a reader would trust.

use std::{fs, path::PathBuf, process::Command};

use kf_dec::FastDecoder;
use kf_tools::{Json, decode_y4m, psnr_y, sha256_hex};

mod common;
use common::scratch;

/// A deterministic clip, written as Y4M for the tool to read.
fn source(directory: &std::path::Path) -> PathBuf {
    let path = directory.join("source.y4m");
    let mut bytes = b"YUV4MPEG2 W64 H64 F24:1 Ip A0:0 C420jpeg\n".to_vec();
    for index in 0..6_u32 {
        bytes.extend_from_slice(b"FRAME\n");
        for y in 0..64_u32 {
            for x in 0..64_u32 {
                bytes.push(u8::try_from((x * 3 + y * 2 + index * 7) % 251).unwrap());
            }
        }
        bytes.extend_from_slice(&vec![128_u8; 64 * 64 / 2]);
    }
    fs::write(&path, bytes).expect("the clip is writable");
    path
}

fn kfenc(arguments: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_kfenc"))
        .args(arguments)
        .output()
        .expect("kfenc runs");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

#[test]
fn the_receipt_records_every_figure_the_table_prints() {
    let directory = scratch("receipt");
    let clip = source(&directory);
    let stream = directory.join("out.kfv");
    let stats = directory.join("stats.json");

    let (status, stdout) = kfenc(&[
        "--input",
        clip.to_str().unwrap(),
        "--qp",
        "28",
        "--output",
        stream.to_str().unwrap(),
        "--stats",
        stats.to_str().unwrap(),
    ]);
    assert_eq!(status, 0, "{stdout}");
    // With --stats the table carries the quality column too, because the
    // decode that makes it measurable has already happened.
    assert_eq!(stdout.matches("psnr").count(), 6, "{stdout}");

    let document =
        Json::parse(&fs::read_to_string(&stats).expect("the receipt exists")).expect("it is JSON");
    assert_eq!(
        document.get("format").and_then(Json::as_str),
        Some("key-frame-encode-stats-v1")
    );

    // The source is named by content, because a path is a fact about one
    // machine and the point of a receipt is that someone else can use it.
    let source_bytes = fs::read(&clip).unwrap();
    assert_eq!(
        document.get("source_sha256").and_then(Json::as_str),
        Some(sha256_hex(&source_bytes).as_str())
    );
    let coded = fs::read(&stream).unwrap();
    assert_eq!(
        document
            .get("stream")
            .and_then(|stream| stream.get("sha256"))
            .and_then(Json::as_str),
        Some(sha256_hex(&coded).as_str())
    );

    // And the figures are the ones a fresh decode produces, not the encoder's
    // own reconstruction.
    let clip_frames = decode_y4m(&source_bytes).unwrap().frames;
    let decoded = FastDecoder::new().decode_stream(&coded).unwrap();
    let quality = psnr_y(&clip_frames, &decoded).unwrap();

    let frames = document
        .get("frames")
        .and_then(Json::as_array)
        .expect("the receipt lists frames");
    assert_eq!(frames.len(), 6);
    for (position, frame) in frames.iter().enumerate() {
        let recorded = frame
            .get("psnr_y")
            .and_then(Json::as_f64)
            .unwrap_or_else(|| panic!("frame {position} records no psnr_y"));
        let expected = quality.per_frame[position];
        assert!(
            (recorded - expected).abs() <= 1e-9 * recorded.abs().max(expected.abs()).max(1.0),
            "frame {position}: receipt {recorded}, fresh decode {expected}"
        );
    }
    let global = document
        .get("quality")
        .and_then(|quality| quality.get("psnr_y"))
        .and_then(Json::as_f64)
        .expect("a clip figure");
    assert!((global - quality.global).abs() <= 1e-9 * global.abs().max(1.0));

    fs::remove_dir_all(&directory).ok();
}

#[test]
fn without_the_flag_nothing_is_decoded_and_nothing_is_written() {
    // The decode costs real time on a long clip, so it is a flag rather than
    // the default, and the table says only what it can say without one.
    let directory = scratch("plain");
    let clip = source(&directory);
    let stream = directory.join("out.kfv");

    let (status, stdout) = kfenc(&[
        "--input",
        clip.to_str().unwrap(),
        "--output",
        stream.to_str().unwrap(),
    ]);
    assert_eq!(status, 0, "{stdout}");
    assert!(!stdout.contains("psnr"), "{stdout}");
    assert!(!directory.join("stats.json").exists());

    fs::remove_dir_all(&directory).ok();
}

#[test]
fn an_average_bitrate_encode_records_the_target_it_was_given() {
    let directory = scratch("abr");
    let clip = source(&directory);
    let stream = directory.join("out.kfv");
    let stats = directory.join("stats.json");

    let (status, stdout) = kfenc(&[
        "--input",
        clip.to_str().unwrap(),
        "--bitrate",
        "200000",
        "--output",
        stream.to_str().unwrap(),
        "--stats",
        stats.to_str().unwrap(),
    ]);
    assert_eq!(status, 0, "{stdout}");

    let document =
        Json::parse(&fs::read_to_string(&stats).expect("the receipt exists")).expect("it is JSON");
    let settings = document.get("settings").expect("settings");
    assert_eq!(
        settings.get("bitrate_bps").and_then(Json::as_f64),
        Some(200_000.0)
    );
    assert!(settings.get("qp").is_none(), "an ABR encode recorded a QP");

    fs::remove_dir_all(&directory).ok();
}

use std::{fs, process::Command};

use kf_frame::Frame;
use kf_tools::{Y4mStream, decode_y4m, encode_y4m};

mod common;
use common::scratch;

#[test]
fn command_line_encode_decode_and_probe_round_trip() {
    let test_dir = scratch("cli");
    let input = test_dir.join("input.y4m");
    let stream_path = test_dir.join("output.kfv");
    let decoded_path = test_dir.join("decoded.y4m");
    let source = Y4mStream {
        width: 64,
        height: 64,
        fps_num: 24,
        fps_den: 1,
        frames: vec![Frame::filled_420(64, 64, 128).unwrap()],
    };
    fs::write(&input, encode_y4m(&source).unwrap()).unwrap();

    let encoded = Command::new(env!("CARGO_BIN_EXE_kfenc"))
        .args([
            "--input",
            input.to_str().unwrap(),
            "--output",
            stream_path.to_str().unwrap(),
            "--qp",
            "28",
        ])
        .output()
        .unwrap();
    assert!(
        encoded.status.success(),
        "{}",
        String::from_utf8_lossy(&encoded.stderr)
    );
    assert!(String::from_utf8_lossy(&encoded.stdout).contains("encoded 1 frame(s)"));

    let decoded = Command::new(env!("CARGO_BIN_EXE_kfdec"))
        .args([
            stream_path.to_str().unwrap(),
            "--output",
            decoded_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        decoded.status.success(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    assert_eq!(
        decode_y4m(&fs::read(&decoded_path).unwrap()).unwrap(),
        source
    );

    let probed = Command::new(env!("CARGO_BIN_EXE_kfprobe"))
        .arg(&stream_path)
        .output()
        .unwrap();
    assert!(
        probed.status.success(),
        "{}",
        String::from_utf8_lossy(&probed.stderr)
    );
    let report = String::from_utf8(probed.stdout).unwrap();
    assert!(report.contains("\"canonical_payload_match\":true"));
    assert!(report.contains("\"probe_version\":1"));

    fs::remove_dir_all(test_dir).unwrap();
}

#[test]
fn command_line_bitrate_and_qp_are_exclusive() {
    let test_dir = scratch("cli-abr");
    let input = test_dir.join("input.y4m");
    let stream_path = test_dir.join("output.kfv");
    let source = Y4mStream {
        width: 64,
        height: 64,
        fps_num: 24,
        fps_den: 1,
        frames: vec![
            Frame::filled_420(64, 64, 80).unwrap(),
            Frame::filled_420(64, 64, 96).unwrap(),
        ],
    };
    fs::write(&input, encode_y4m(&source).unwrap()).unwrap();

    let both = Command::new(env!("CARGO_BIN_EXE_kfenc"))
        .args([
            "--input",
            input.to_str().unwrap(),
            "--output",
            stream_path.to_str().unwrap(),
            "--qp",
            "28",
            "--bitrate",
            "80000",
        ])
        .output()
        .unwrap();
    assert!(!both.status.success());
    assert!(
        String::from_utf8_lossy(&both.stderr).contains("mutually exclusive"),
        "{}",
        String::from_utf8_lossy(&both.stderr)
    );

    let first = Command::new(env!("CARGO_BIN_EXE_kfenc"))
        .args([
            "--input",
            input.to_str().unwrap(),
            "--output",
            stream_path.to_str().unwrap(),
            "--bitrate",
            "80000",
        ])
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_bytes = fs::read(&stream_path).unwrap();
    let second = Command::new(env!("CARGO_BIN_EXE_kfenc"))
        .args([
            "--input",
            input.to_str().unwrap(),
            "--output",
            stream_path.to_str().unwrap(),
            "--bitrate",
            "80000",
        ])
        .output()
        .unwrap();
    assert!(second.status.success());
    assert_eq!(first_bytes, fs::read(&stream_path).unwrap());

    fs::remove_dir_all(test_dir).unwrap();
}

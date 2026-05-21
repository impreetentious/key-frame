use kf_bitstream::{FrameFlags, FramePacket};
use kf_tools::probe_stream;

const ORACLE_STREAM: [u8; 54] = [
    0x4b, 0x46, 0x56, 0x31, 0x01, 0x00, 0x40, 0x00, 0x40, 0x00, 0x01, 0x08, 0x18, 0x00, 0x01, 0x00,
    0x78, 0x00, 0x10, 0x00, 0x0e, 0x72, 0xb9, 0x5d, 0x4b, 0x46, 0x50, 0x31, 0x06, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x07, 0x20, 0x00, 0x00, 0x23, 0x0e, 0x00, 0x74, 0x8a, 0x7c, 0x2a, 0x57,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

#[test]
fn oracle_stream_shadow_replays_canonically() {
    let report = probe_stream(&ORACLE_STREAM).unwrap();
    assert!(report.canonical_payload_match);
    assert_eq!(report.input_payload_len, 6);
    assert_eq!(report.canonical_replay_payload_len, 6);
    assert_eq!(report.superblocks.len(), 1);
    assert_eq!(report.superblocks[0].blocks.len(), 1);
    assert_eq!(report.superblocks[0].blocks[0].mode, "dc");
    assert_eq!(report.superblocks[0].blocks[0].dc_energy, 0);
    let accounted = report
        .superblocks
        .iter()
        .map(|superblock| superblock.structure_emitted_payload_bytes)
        .sum::<u64>()
        + report
            .superblocks
            .iter()
            .flat_map(|superblock| &superblock.blocks)
            .map(|block| block.emitted_payload_bytes)
            .sum::<u64>()
        + report.frame_flush_bytes;
    assert_eq!(
        accounted,
        u64::try_from(report.canonical_replay_payload_len).unwrap()
    );
}

#[test]
fn json_carries_replay_semantics() {
    let json = probe_stream(&ORACLE_STREAM).unwrap().to_json();
    assert!(json.contains("\"probe_version\":1"));
    assert!(json.contains("\"canonical_payload_match\":true"));
    assert!(json.contains("\"structure_emitted_payload_bytes\""));
    assert!(json.contains("\"prediction\":{\"kind\":\"intra\",\"mode\":\"dc\"}"));
    assert!(json.contains("\"dc_energy\":0"));
}

#[test]
fn valid_noncanonical_tail_reports_mismatch_without_attribution() {
    let mut payload = ORACLE_STREAM[48..].to_vec();
    payload.push(0xa5);
    let packet = FramePacket::new(
        0,
        FrameFlags {
            key: true,
            golden_refresh: true,
            show: true,
        },
        28,
        payload,
    )
    .unwrap();
    let mut stream = ORACLE_STREAM[..24].to_vec();
    stream.extend_from_slice(&packet.encode());

    let report = probe_stream(&stream).unwrap();
    assert!(!report.canonical_payload_match);
    assert_eq!(report.input_payload_len, 7);
    assert_eq!(report.canonical_replay_payload_len, 6);
    assert_eq!(report.first_mismatch_offset, Some(6));
    let attributed = report
        .superblocks
        .iter()
        .map(|superblock| superblock.structure_emitted_payload_bytes)
        .sum::<u64>()
        + report
            .superblocks
            .iter()
            .flat_map(|superblock| &superblock.blocks)
            .map(|block| block.emitted_payload_bytes)
            .sum::<u64>()
        + report.frame_flush_bytes;
    assert_eq!(attributed, 6);
    assert_ne!(attributed, 7);
}

/// `kfprobe --summary`, the rendering the terminal demo used to do in Python.
///
/// The Python read the report's JSON with unchecked dictionary lookups, so a
/// schema change would have surfaced as a traceback in the demo rather than as
/// a failing test. This drives the tool and asserts what a reader is owed: the
/// partition, both accounting figures, and the sentence that keeps them from
/// being added.
mod summary {
    use std::process::Command;

    const STREAM: &str = "../../conformance/encoder/inter_motion64_qp32.kfv";

    fn kfprobe(arguments: &[&str]) -> (i32, String, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_kfprobe"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(arguments)
            .output()
            .expect("kfprobe runs");
        (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }

    #[test]
    fn the_summary_renders_the_partition_and_both_accounting_figures() {
        let (status, stdout, stderr) = kfprobe(&[STREAM, "--summary"]);
        assert_eq!(status, 0, "{stderr}");

        assert!(stdout.starts_with("frame 0, key, qp 32,"), "{stdout}");
        assert!(
            stdout.contains("superblock at (0, 0) partitions to:"),
            "{stdout}"
        );
        // Four leaves, each named with its size, position, and prediction.
        assert_eq!(stdout.matches("32x32 at (").count(), 8, "{stdout}");
        assert!(stdout.contains("intra dc"), "{stdout}");

        // The disclaimer is the point of the rendering, not decoration: a
        // summary that showed both numbers without it would invite the sum.
        assert!(
            stdout.contains("the two quantities never added"),
            "{stdout}"
        );
        assert!(
            stdout.contains("Neither is 'this block's bit count'."),
            "{stdout}"
        );
        assert!(stdout.contains("modeled "), "{stdout}");
        assert!(stdout.contains("emitted "), "{stdout}");
        assert!(stdout.contains("canonical replay"), "{stdout}");
    }

    #[test]
    fn the_modeled_figure_is_the_reports_own_fixed_point_value() {
        // Rendered from Q16.16 without going through a float, so the printed
        // figure is exactly the report's value to two decimals rather than the
        // nearest double to it.
        let report = kf_tools::probe_frame(
            &std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(STREAM))
                .expect("the committed stream is readable"),
            0,
        )
        .expect("it probes");
        let first = &report.superblocks[0].blocks[0];
        let expected = format!(
            "{}.{:02}",
            first.modeled_entropy_q16 >> 16,
            ((first.modeled_entropy_q16 & 0xFFFF) * 100 + (1 << 15)) >> 16
        );

        let (_, stdout, _) = kfprobe(&[STREAM, "--summary"]);
        assert!(
            stdout.contains(&format!("modeled {expected:>8} bits")),
            "expected {expected} in:\n{stdout}"
        );
    }

    #[test]
    fn a_superblock_that_is_not_there_is_named_rather_than_defaulted() {
        let (status, _, stderr) = kfprobe(&[STREAM, "--summary", "--superblock", "9"]);
        assert_eq!(status, 1);
        assert!(stderr.contains("no superblock 9"), "{stderr}");
    }

    #[test]
    fn the_default_output_is_still_the_wire_format() {
        // The projection room and the equality gate both read this. A tool that
        // started printing prose by default would break them at once, which is
        // the good case; asserting it keeps the bad case from being invented.
        let (status, stdout, _) = kfprobe(&[STREAM]);
        assert_eq!(status, 0);
        assert!(stdout.starts_with("{\"probe_version\":1,"), "{stdout}");
    }
}

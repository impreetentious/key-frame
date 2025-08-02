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

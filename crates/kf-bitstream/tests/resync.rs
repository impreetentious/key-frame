use kf_bitstream::{BitstreamError, FrameFlags, FramePacket, PacketScanner};

fn packet(index: u32, key: bool) -> Vec<u8> {
    FramePacket::new(
        index,
        FrameFlags {
            key,
            golden_refresh: key,
            show: true,
        },
        20,
        vec![0, 0, 0, 0, 0],
    )
    .unwrap()
    .encode()
}

#[test]
fn trap_sync_false_positive() {
    let mut bytes = b"noiseKFP1bad-header".to_vec();
    bytes.extend(packet(0, true));
    let mut scanner = PacketScanner::new(&bytes);
    assert_eq!(scanner.next_packet().unwrap().unwrap().frame_index, 0);
}

#[test]
fn trap_payload_crc_consumes_declared_extent() {
    let mut corrupt = packet(0, true);
    let corrupt_extent = corrupt.len();
    *corrupt.last_mut().unwrap() ^= 1;
    let mut bytes = corrupt;
    bytes.extend(packet(1, true));
    let mut scanner = PacketScanner::new(&bytes);
    assert!(matches!(
        scanner.next_packet(),
        Err(BitstreamError::CrcMismatch {
            element: "packet.payload_crc32c",
            ..
        })
    ));
    assert_eq!(scanner.offset(), corrupt_extent);
    assert_eq!(scanner.next_packet().unwrap().unwrap().frame_index, 1);
}

#[test]
fn stale_index_candidate_is_skipped() {
    let mut bytes = packet(2, true);
    bytes.extend(packet(1, true));
    bytes.extend(packet(3, true));
    let mut scanner = PacketScanner::new(&bytes);
    assert_eq!(scanner.next_packet().unwrap().unwrap().frame_index, 2);
    assert_eq!(scanner.next_packet().unwrap().unwrap().frame_index, 3);
}

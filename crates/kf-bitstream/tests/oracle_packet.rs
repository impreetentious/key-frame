use kf_bitstream::{FrameFlags, FramePacket};

const ORACLE_PACKET: [u8; 29] = [
    0x4b, 0x46, 0x50, 0x31, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, 0x20, 0x00, 0x00,
    0x73, 0x72, 0x92, 0x27, 0x35, 0x76, 0x72, 0x45, 0x00, 0x00, 0x00, 0x00, 0x00,
];

#[test]
fn independent_packet_vector_matches_byte_for_byte() {
    let packet = FramePacket::new(
        0,
        FrameFlags {
            key: true,
            golden_refresh: true,
            show: true,
        },
        32,
        vec![0; 5],
    )
    .unwrap();
    assert_eq!(packet.encode(), ORACLE_PACKET);
    let (decoded, extent) = FramePacket::decode(&ORACLE_PACKET).unwrap();
    assert_eq!(decoded, packet);
    assert_eq!(extent, ORACLE_PACKET.len());
}

#[test]
fn header_and_payload_checksums_are_independent() {
    let mut header_corrupt = ORACLE_PACKET;
    header_corrupt[8] ^= 1;
    assert!(FramePacket::decode(&header_corrupt).is_err());

    let mut payload_corrupt = ORACLE_PACKET;
    payload_corrupt[28] ^= 1;
    assert!(FramePacket::decode(&payload_corrupt).is_err());
}

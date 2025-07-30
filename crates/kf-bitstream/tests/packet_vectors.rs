use kf_bitstream::{BitstreamError, FrameFlags, FramePacket, PacketHeader};

fn key_packet(index: u32) -> FramePacket {
    FramePacket::new(
        index,
        FrameFlags {
            key: true,
            golden_refresh: true,
            show: true,
        },
        32,
        vec![0, 0, 0, 0, 0],
    )
    .unwrap()
}

#[test]
fn packet_round_trip_preserves_authoritative_fields() {
    let packet = key_packet(7);
    let encoded = packet.encode();
    let header = PacketHeader::decode(&encoded).unwrap();
    assert_eq!(header.frame_index, 7);
    assert_eq!(header.payload_len, 5);
    assert_eq!(FramePacket::decode(&encoded).unwrap().0, packet);
}

#[test]
fn trap_zero_size_packet() {
    for length in 0..5 {
        assert!(matches!(
            FramePacket::new(
                0,
                FrameFlags {
                    key: true,
                    golden_refresh: true,
                    show: true,
                },
                0,
                vec![0; length]
            ),
            Err(BitstreamError::InvalidField {
                element: "packet.payload_len",
                ..
            })
        ));
    }
}

#[test]
fn trap_frame_qp_range_and_key_flags() {
    let flags = FrameFlags {
        key: true,
        golden_refresh: true,
        show: true,
    };
    assert!(FramePacket::new(0, flags, 64, vec![0; 5]).is_err());
    assert!(
        FramePacket::new(
            0,
            FrameFlags {
                golden_refresh: false,
                ..flags
            },
            0,
            vec![0; 5]
        )
        .is_err()
    );
}

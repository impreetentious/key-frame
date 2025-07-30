use kf_bitstream::{BitstreamError, SequenceHeader};

#[test]
fn oracle_sequence_header_matches_byte_for_byte() {
    let header = SequenceHeader::new(64, 64, 24, 1, 120, 16).unwrap();
    assert_eq!(
        header.encode(),
        [
            0x4b, 0x46, 0x56, 0x31, 0x01, 0x00, 0x40, 0x00, 0x40, 0x00, 0x01, 0x08, 0x18, 0x00,
            0x01, 0x00, 0x78, 0x00, 0x10, 0x00, 0x0e, 0x72, 0xb9, 0x5d,
        ]
    );
    assert_eq!(SequenceHeader::decode(&header.encode()).unwrap(), header);
}

#[test]
fn trap_sequence_zero_fields() {
    for error in [
        SequenceHeader::new(64, 64, 0, 1, 120, 16).unwrap_err(),
        SequenceHeader::new(64, 64, 24, 0, 120, 16).unwrap_err(),
        SequenceHeader::new(64, 64, 24, 1, 0, 16).unwrap_err(),
        SequenceHeader::new(64, 64, 24, 1, 120, 0).unwrap_err(),
    ] {
        assert!(matches!(error, BitstreamError::InvalidField { .. }));
    }
}

#[test]
fn sequence_crc_detects_each_mutated_prefix_byte() {
    let encoded = SequenceHeader::new(320, 240, 30, 1, 60, 8)
        .unwrap()
        .encode();
    for index in 0..20 {
        let mut mutated = encoded;
        mutated[index] ^= 0x80;
        assert!(SequenceHeader::decode(&mutated).is_err(), "byte {index}");
    }
}

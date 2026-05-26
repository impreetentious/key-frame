use kf_bitstream::{BitstreamError, SequenceHeader};
use kf_core::crc32c;

fn header(width: u16, height: u16, chroma: u8, depth: u8) -> Vec<u8> {
    let mut bytes = vec![0_u8; 24];
    bytes[..4].copy_from_slice(b"KFV1");
    bytes[4..6].copy_from_slice(&1_u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&width.to_le_bytes());
    bytes[8..10].copy_from_slice(&height.to_le_bytes());
    bytes[10] = chroma;
    bytes[11] = depth;
    bytes[12..14].copy_from_slice(&24_u16.to_le_bytes());
    bytes[14..16].copy_from_slice(&1_u16.to_le_bytes());
    bytes[16..18].copy_from_slice(&120_u16.to_le_bytes());
    bytes[18] = 16;
    let checksum = crc32c(&bytes[..20]);
    bytes[20..24].copy_from_slice(&checksum.to_le_bytes());
    bytes
}

#[test]
fn probe_offsets() {
    for (label, bytes) in [
        ("bad height (declared at 8)", header(64, 65, 1, 8)),
        ("bad width  (declared at 6)", header(65, 64, 1, 8)),
        ("bad depth  (declared at 11)", header(64, 64, 1, 9)),
        ("bad chroma (declared at 10)", header(64, 64, 2, 8)),
    ] {
        match SequenceHeader::decode(&bytes) {
            Err(BitstreamError::InvalidField { offset, element }) => {
                println!("{label}: reported offset {offset}, element {element}");
            }
            other => println!("{label}: {other:?}"),
        }
    }
}

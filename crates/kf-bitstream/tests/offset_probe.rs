//! Every malformed sequence-header field is refused at the byte it is declared at.
//!
//! An error that carries a byte offset carries it so a reader can find the
//! field that was wrong. Naming the wrong field is worse than naming none: it
//! sends whoever is holding a hex dump to two bytes that are fine.
//!
//! This file used to print what the decoder reported and assert nothing. It
//! passed whatever came back, and what came back for a bad height was offset 6
//! — the two bytes the width occupies — under the name `sequence.dimensions`.
//! The label beside each case in the old loop said "declared at 8". The test
//! printed both numbers on every run and cargo swallowed the output, because a
//! passing test's stdout is not shown.
//!
//! The offsets below are read from `fields.toml` rather than written here, so
//! this cannot become a second copy of the layout it exists to check.

use kf_bitstream::{BitstreamError, SequenceHeader};
use kf_core::crc32c;
use kf_spec::V1_ASSETS;

/// The byte offset `fields.toml` declares for one sequence-header field.
fn declared_offset(field: &str) -> u32 {
    let contents = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "fields.toml")
        .expect("invariant: kf-spec exposes fields.toml")
        .contents;
    let sequence = contents
        .split_once("[sequence]")
        .expect("invariant: fields.toml declares a sequence header")
        .1
        .split_once("[packet]")
        .map_or(contents, |(before, _)| before);
    let needle = format!("\"{field}:");
    sequence
        .lines()
        .find_map(|line| {
            let entry = line.trim().strip_prefix(&needle)?;
            entry
                .split_once('@')?
                .1
                .trim_end_matches("\",")
                .parse()
                .ok()
        })
        .unwrap_or_else(|| panic!("fields.toml declares no sequence field {field}"))
}

/// A well-formed header, with each field settable so one can be spoiled.
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

/// A header with one field zeroed and its checksum repaired.
fn zeroed(field_offset: usize, width: usize) -> Vec<u8> {
    let mut bytes = header(64, 64, 1, 8);
    bytes[field_offset..field_offset + width].fill(0);
    let checksum = crc32c(&bytes[..20]);
    bytes[20..24].copy_from_slice(&checksum.to_le_bytes());
    bytes
}

fn refused_at(bytes: &[u8]) -> (u32, &'static str) {
    match SequenceHeader::decode(bytes) {
        Err(BitstreamError::InvalidField { offset, element }) => (offset, element),
        other => panic!("expected an invalid-field error, got {other:?}"),
    }
}

#[test]
fn every_malformed_field_is_refused_at_its_declared_offset() {
    // Each case spoils exactly one field, so the offset the decoder reports has
    // only one field it could correctly name.
    let cases: [(&str, Vec<u8>); 10] = [
        ("width", header(62, 64, 1, 8)),
        ("width", header(4098, 64, 1, 8)),
        ("width", header(65, 64, 1, 8)),
        ("height", header(64, 62, 1, 8)),
        ("height", header(64, 2306, 1, 8)),
        ("height", header(64, 65, 1, 8)),
        ("fps_num", zeroed(12, 2)),
        ("fps_den", zeroed(14, 2)),
        ("kf_interval", zeroed(16, 2)),
        ("golden_interval", zeroed(18, 1)),
    ];
    for (field, bytes) in cases {
        let (offset, element) = refused_at(&bytes);
        assert!(
            element.ends_with(field),
            "a bad {field} was reported as {element}"
        );
        assert_eq!(
            offset,
            declared_offset(field),
            "a bad {field} was reported at byte {offset}, and {field} is declared elsewhere"
        );
    }
}

/// The two format fields are refused where they are declared, one byte apart.
///
/// These are checked before the header is constructed rather than inside
/// `validate`, so they take a different path from the fields above and are
/// worth driving separately.
#[test]
fn the_sample_format_fields_are_refused_at_their_own_bytes() {
    for (field, bytes) in [
        ("chroma", header(64, 64, 2, 8)),
        ("depth", header(64, 64, 1, 9)),
    ] {
        let (offset, element) = refused_at(&bytes);
        assert!(
            element.ends_with(field),
            "a bad {field} was reported as {element}"
        );
        assert_eq!(
            offset,
            declared_offset(field),
            "a bad {field} was misplaced"
        );
    }
}

/// The declaration this test reads is the one the decoder is being held to.
///
/// Without this, a `fields.toml` that lost its sequence layout would make every
/// assertion above compare a reported offset against a panic that never
/// happened — or, worse, against a silently wrong parse.
#[test]
fn the_declared_layout_is_the_one_the_header_is_written_in() {
    assert_eq!(declared_offset("magic"), 0);
    assert_eq!(declared_offset("version"), 4);
    assert!(
        declared_offset("height") > declared_offset("width"),
        "the two dimensions cannot share a byte"
    );
    assert!(
        declared_offset("depth") > declared_offset("chroma"),
        "the two format fields cannot share a byte"
    );
}

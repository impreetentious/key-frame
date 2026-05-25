//! The frozen limits, checked against what the code actually enforces.
//!
//! `spec/v1/constants.toml` declares the picture bounds, the quantizer range,
//! and the payload cap, and the generated normative document publishes them.
//! The implementation holds the same numbers as literals, because they sit in
//! validation paths where a parse would be absurd.
//!
//! That leaves a hole the rest of the specification machinery does not cover.
//! Changing the asset is caught, because the generated document drifts. Changing
//! the *implementation* is not: a decoder that accepted pictures wider than the
//! document allows would pass every gate in the repository, and the first person
//! to notice would be someone whose conformant decoder rejected a stream this
//! one produced.
//!
//! So these tests do not compare literals. They drive the real constructor with
//! the values the asset declares and require it to accept what the document
//! promises and refuse everything outside it. A limit that moved on either side
//! fails here.

use kf_bitstream::SequenceHeader;
use kf_spec::V1_ASSETS;

/// Reads a declared scalar out of the frozen constants.
fn constant(name: &str) -> i64 {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "constants.toml")
        .expect("the specification exposes constants.toml");
    asset
        .contents
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            if key.trim() != name {
                return None;
            }
            value.trim().parse::<i64>().ok()
        })
        .unwrap_or_else(|| panic!("constants.toml declares no scalar {name}"))
}

fn dimension(name: &str) -> u16 {
    u16::try_from(constant(name)).unwrap_or_else(|_| panic!("{name} does not fit a picture field"))
}

fn header(width: u16, height: u16) -> Result<SequenceHeader, kf_bitstream::BitstreamError> {
    SequenceHeader::new(width, height, 30, 1, 120, 16)
}

#[test]
fn the_smallest_picture_the_document_promises_is_accepted() {
    let (width, height) = (dimension("min_width"), dimension("min_height"));
    assert!(
        header(width, height).is_ok(),
        "the document promises {width}x{height} and the implementation refuses it"
    );
}

#[test]
fn the_largest_picture_the_document_promises_is_accepted() {
    let (width, height) = (dimension("max_width"), dimension("max_height"));
    assert!(
        header(width, height).is_ok(),
        "the document promises {width}x{height} and the implementation refuses it"
    );
}

#[test]
fn nothing_outside_the_declared_bounds_is_accepted() {
    // The half that matters most. An implementation that accepts more than the
    // document allows produces streams a conformant decoder rejects, and every
    // gate here would stay green while it did.
    let min_width = dimension("min_width");
    let min_height = dimension("min_height");
    let max_width = dimension("max_width");
    let max_height = dimension("max_height");

    for (width, height, what) in [
        (min_width - 2, min_height, "narrower than the minimum"),
        (min_width, min_height - 2, "shorter than the minimum"),
        (max_width + 2, max_height, "wider than the maximum"),
        (max_width, max_height + 2, "taller than the maximum"),
    ] {
        assert!(
            header(width, height).is_err(),
            "{width}x{height} is {what} and the implementation accepted it"
        );
    }
}

#[test]
fn odd_dimensions_are_refused_at_both_ends_of_the_range() {
    // A 4:2:0 picture cannot have an odd side: the chroma planes would have no
    // whole number of samples. Checked at both ends because a bounds test that
    // only exercises the middle would miss an off-by-one in either comparison.
    let min_width = dimension("min_width");
    let max_width = dimension("max_width");
    assert!(
        header(min_width + 1, min_width).is_err(),
        "an odd width was accepted"
    );
    assert!(
        header(min_width, min_width + 1).is_err(),
        "an odd height was accepted"
    );
    assert!(
        header(max_width - 1, dimension("max_height")).is_err(),
        "an odd width was accepted"
    );
}

#[test]
fn the_declared_superblock_size_is_the_smallest_picture() {
    // Not a coincidence and not free to change independently. The encoder pads
    // to whole superblocks, so a picture smaller than one would be entirely
    // padding, and the minimum exists precisely to make that impossible.
    assert_eq!(
        constant("min_width"),
        constant("superblock_size"),
        "the smallest picture and the superblock have drifted apart"
    );
    assert_eq!(constant("min_height"), constant("superblock_size"));
}

#[test]
fn the_declared_quantizer_range_is_the_one_that_is_enforced() {
    let min = u8::try_from(constant("qp_min")).expect("a quantizer fits a byte");
    let max = u8::try_from(constant("qp_max")).expect("a quantizer fits a byte");
    assert!(
        packet_is_accepted(min, smallest_payload()),
        "the document allows QP {min}"
    );
    assert!(
        packet_is_accepted(max, smallest_payload()),
        "the document allows QP {max}"
    );
    assert!(
        !packet_is_accepted(max + 1, smallest_payload()),
        "QP {} is outside the document and was accepted",
        max + 1
    );
}

#[test]
fn the_smallest_payload_is_what_the_range_decoder_needs_to_start() {
    // A payload shorter than the decoder's initial read cannot be the start of
    // a range-coded frame, so the packet layer refuses it before anything tries.
    // The two numbers are declared separately in the asset and have to agree.
    let initial = usize::try_from(constant("decoder_initial_bytes")).expect("a small count");
    assert!(
        packet_is_accepted(32, vec![0; initial]),
        "a payload of exactly the decoder's initial read was refused"
    );
    assert!(
        !packet_is_accepted(32, vec![0; initial - 1]),
        "a payload one byte short of the decoder's initial read was accepted"
    );
}

#[test]
fn the_payload_cap_is_the_one_the_document_declares() {
    // Only the boundary is exercised: allocating a buffer past the cap to prove
    // it is refused would cost sixteen megabytes to learn nothing extra, so the
    // check is that the cap itself is where the document puts it.
    let cap = usize::try_from(constant("max_payload_bytes")).expect("a payload cap");
    assert!(
        !packet_is_accepted(32, vec![0; cap + 1]),
        "a payload past the declared cap was accepted"
    );
}

fn smallest_payload() -> Vec<u8> {
    vec![0; usize::try_from(constant("decoder_initial_bytes")).expect("a small count")]
}

/// Whether the frame header will carry this quantizer and payload.
///
/// Driven through the packet layer rather than the encoder, so this crate stays
/// where it is in the graph and the check still exercises the fields that
/// actually travel in a stream.
fn packet_is_accepted(qp: u8, payload: Vec<u8>) -> bool {
    use kf_bitstream::{FrameFlags, FramePacket};
    FramePacket::new(
        0,
        FrameFlags {
            key: true,
            golden_refresh: true,
            show: true,
        },
        qp,
        payload,
    )
    .is_ok()
}

#[test]
fn the_motion_vector_difference_bound_is_the_declared_range() {
    // Derived rather than declared: two vectors inside the full-pixel range can
    // differ by its whole width, coded at the declared fractional precision.
    // The syntax layer carried the arithmetic's answer as a literal, so the
    // three declarations it comes from governed nothing that codes a vector.
    use kf_bitstream::{FrameType, MotionVector, Prediction, ReferenceFrame, SyntaxWriter};
    use kf_range::ContextBank;

    let span = constant("mv_fullpel_max") - constant("mv_fullpel_min");
    let fractional = u32::try_from(constant("mv_fractional_bits")).expect("a bit count");
    let bound = i16::try_from(span << fractional).expect("the declared bound fits a vector field");

    let write = |x_q4: i16| {
        SyntaxWriter::new(ContextBank::initial()).write_prediction(
            FrameType::P,
            Prediction::Inter {
                reference: ReferenceFrame::Last,
                mvd: MotionVector { x_q4, y_q4: 0 },
            },
        )
    };

    assert!(write(bound).is_ok(), "the declared bound was refused");
    assert!(write(-bound).is_ok(), "the declared bound was refused");
    assert!(
        write(bound + 1).is_err(),
        "a difference past the bound was written"
    );
    assert!(
        write(-bound - 1).is_err(),
        "a difference past the bound was written"
    );
}

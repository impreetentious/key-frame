//! The corruption and error matrix.
//!
//! Every case damages a real encoded stream and asserts the normative outcome
//! in *both* decoders. The two implementations classify damage independently,
//! so agreement here is evidence about the rules rather than about one
//! decoder's habits.
//!
//! The invariant underneath all of it: no corrupt, held, or dependency-lost
//! image or context state may ever reach later prediction.

use kf_bitstream::{BitstreamError, FRAME_HEADER_SIZE, SEQUENCE_HEADER_SIZE, SequenceHeader};
use kf_core::crc32c;
use kf_dec::{DecodeError, FastDecoder, FrameStatus, Recovery};
use kf_enc::Encoder;
use kf_frame::Frame;
use kf_ref::{RefFrameStatus, RefRecovery, ReferenceDecoder};

const WIDTH: u16 = 64;
const HEIGHT: u16 = 64;
/// Keys at frame 0, 3, and 6, so a mid-stream keyframe is always available as a
/// recovery point.
const KEY_INTERVAL: u16 = 3;
const FRAME_COUNT: usize = 7;

/// A moving gradient: enough real content that P-frames carry motion and
/// residual rather than degenerating to all-skip.
fn source_frames() -> Vec<Frame> {
    (0..FRAME_COUNT)
        .map(|index| {
            let mut frame = Frame::filled_420(u32::from(WIDTH), u32::from(HEIGHT), 0).unwrap();
            let shift = u32::try_from(index).unwrap() * 3;
            for y in 0..u32::from(HEIGHT) {
                for x in 0..u32::from(WIDTH) {
                    let value = u8::try_from((x + shift) * 2 % 256).unwrap();
                    frame.y.set(x, y, value).unwrap();
                }
            }
            frame
        })
        .collect()
}

/// The byte offset `fields.toml` declares for one sequence-header field.
fn declared_offset(field: &str) -> u32 {
    let contents = kf_spec::V1_ASSETS
        .iter()
        .find(|asset| asset.name == "fields.toml")
        .expect("the specification exposes fields.toml")
        .contents;
    let sequence = contents
        .split_once("[sequence]")
        .expect("fields.toml declares a sequence header")
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

/// A sequence header with one field spoiled, sealed so only that field is wrong.
fn spoiled_header(width: u16, height: u16, chroma: u8, depth: u8) -> Vec<u8> {
    let mut bytes = vec![0_u8; SEQUENCE_HEADER_SIZE];
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
    let crc = crc32c(&bytes[..20]);
    bytes[20..24].copy_from_slice(&crc.to_le_bytes());
    bytes
}

/// Both decoders name the same byte, and it is the byte the field is declared at.
///
/// The two implementations classify damage independently, and the matrix below
/// compares the frame statuses they reach. What nothing compared was the offset
/// and field name each one puts in its error — and they disagreed about six
/// fields. This decoder reported a bad height at the width's byte; the
/// reference decoder did too, and additionally collapsed depth onto chroma's
/// byte, both frame rates and the keyframe interval onto byte 12, the sequence
/// flags onto the golden interval's byte, and the packet's reserved field onto
/// the quantizer's. An offset in an error exists so a reader can find the field.
#[test]
fn both_decoders_refuse_a_spoiled_field_at_the_byte_it_is_declared_at() {
    let cases: [(&str, Vec<u8>); 6] = [
        ("width", spoiled_header(62, 64, 1, 8)),
        ("width", spoiled_header(65, 64, 1, 8)),
        ("height", spoiled_header(64, 2306, 1, 8)),
        ("height", spoiled_header(64, 65, 1, 8)),
        ("chroma", spoiled_header(64, 64, 2, 8)),
        ("depth", spoiled_header(64, 64, 1, 9)),
    ];
    for (field, bytes) in cases {
        let declared = declared_offset(field);

        let fast = match FastDecoder::new().decode_stream(&bytes) {
            Err(DecodeError::Bitstream(BitstreamError::InvalidField { offset, element })) => {
                (offset, element)
            }
            other => panic!("fast decoder on a bad {field}: {other:?}"),
        };
        let reference = match ReferenceDecoder::new().decode_stream(&bytes) {
            Err(error) => (error.offset, error.element),
            Ok(_) => panic!("the reference decoder accepted a bad {field}"),
        };

        assert_eq!(
            fast.0, declared,
            "the fast decoder put a bad {field} at byte {}",
            fast.0
        );
        assert_eq!(
            reference.0, declared,
            "the reference decoder put a bad {field} at byte {}",
            reference.0
        );
        assert!(
            fast.1.ends_with(field) && reference.1.ends_with(field),
            "a bad {field} was named {} and {}",
            fast.1,
            reference.1
        );
    }
}

fn clean_stream() -> Vec<u8> {
    let sequence = SequenceHeader::new(WIDTH, HEIGHT, 24, 1, KEY_INTERVAL, 16).unwrap();
    Encoder::new(sequence, 28)
        .unwrap()
        .encode(&source_frames())
        .unwrap()
        .bytes
}

/// Byte offsets of every packet in the region, in stream order.
fn packet_offsets(bytes: &[u8]) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut offset = SEQUENCE_HEADER_SIZE;
    while offset + FRAME_HEADER_SIZE <= bytes.len() {
        if &bytes[offset..offset + 4] != b"KFP1" {
            offset += 1;
            continue;
        }
        let payload_len =
            u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        offsets.push(offset);
        offset += FRAME_HEADER_SIZE + payload_len;
    }
    offsets
}

/// Rewrites the header CRC so a mutated header stays structurally admissible.
fn reseal_header(bytes: &mut [u8], start: usize) {
    let crc = crc32c(&bytes[start + 4..start + 16]);
    bytes[start + 16..start + 20].copy_from_slice(&crc.to_le_bytes());
}

/// Both decoders' verdicts on the same damaged stream.
struct Verdicts {
    fast: Vec<FrameStatus>,
    reference: Vec<RefFrameStatus>,
    fast_images: usize,
    reference_images: usize,
}

/// Decodes with both implementations and asserts they agree before returning.
fn classify(bytes: &[u8]) -> Verdicts {
    let fast = FastDecoder::new().decode_stream_resilient(bytes).unwrap();
    let reference = ReferenceDecoder::new()
        .decode_stream_resilient(bytes)
        .unwrap();
    assert_eq!(
        fast.statuses.len(),
        reference.statuses.len(),
        "decoders accepted a different number of packets"
    );
    for (index, (left, right)) in fast.statuses.iter().zip(&reference.statuses).enumerate() {
        let matched = matches!(
            (left, right),
            (FrameStatus::Shown, RefFrameStatus::Shown)
                | (FrameStatus::Corrupt, RefFrameStatus::Corrupt)
                | (FrameStatus::DependencyLost, RefFrameStatus::DependencyLost)
                | (
                    FrameStatus::RecoveredKeyframe(Recovery::Gap),
                    RefFrameStatus::RecoveredKeyframe(RefRecovery::Gap),
                )
                | (
                    FrameStatus::RecoveredKeyframe(Recovery::LeadingLoss),
                    RefFrameStatus::RecoveredKeyframe(RefRecovery::LeadingLoss),
                )
        );
        assert!(
            matched,
            "packet {index}: fast said {left:?}, reference said {right:?}"
        );
    }
    assert_eq!(
        fast.frames, reference.frames,
        "decoders produced different images"
    );
    Verdicts {
        fast: fast.statuses,
        reference: reference.statuses,
        fast_images: fast.frames.len(),
        reference_images: reference.frames.len(),
    }
}

#[test]
fn clean_stream_is_entirely_shown() {
    let verdicts = classify(&clean_stream());
    assert_eq!(verdicts.fast.len(), FRAME_COUNT);
    assert!(verdicts.fast.iter().all(|s| *s == FrameStatus::Shown));
    assert_eq!(verdicts.fast_images, FRAME_COUNT);
    assert_eq!(verdicts.reference_images, FRAME_COUNT);
}

#[test]
fn trap_bitstream_truncation() {
    let clean = clean_stream();
    let offsets = packet_offsets(&clean);
    // Cut inside the last packet's payload: its header is intact and promises
    // more bytes than exist.
    let truncated = &clean[..offsets[FRAME_COUNT - 1] + FRAME_HEADER_SIZE + 2];
    let verdicts = classify(truncated);
    assert_eq!(verdicts.fast.len(), FRAME_COUNT);
    assert_eq!(verdicts.fast[FRAME_COUNT - 1], FrameStatus::Corrupt);
    assert!(
        verdicts.fast[..FRAME_COUNT - 1]
            .iter()
            .all(|s| *s == FrameStatus::Shown)
    );
    assert_eq!(verdicts.fast_images, FRAME_COUNT - 1);
}

#[test]
fn trap_header_crc_resync() {
    let mut damaged = clean_stream();
    let offsets = packet_offsets(&damaged);
    // Flip a header CRC byte without resealing: the packet is not structurally
    // admissible at all, so the scanner walks past it one byte at a time and
    // the packet never reaches classification.
    damaged[offsets[1] + 16] ^= 0xFF;
    let verdicts = classify(&damaged);
    assert_eq!(
        verdicts.fast.len(),
        FRAME_COUNT - 1,
        "the unreadable header should not be counted as an accepted packet"
    );
    // Frame 1 is gone, so frame 2 lands on a gap and is dependency-lost. The
    // keyframe at index 3 arrives contiguously after it, so it resumes as an
    // ordinary shown frame rather than as a recovery point.
    assert_eq!(verdicts.fast[0], FrameStatus::Shown);
    assert_eq!(verdicts.fast[1], FrameStatus::DependencyLost);
    assert_eq!(verdicts.fast[2], FrameStatus::Shown);
}

#[test]
fn trap_payload_crc_reference_invalidation() {
    let mut damaged = clean_stream();
    let offsets = packet_offsets(&damaged);
    // Frame 1 is a P-frame; break its payload so the header still parses.
    damaged[offsets[1] + FRAME_HEADER_SIZE + 1] ^= 0xFF;
    let verdicts = classify(&damaged);
    assert_eq!(verdicts.fast[0], FrameStatus::Shown);
    assert_eq!(verdicts.fast[1], FrameStatus::Corrupt);
    // Both references were invalidated, so frame 2 cannot be predicted.
    assert_eq!(verdicts.fast[2], FrameStatus::DependencyLost);
    // The keyframe at index 3 needs no old reference and resumes cleanly.
    assert_eq!(verdicts.fast[3], FrameStatus::Shown);
    assert_eq!(verdicts.fast[4], FrameStatus::Shown);
}

#[test]
fn trap_dependency_loss_until_keyframe() {
    let mut damaged = clean_stream();
    let offsets = packet_offsets(&damaged);
    // Corrupt the keyframe at index 3: everything up to the next key at 6 is
    // unpredictable, and nothing in that window may be entropy-decoded.
    damaged[offsets[3] + FRAME_HEADER_SIZE + 1] ^= 0xFF;
    let verdicts = classify(&damaged);
    assert_eq!(verdicts.fast[3], FrameStatus::Corrupt);
    assert_eq!(verdicts.fast[4], FrameStatus::DependencyLost);
    assert_eq!(verdicts.fast[5], FrameStatus::DependencyLost);
    assert_eq!(verdicts.fast[6], FrameStatus::Shown, "key at 6 recovers");
    assert_eq!(verdicts.fast_images, FRAME_COUNT - 3);
}

#[test]
fn trap_initial_nonkey_rejected() {
    let clean = clean_stream();
    let offsets = packet_offsets(&clean);
    // Start the region at frame 1, a P-frame with no reference state behind it.
    let mut headless = clean[..SEQUENCE_HEADER_SIZE].to_vec();
    headless.extend_from_slice(&clean[offsets[1]..offsets[3]]);
    let verdicts = classify(&headless);
    assert_eq!(verdicts.fast[0], FrameStatus::DependencyLost);
    assert_eq!(verdicts.fast[1], FrameStatus::DependencyLost);
    assert_eq!(
        verdicts.fast_images, 0,
        "no image may be shown before a keyframe"
    );
}

#[test]
fn trap_corrupt_before_first_show() {
    let mut damaged = clean_stream();
    let offsets = packet_offsets(&damaged);
    // Break frame zero. Nothing has ever been shown, so there is no replacement
    // image to hold, and the decoder must simply produce nothing until a key.
    damaged[offsets[0] + FRAME_HEADER_SIZE + 1] ^= 0xFF;
    let verdicts = classify(&damaged);
    assert_eq!(verdicts.fast[0], FrameStatus::Corrupt);
    assert_eq!(verdicts.fast[1], FrameStatus::DependencyLost);
    assert_eq!(verdicts.fast[2], FrameStatus::DependencyLost);
    assert_eq!(verdicts.fast[3], FrameStatus::Shown);
    assert_eq!(verdicts.fast_images, FRAME_COUNT - 3);
}

#[test]
fn trap_corrupt_first_recovers_at_key() {
    let mut damaged = clean_stream();
    let offsets = packet_offsets(&damaged);
    damaged[offsets[0] + FRAME_HEADER_SIZE + 1] ^= 0xFF;
    let fast = FastDecoder::new()
        .decode_stream_resilient(&damaged)
        .unwrap();
    let clean = FastDecoder::new()
        .decode_stream_resilient(&clean_stream())
        .unwrap();
    // The recovered tail must be bit-identical to the same frames decoded from
    // an undamaged stream: recovery does not mean "close enough".
    assert_eq!(fast.frames, clean.frames[3..].to_vec());
}

#[test]
fn trap_index_gap_requires_keyframe() {
    let clean = clean_stream();
    let offsets = packet_offsets(&clean);

    // A gap that lands on a P-frame: it cannot recover, whatever it contains.
    let mut gapped = clean[..offsets[4]].to_vec();
    gapped.extend_from_slice(&clean[offsets[5]..]);
    let verdicts = classify(&gapped);
    assert_eq!(verdicts.fast[3], FrameStatus::Shown);
    assert_eq!(verdicts.fast[4], FrameStatus::DependencyLost);

    // A gap that lands on a keyframe: the loss is processed first, then the key
    // decodes immediately from literal context initials and no old reference.
    let mut gapped_to_key = clean[..offsets[4]].to_vec();
    gapped_to_key.extend_from_slice(&clean[offsets[6]..]);
    let verdicts = classify(&gapped_to_key);
    assert_eq!(verdicts.fast[3], FrameStatus::Shown);
    assert_eq!(
        verdicts.fast[4],
        FrameStatus::RecoveredKeyframe(Recovery::Gap),
        "a keyframe arriving on a gap is the recovery point"
    );
    assert_eq!(verdicts.fast_images, 5);
}

#[test]
fn trap_leading_loss_recovers_at_key() {
    let clean = clean_stream();
    let offsets = packet_offsets(&clean);
    // The region's first packet is the keyframe at index 3: nothing preceded it,
    // so the loss is leading rather than a mid-stream gap.
    let mut headless = clean[..SEQUENCE_HEADER_SIZE].to_vec();
    headless.extend_from_slice(&clean[offsets[3]..]);
    let verdicts = classify(&headless);
    assert_eq!(
        verdicts.fast[0],
        FrameStatus::RecoveredKeyframe(Recovery::LeadingLoss)
    );
    assert!(
        verdicts.fast[1..].iter().all(|s| *s == FrameStatus::Shown),
        "once recovered, the tail decodes normally"
    );
}

#[test]
fn trap_hidden_frame_rejected() {
    let mut damaged = clean_stream();
    let offsets = packet_offsets(&damaged);
    // Clear the show bit and reseal, so only the flag rule can reject it.
    damaged[offsets[1] + 12] &= !0b100;
    reseal_header(&mut damaged, offsets[1]);
    let verdicts = classify(&damaged);
    assert_eq!(
        verdicts.fast.len(),
        FRAME_COUNT - 1,
        "a hidden frame is not an admissible packet in version one"
    );
    assert_eq!(verdicts.fast[1], FrameStatus::DependencyLost);
}

#[test]
fn trap_key_requires_golden_refresh() {
    let mut damaged = clean_stream();
    let offsets = packet_offsets(&damaged);
    // Frame 3 is a keyframe; clear its golden-refresh bit and reseal.
    damaged[offsets[3] + 12] &= !0b10;
    reseal_header(&mut damaged, offsets[3]);
    let verdicts = classify(&damaged);
    assert_eq!(
        verdicts.fast.len(),
        FRAME_COUNT - 1,
        "a key that does not refresh GOLDEN is not admissible"
    );
}

#[test]
fn trap_zero_size_packet() {
    let mut damaged = clean_stream();
    let offsets = packet_offsets(&damaged);
    // A declared payload length of zero is below the floor and rejected at the
    // header, so the packet is skipped rather than decoded as empty.
    damaged[offsets[1] + 4..offsets[1] + 8].copy_from_slice(&0u32.to_le_bytes());
    reseal_header(&mut damaged, offsets[1]);
    let verdicts = classify(&damaged);
    assert!(verdicts.fast.len() < FRAME_COUNT);
    assert_eq!(verdicts.fast[0], FrameStatus::Shown);
}

#[test]
fn trap_oversize_payload_length() {
    let mut damaged = clean_stream();
    let offsets = packet_offsets(&damaged);
    // A payload length past the region end must be refused before any
    // allocation is attempted.
    damaged[offsets[1] + 4..offsets[1] + 8].copy_from_slice(&0x00FF_FFFFu32.to_le_bytes());
    reseal_header(&mut damaged, offsets[1]);
    let verdicts = classify(&damaged);
    assert_eq!(verdicts.fast[0], FrameStatus::Shown);
    assert_eq!(
        verdicts.fast[1],
        FrameStatus::Corrupt,
        "an unsatisfiable declared extent is corruption, not a silent stop"
    );
}

#[test]
fn damaged_streams_never_poison_later_prediction() {
    // Every single-byte payload wound, swept across the stream, must leave the
    // decoders agreeing frame for frame. This is the matrix's backstop: it
    // catches a poisoned reference that an individually named case would miss.
    let clean = clean_stream();
    let offsets = packet_offsets(&clean);
    for (packet, &start) in offsets.iter().enumerate() {
        for byte in [1usize, 3, 7] {
            let mut damaged = clean.clone();
            let target = start + FRAME_HEADER_SIZE + byte;
            if target >= damaged.len() {
                continue;
            }
            damaged[target] ^= 0xFF;
            let verdicts = classify(&damaged);
            assert_eq!(
                verdicts.fast.len(),
                verdicts.reference.len(),
                "packet {packet} byte {byte}"
            );
            assert_eq!(verdicts.fast_images, verdicts.reference_images);
        }
    }
}

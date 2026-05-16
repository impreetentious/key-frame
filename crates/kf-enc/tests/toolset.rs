//! Every ablation has to stay inside the format.
//!
//! An ablation is only a fair comparison if the thing being measured is still
//! the same codec. A narrowed toolset changes which decision the encoder
//! reaches, never what a decision means, so every stream one of them produces
//! must decode bit-exactly in both decoders exactly like any other stream. If
//! that ever stopped being true, the ablation curves would be measuring a fork
//! of the format against itself and the comparison would be worthless.
//!
//! The second thing checked here is that the switches do something. A tool that
//! is "turned off" but leaves the bytes unchanged produces a flat ablation
//! curve, which reads as "this tool is worth nothing" rather than as the bug it
//! actually is.

use kf_bitstream::SequenceHeader;
use kf_dec::FastDecoder;
use kf_enc::{Encoder, Toolset};
use kf_frame::Frame;
use kf_ref::ReferenceDecoder;

fn sequence() -> SequenceHeader {
    SequenceHeader::new(64, 64, 24, 1, 120, 4).unwrap()
}

/// A short clip with real translation in it.
///
/// A textured field panned by a non-integer displacement is the only content
/// that makes every switch matter at once: motion search has something to find,
/// subpel refinement has something to refine, and the quadtree has an edge to
/// follow. Flat or purely random frames would leave several of the ablations
/// indistinguishable from the full toolset for reasons that say nothing about
/// the tools.
fn panning_clip(frames: usize) -> Vec<Frame> {
    let mut field = vec![0_u8; 256 * 256];
    let mut state = 0x2545_f491_u32;
    for sample in &mut field {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        // Half-amplitude noise over mid grey: structured at block scale rather
        // than incompressible.
        *sample = 64 + (state.to_le_bytes()[0] >> 1);
    }
    // A blur pass gives the field low-frequency content, so subpel positions
    // are genuinely better than the integer ones next to them.
    let mut smooth = field.clone();
    for y in 1..255_usize {
        for x in 1..255_usize {
            let mut total = 0_u32;
            for dy in 0..3 {
                for dx in 0..3 {
                    total += u32::from(field[(y + dy - 1) * 256 + (x + dx - 1)]);
                }
            }
            smooth[y * 256 + x] = u8::try_from(total / 9).unwrap();
        }
    }

    (0..frames)
        .map(|index| {
            let mut frame = Frame::filled_420(64, 64, 0).unwrap();
            for y in 0..64_u32 {
                for x in 0..64_u32 {
                    let source_x = (usize::try_from(x).unwrap() + index * 3) % 200;
                    let source_y = (usize::try_from(y).unwrap() + index * 2) % 200;
                    frame
                        .y
                        .set(x, y, smooth[source_y * 256 + source_x])
                        .unwrap();
                }
            }
            frame
        })
        .collect()
}

fn encode(toolset: Toolset, clip: &[Frame]) -> Vec<u8> {
    Encoder::new(sequence(), 32)
        .unwrap()
        .with_toolset(toolset)
        .encode(clip)
        .unwrap()
        .bytes
}

#[test]
fn every_ablation_produces_a_stream_both_decoders_reconstruct_identically() {
    let clip = panning_clip(6);
    for name in Toolset::names() {
        let toolset = Toolset::named(name).expect("a named toolset");
        let encoder = Encoder::new(sequence(), 32).unwrap().with_toolset(toolset);
        assert_eq!(encoder.toolset(), toolset);

        let encoded = encoder.encode(&clip).unwrap();
        assert_eq!(
            encoder.encode(&clip).unwrap().bytes,
            encoded.bytes,
            "{name}: a narrowed toolset must still encode deterministically"
        );

        let fast = FastDecoder::new()
            .decode_stream(&encoded.bytes)
            .unwrap_or_else(|error| panic!("{name}: the fast decoder refused the stream: {error}"));
        let reference = ReferenceDecoder::new()
            .decode_stream(&encoded.bytes)
            .unwrap_or_else(|error| {
                panic!("{name}: the reference decoder refused the stream: {error}")
            });

        assert_eq!(fast.len(), clip.len(), "{name}: the stream lost a frame");
        for (index, ((fast, reference), closed_loop)) in fast
            .iter()
            .zip(&reference)
            .zip(&encoded.reconstructed_frames)
            .enumerate()
        {
            assert_eq!(
                fast, reference,
                "{name}: the decoders disagree on frame {index}"
            );
            assert_eq!(
                fast, closed_loop,
                "{name}: the encoder's own reconstruction of frame {index} is not what decodes"
            );
        }
    }
}

#[test]
fn turning_a_tool_off_changes_the_stream() {
    // Each switch has to reach the bytes. A switch that is read but never acted
    // on gives an ablation curve identical to the baseline, which looks like a
    // finding — "this tool buys nothing" — and is really a dead branch.
    let clip = panning_clip(6);
    let full = encode(Toolset::full(), &clip);
    for name in Toolset::names() {
        if name == "full" {
            continue;
        }
        let ablated = encode(Toolset::named(name).expect("a named toolset"), &clip);
        assert_ne!(
            ablated, full,
            "{name} produced the same bytes as the full toolset, so the switch does nothing"
        );
    }
}

#[test]
fn dropping_inter_costs_real_bits_on_a_moving_clip() {
    // The one ablation whose direction is not a matter of taste. Every frame
    // after the first is a translation of the one before it, so intra-only
    // coding has to spend the whole picture again at roughly the same
    // distortion. If this ever came out close, inter prediction would not be
    // doing anything and the rest of the suite would be proving very little.
    //
    // The margin is a fifth rather than something dramatic because this clip is
    // a smoothed noise field: intra prediction does respectably on it, and the
    // first frame — which no toolset can code any other way — is a large share
    // of six. It measures around a third today. A fifth is the floor below
    // which "inter prediction earns its keep" would stop being true.
    let clip = panning_clip(6);
    let full = encode(Toolset::full(), &clip).len();
    let intra_only = encode(Toolset::named("no-inter").unwrap(), &clip).len();
    assert!(
        intra_only * 5 > full * 6,
        "intra-only coded {intra_only} bytes against {full} with inter prediction, \
         which is too close for a clip that is pure translation"
    );
}

#[test]
fn dropping_inter_leaves_p_frames_that_are_still_p_frames() {
    // The ablation removes candidates from the search. It does not turn P
    // frames into key frames: the frame type, the context carry, and the
    // reference bookkeeping all have to stay exactly as they were, or the
    // curve measures a different GOP structure rather than a different toolset.
    let clip = panning_clip(6);
    let encoded = Encoder::new(sequence(), 32)
        .unwrap()
        .with_toolset(Toolset::named("no-inter").unwrap())
        .encode(&clip)
        .unwrap();

    assert!(
        encoded.frames[0].key,
        "the first frame is always a key frame"
    );
    for (index, frame) in encoded.frames.iter().enumerate().skip(1) {
        assert!(
            !frame.key,
            "frame {index} became a key frame when inter prediction was dropped"
        );
    }
    assert!(
        ReferenceDecoder::new()
            .decode_stream(&encoded.bytes)
            .is_ok(),
        "an all-intra P frame is still an ordinary stream"
    );
}

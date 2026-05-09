//! The one thing about the ablations that cannot be checked from inside the
//! encoder.
//!
//! `kf-enc` proves the structural properties of a narrowed toolset — that every
//! combination still produces a stream both decoders reconstruct identically,
//! that each switch reaches the bytes, that a P frame stays a P frame. It
//! cannot prove anything about *quality*, because quality is a decibel and the
//! codec perimeter forbids floating point outright. So the property that needs
//! a metric lives here, with the tools that are allowed to compute one.
//!
//! The property is that a narrowed search never beats the full one on both rate
//! and quality at once. Fewer bytes alone proves nothing: rate–distortion
//! optimization trades the two against each other, so removing a candidate can
//! save bits by giving up the quality that candidate was buying. Dropping the
//! golden reference on this clip does exactly that, and it is a real result.
//! What cannot happen is a strict win on both axes — the full toolset scores
//! every candidate a narrowed one does, with the same lambda, so a double win
//! would mean the switches are perturbing the search rather than restricting
//! it.

use kf_bitstream::SequenceHeader;
use kf_enc::{Encoder, Toolset};
use kf_frame::Frame;
use kf_tools::psnr_y;

fn sequence() -> SequenceHeader {
    SequenceHeader::new(128, 128, 24, 1, 120, 4).unwrap()
}

/// A textured field panned by a displacement that is not a whole block.
///
/// Motion search has something to find, subpel refinement has something to
/// refine, and the quadtree has an edge to follow. Flat or purely random frames
/// would leave several ablations indistinguishable from the full toolset for
/// reasons that say nothing about the tools.
fn panning_clip(frames: usize) -> Vec<Frame> {
    let mut field = vec![0_u8; 256 * 256];
    let mut state = 0x2545_f491_u32;
    for sample in &mut field {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *sample = 64 + (state.to_le_bytes()[0] >> 1);
    }
    let mut smooth = field.clone();
    for y in 1..255_usize {
        for x in 1..255_usize {
            let mut total = 0_u32;
            for dy in 0..3 {
                for dx in 0..3 {
                    total += u32::from(field[(y + dy - 1) * 256 + (x + dx - 1)]);
                }
            }
            smooth[y * 256 + x] = (total / 9) as u8;
        }
    }

    (0..frames)
        .map(|index| {
            let mut frame = Frame::filled_420(128, 128, 0).unwrap();
            for y in 0..128_u32 {
                for x in 0..128_u32 {
                    let source_x = (x as usize + index * 3) % 200;
                    let source_y = (y as usize + index * 2) % 200;
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

fn rate_and_quality(toolset: Toolset, clip: &[Frame]) -> (usize, f64) {
    let encoded = Encoder::new(sequence(), 32)
        .unwrap()
        .with_toolset(toolset)
        .encode(clip)
        .unwrap();
    let quality = psnr_y(clip, &encoded.reconstructed_frames)
        .expect("the reconstruction measures against its source");
    (encoded.bytes.len(), quality.global)
}

#[test]
fn no_ablation_is_both_smaller_and_sharper_than_the_full_toolset() {
    let clip = panning_clip(6);
    let (full_bytes, full_quality) = rate_and_quality(Toolset::full(), &clip);
    assert!(
        full_quality.is_finite(),
        "the baseline encode was lossless, so this clip proves nothing at this QP"
    );

    for name in Toolset::names() {
        if name == "full" {
            continue;
        }
        let (bytes, quality) = rate_and_quality(Toolset::named(name).unwrap(), &clip);
        assert!(
            !(bytes < full_bytes && quality > full_quality),
            "{name} coded {bytes} bytes at {quality:.3} dB, beating the full toolset's \
             {full_bytes} bytes at {full_quality:.3} dB on both axes"
        );
    }
}

#[test]
fn dropping_the_quadtree_trades_quality_away_rather_than_saving_bits_for_free() {
    // `no-split` is the ablation most likely to be misread. It codes far fewer
    // bytes, which looks like a win until the picture is measured: a 64×64 leaf
    // fits real content badly, so the saving is paid for in distortion. Asserting
    // both directions is what stops the charts page from being read as "the
    // quadtree costs bits and buys nothing".
    let clip = panning_clip(6);
    let (full_bytes, full_quality) = rate_and_quality(Toolset::full(), &clip);
    let (flat_bytes, flat_quality) = rate_and_quality(Toolset::named("no-split").unwrap(), &clip);

    assert!(
        flat_bytes < full_bytes,
        "a single-leaf partition should code fewer bytes, not {flat_bytes} against {full_bytes}"
    );
    assert!(
        flat_quality < full_quality,
        "a single-leaf partition coded {flat_quality:.3} dB against the quadtree's \
         {full_quality:.3} dB, so the bytes it saved were apparently free"
    );
}

//! Hand-authored conformance streams.
//!
//! These are written at the syntax layer rather than produced by `kf-enc`, so
//! they can exercise combinations the encoder's mode decision would never
//! choose: a superblock split all the way down on one side and left whole on
//! the other, every intra mode in one frame, every transform size carrying
//! coefficients, every P-frame prediction branch side by side.
//!
//! The point is coverage that does not depend on the encoder. If the only
//! streams proving an element were encoder output, an encoder bug and a
//! decoder bug that agreed with each other would look like conformance.
//!
//! Everything here mirrors the decoder's own traversal — superblocks in raster
//! order, the partition tree top-down, then per leaf a prediction followed by
//! luma and both chroma residuals — because a payload that does not is simply
//! a different stream, not a wrong one.

use kf_bitstream::{
    BlockSize, FrameFlags, FramePacket, FrameType, IntraMode, MotionVector, PartitionTree,
    PlaneClass, Prediction, ReferenceFrame, SequenceHeader, SyntaxWriter, TransformBlockSize,
};
use kf_range::ContextBank;

/// One committed hand-authored stream and the facts the manifest records.
pub struct HandVector {
    pub name: &'static str,
    pub width: u16,
    pub height: u16,
    pub qp: u8,
    pub frame_count: u32,
    pub bytes: Vec<u8>,
}

/// What residual a leaf carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Residual {
    /// Every transform block codes presence-false and stops.
    Empty,
    /// Every transform block carries a dense magnitude ladder.
    Ladder,
}

/// One coding-block leaf: how it predicts and what it carries.
struct Leaf {
    prediction: Prediction,
    residual: Residual,
}

/// The transform blocks one coding block codes, in decoder order.
///
/// Luma uses the block side capped at 32, so a 64-wide block codes four luma
/// transforms; chroma is always half the block side. Luma therefore never
/// reaches the 4×4 transform and chroma never exceeds it at the small end —
/// that asymmetry is why the coefficient vector needs both an unsplit
/// superblock and a fully split one.
#[must_use]
pub fn transform_schedule(size: BlockSize) -> [(PlaneClass, TransformBlockSize, usize); 3] {
    match size {
        BlockSize::N64 => [
            (PlaneClass::Luma, TransformBlockSize::N32, 4),
            (PlaneClass::Chroma, TransformBlockSize::N32, 1),
            (PlaneClass::Chroma, TransformBlockSize::N32, 1),
        ],
        BlockSize::N32 => [
            (PlaneClass::Luma, TransformBlockSize::N32, 1),
            (PlaneClass::Chroma, TransformBlockSize::N16, 1),
            (PlaneClass::Chroma, TransformBlockSize::N16, 1),
        ],
        BlockSize::N16 => [
            (PlaneClass::Luma, TransformBlockSize::N16, 1),
            (PlaneClass::Chroma, TransformBlockSize::N8, 1),
            (PlaneClass::Chroma, TransformBlockSize::N8, 1),
        ],
        BlockSize::N8 => [
            (PlaneClass::Luma, TransformBlockSize::N8, 1),
            (PlaneClass::Chroma, TransformBlockSize::N4, 1),
            (PlaneClass::Chroma, TransformBlockSize::N4, 1),
        ],
    }
}

/// A dense ladder of nonzero levels for one transform block.
///
/// Fourteen nonzero coefficients, so that whichever one the diagonal scan
/// places last, thirteen precede it and the significance counter walks past
/// its final distinct slot. Every magnitude is at least two, which forces the
/// greater-than-one flag true at each of its distinct counter slots and so
/// reaches the greater-than-two flag and the bypass remainder behind it.
#[must_use]
pub fn ladder_levels(size: TransformBlockSize) -> Vec<i32> {
    let total = size.side() * size.side();
    let mut levels = vec![0_i32; total];
    for (position, slot) in levels.iter_mut().take(14.min(total)).enumerate() {
        let magnitude = 2 + i32::try_from(position % 4).expect("invariant: position below four");
        *slot = if position % 2 == 0 {
            magnitude
        } else {
            -magnitude
        };
    }
    levels
}

fn leaf_sizes(tree: &PartitionTree) -> Vec<BlockSize> {
    match tree {
        PartitionTree::Leaf(size) => vec![*size],
        PartitionTree::Split { children, .. } => {
            children.iter().flat_map(leaf_sizes).collect::<Vec<_>>()
        }
    }
}

fn write_superblock(
    writer: &mut SyntaxWriter,
    frame_type: FrameType,
    tree: &PartitionTree,
    leaves: &[Leaf],
) -> Result<(), String> {
    let sizes = leaf_sizes(tree);
    if sizes.len() != leaves.len() {
        return Err(format!(
            "superblock has {} leaves but {} were described",
            sizes.len(),
            leaves.len()
        ));
    }
    writer.write_partition(tree).map_err(stringify)?;
    for (size, leaf) in sizes.into_iter().zip(leaves) {
        writer
            .write_prediction(frame_type, leaf.prediction)
            .map_err(stringify)?;
        if matches!(leaf.prediction, Prediction::Skip { .. }) {
            continue;
        }
        for (plane, transform, count) in transform_schedule(size) {
            for _ in 0..count {
                let levels = match leaf.residual {
                    Residual::Empty => vec![0; transform.side().pow(2)],
                    Residual::Ladder => ladder_levels(transform),
                };
                writer
                    .write_coefficients(plane, transform, &levels)
                    .map_err(stringify)?;
            }
        }
    }
    Ok(())
}

/// Continues a stream whose contexts carry across frames, which is why a
/// multi-frame hand vector cannot simply concatenate independently written
/// payloads: the P frame must be written from the key frame's final bank.
fn continued_payload(
    contexts: ContextBank,
    frame_type: FrameType,
    superblocks: &[(PartitionTree, Vec<Leaf>)],
) -> Result<(Vec<u8>, ContextBank), String> {
    let mut writer = SyntaxWriter::new(contexts);
    for (tree, leaves) in superblocks {
        write_superblock(&mut writer, frame_type, tree, leaves)?;
    }
    let (encoded, final_contexts) = writer.finish();
    Ok((encoded.bytes, final_contexts))
}

fn key_payload(
    superblocks: &[(PartitionTree, Vec<Leaf>)],
) -> Result<(Vec<u8>, ContextBank), String> {
    continued_payload(ContextBank::initial(), FrameType::Key, superblocks)
}

fn stream(
    width: u16,
    height: u16,
    qp: u8,
    payloads: &[(bool, Vec<u8>)],
) -> Result<Vec<u8>, String> {
    let sequence = SequenceHeader::new(width, height, 24, 1, 120, 16).map_err(stringify)?;
    let mut bytes = sequence.encode().to_vec();
    for (index, (key, payload)) in payloads.iter().enumerate() {
        let packet = FramePacket::new(
            u32::try_from(index).map_err(|_| "frame index overflow".to_owned())?,
            FrameFlags {
                key: *key,
                golden_refresh: *key,
                show: true,
            },
            qp,
            payload.clone(),
        )
        .map_err(stringify)?;
        bytes.extend_from_slice(&packet.encode());
    }
    Ok(bytes)
}

fn intra_leaf(mode: IntraMode, residual: Residual) -> Leaf {
    Leaf {
        prediction: Prediction::Intra(mode),
        residual,
    }
}

fn split(size: BlockSize, children: [PartitionTree; 4]) -> PartitionTree {
    PartitionTree::Split {
        size,
        children: Box::new(children),
    }
}

/// A single superblock split to a different depth in each quadrant, so the
/// split decision is coded once at every depth the format allows. The
/// minimum-size leaf codes no decision at all, which is the rule the counter
/// has to see honored rather than asserted.
fn partition_depth_ladder() -> Result<Vec<u8>, String> {
    let tree = split(
        BlockSize::N64,
        [
            PartitionTree::Leaf(BlockSize::N32),
            split(
                BlockSize::N32,
                [
                    PartitionTree::Leaf(BlockSize::N16),
                    PartitionTree::Leaf(BlockSize::N16),
                    PartitionTree::Leaf(BlockSize::N16),
                    split(
                        BlockSize::N16,
                        [
                            PartitionTree::Leaf(BlockSize::N8),
                            PartitionTree::Leaf(BlockSize::N8),
                            PartitionTree::Leaf(BlockSize::N8),
                            PartitionTree::Leaf(BlockSize::N8),
                        ],
                    ),
                ],
            ),
            PartitionTree::Leaf(BlockSize::N32),
            PartitionTree::Leaf(BlockSize::N32),
        ],
    );
    let leaves = leaf_sizes(&tree)
        .into_iter()
        .map(|_| intra_leaf(IntraMode::Dc, Residual::Empty))
        .collect::<Vec<_>>();
    let (payload, _) = key_payload(&[(tree, leaves)])?;
    stream(64, 64, 32, &[(true, payload)])
}

/// Every intra mode in one frame, so the three live bins of the mode tree are
/// each coded with both symbols.
fn every_intra_mode() -> Result<Vec<u8>, String> {
    let quadrant = || {
        split(
            BlockSize::N32,
            [
                PartitionTree::Leaf(BlockSize::N16),
                PartitionTree::Leaf(BlockSize::N16),
                PartitionTree::Leaf(BlockSize::N16),
                PartitionTree::Leaf(BlockSize::N16),
            ],
        )
    };
    let tree = split(
        BlockSize::N64,
        [quadrant(), quadrant(), quadrant(), quadrant()],
    );
    let modes = [
        IntraMode::Dc,
        IntraMode::Planar,
        IntraMode::Horizontal,
        IntraMode::Vertical,
        IntraMode::D45,
        IntraMode::D135,
        IntraMode::D117,
        IntraMode::D153,
    ];
    let leaves = (0..leaf_sizes(&tree).len())
        .map(|index| intra_leaf(modes[index % modes.len()], Residual::Empty))
        .collect::<Vec<_>>();
    let (payload, _) = key_payload(&[(tree, leaves)])?;
    stream(64, 64, 32, &[(true, payload)])
}

/// Coefficients at every transform size in both plane classes.
///
/// Two superblocks are needed, not one: the 32×32 chroma transform only
/// appears under an unsplit 64×64 block, and the 4×4 chroma transform only
/// under an 8×8 one.
fn coefficient_size_matrix() -> Result<Vec<u8>, String> {
    let whole = PartitionTree::Leaf(BlockSize::N64);
    let ladder = split(
        BlockSize::N64,
        [
            PartitionTree::Leaf(BlockSize::N32),
            split(
                BlockSize::N32,
                [
                    PartitionTree::Leaf(BlockSize::N16),
                    PartitionTree::Leaf(BlockSize::N16),
                    PartitionTree::Leaf(BlockSize::N16),
                    split(
                        BlockSize::N16,
                        [
                            PartitionTree::Leaf(BlockSize::N8),
                            PartitionTree::Leaf(BlockSize::N8),
                            PartitionTree::Leaf(BlockSize::N8),
                            PartitionTree::Leaf(BlockSize::N8),
                        ],
                    ),
                ],
            ),
            PartitionTree::Leaf(BlockSize::N32),
            PartitionTree::Leaf(BlockSize::N32),
        ],
    );
    let whole_leaves = vec![intra_leaf(IntraMode::Dc, Residual::Ladder)];
    let ladder_leaves = leaf_sizes(&ladder)
        .into_iter()
        .map(|_| intra_leaf(IntraMode::Dc, Residual::Ladder))
        .collect::<Vec<_>>();
    let (payload, _) = key_payload(&[(whole, whole_leaves), (ladder, ladder_leaves)])?;
    stream(128, 64, 32, &[(true, payload)])
}

/// A key frame followed by a P frame that takes every prediction branch: both
/// skip references, both non-skip inter references with a motion difference
/// long enough to reach the third prefix bin on each axis, and an intra block
/// inside a P frame.
fn pframe_prediction_branches() -> Result<Vec<u8>, String> {
    let tree = split(
        BlockSize::N64,
        [
            PartitionTree::Leaf(BlockSize::N32),
            PartitionTree::Leaf(BlockSize::N32),
            PartitionTree::Leaf(BlockSize::N32),
            split(
                BlockSize::N32,
                [
                    PartitionTree::Leaf(BlockSize::N16),
                    PartitionTree::Leaf(BlockSize::N16),
                    PartitionTree::Leaf(BlockSize::N16),
                    PartitionTree::Leaf(BlockSize::N16),
                ],
            ),
        ],
    );
    let key_leaves = leaf_sizes(&tree)
        .into_iter()
        .map(|_| intra_leaf(IntraMode::Dc, Residual::Empty))
        .collect::<Vec<_>>();
    let (key, contexts) = key_payload(&[(tree.clone(), key_leaves)])?;

    // A magnitude of three codes an exp-Golomb prefix of two, which is the
    // shortest difference that touches all three contexted prefix positions.
    let reaching_mvd = MotionVector { x_q4: 3, y_q4: -3 };
    let p_leaves = vec![
        Leaf {
            prediction: Prediction::Skip {
                reference: ReferenceFrame::Last,
            },
            residual: Residual::Empty,
        },
        Leaf {
            prediction: Prediction::Skip {
                reference: ReferenceFrame::Golden,
            },
            residual: Residual::Empty,
        },
        Leaf {
            prediction: Prediction::Inter {
                reference: ReferenceFrame::Last,
                mvd: reaching_mvd,
            },
            residual: Residual::Empty,
        },
        Leaf {
            prediction: Prediction::Inter {
                reference: ReferenceFrame::Golden,
                mvd: reaching_mvd,
            },
            residual: Residual::Empty,
        },
        intra_leaf(IntraMode::Vertical, Residual::Empty),
        Leaf {
            prediction: Prediction::Inter {
                reference: ReferenceFrame::Last,
                mvd: MotionVector { x_q4: 0, y_q4: 0 },
            },
            residual: Residual::Empty,
        },
        Leaf {
            prediction: Prediction::Skip {
                reference: ReferenceFrame::Last,
            },
            residual: Residual::Empty,
        },
    ];
    let (p_frame, _) = continued_payload(contexts, FrameType::P, &[(tree, p_leaves)])?;
    stream(64, 64, 32, &[(true, key), (false, p_frame)])
}

/// Every hand-authored vector, in manifest order.
pub fn hand_vectors() -> Result<Vec<HandVector>, String> {
    Ok(vec![
        HandVector {
            name: "partition_depth_ladder",
            width: 64,
            height: 64,
            qp: 32,
            frame_count: 1,
            bytes: partition_depth_ladder()?,
        },
        HandVector {
            name: "every_intra_mode",
            width: 64,
            height: 64,
            qp: 32,
            frame_count: 1,
            bytes: every_intra_mode()?,
        },
        HandVector {
            name: "coefficient_size_matrix",
            width: 128,
            height: 64,
            qp: 32,
            frame_count: 1,
            bytes: coefficient_size_matrix()?,
        },
        HandVector {
            name: "pframe_prediction_branches",
            width: 64,
            height: 64,
            qp: 32,
            frame_count: 2,
            bytes: pframe_prediction_branches()?,
        },
    ])
}

fn stringify<E: core::fmt::Display>(error: E) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::{Residual, ladder_levels, leaf_sizes, transform_schedule};
    use kf_bitstream::{BlockSize, PartitionTree, TransformBlockSize};

    #[test]
    fn the_ladder_leaves_room_for_the_significance_counter() {
        for size in [
            TransformBlockSize::N4,
            TransformBlockSize::N8,
            TransformBlockSize::N16,
            TransformBlockSize::N32,
        ] {
            let levels = ladder_levels(size);
            assert_eq!(levels.len(), size.side() * size.side());
            let nonzero = levels.iter().filter(|level| **level != 0).count();
            assert_eq!(nonzero, 14.min(levels.len()));
            assert!(
                levels
                    .iter()
                    .filter(|level| **level != 0)
                    .all(|level| level.unsigned_abs() >= 2),
                "every ladder magnitude must reach the greater-than-one flag"
            );
            assert!(
                levels.iter().any(|level| level.unsigned_abs() >= 3),
                "the ladder must reach the bypass remainder"
            );
            assert!(levels.iter().any(|level| *level < 0));
        }
    }

    #[test]
    fn only_the_widest_block_reaches_the_largest_chroma_transform() {
        assert!(
            transform_schedule(BlockSize::N64)
                .iter()
                .any(|(_, size, _)| *size == TransformBlockSize::N32)
        );
        for size in [BlockSize::N32, BlockSize::N16, BlockSize::N8] {
            assert!(
                !transform_schedule(size)
                    .iter()
                    .any(
                        |(plane, transform, _)| *plane == kf_bitstream::PlaneClass::Chroma
                            && *transform == TransformBlockSize::N32
                    )
            );
        }
    }

    #[test]
    fn leaf_order_is_the_decoder_traversal_order() {
        let tree = PartitionTree::Split {
            size: BlockSize::N64,
            children: Box::new([
                PartitionTree::Leaf(BlockSize::N32),
                PartitionTree::Split {
                    size: BlockSize::N32,
                    children: Box::new([
                        PartitionTree::Leaf(BlockSize::N16),
                        PartitionTree::Leaf(BlockSize::N16),
                        PartitionTree::Leaf(BlockSize::N16),
                        PartitionTree::Leaf(BlockSize::N16),
                    ]),
                },
                PartitionTree::Leaf(BlockSize::N32),
                PartitionTree::Leaf(BlockSize::N32),
            ]),
        };
        assert_eq!(
            leaf_sizes(&tree),
            vec![
                BlockSize::N32,
                BlockSize::N16,
                BlockSize::N16,
                BlockSize::N16,
                BlockSize::N16,
                BlockSize::N32,
                BlockSize::N32,
            ]
        );
    }

    #[test]
    fn an_empty_residual_is_distinguishable_from_a_ladder() {
        assert_ne!(Residual::Empty, Residual::Ladder);
    }
}

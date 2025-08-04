use kf_bitstream::{
    BlockSize, FrameFlags, FramePacket, FrameType, IntraMode, PartitionTree, PlaneClass,
    Prediction, SequenceHeader, SyntaxWriter, TransformBlockSize,
};
use kf_frame::{Frame, Plane};
use kf_predict::{
    BlockMotion, IntraMode as PredictMode, MotionField, MotionVector as PredictMotionVector,
    PlaneScale, ReferenceSlot, clamp_motion_vector, predict_inter, predict_intra,
};
use kf_range::{ContextBank, CoverageCounter};
use kf_transform::{
    TransformSize, dequantize_block, forward_transform, inverse_transform, lambda_q8,
    quantize_block,
};

use crate::{EncodeError, FrameDecision, GopPlanner, motion_search::estimate_motion};

/// Canonical replay accounting for one encoded coding block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockAccounting {
    pub x: u32,
    pub y: u32,
    pub size: u8,
    pub prediction: Prediction,
    pub motion_vector_q4: Option<[i32; 2]>,
    pub modeled_entropy_q16: u64,
    pub emitted_payload_bytes: u64,
}

/// Partition-structure and leaf accounting for one superblock.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuperblockAccounting {
    pub x: u32,
    pub y: u32,
    pub structure_modeled_entropy_q16: u64,
    pub structure_emitted_payload_bytes: u64,
    pub blocks: Vec<BlockAccounting>,
}

/// Exact canonical accounting for one frame payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameAccounting {
    pub frame_index: u32,
    pub key: bool,
    pub golden_refresh: bool,
    pub qp: u8,
    pub payload_len: usize,
    pub frame_flush_bytes: u64,
    pub superblocks: Vec<SuperblockAccounting>,
}

/// Serialized stream plus encoder-side accounting receipts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedStream {
    pub bytes: Vec<u8>,
    pub frames: Vec<FrameAccounting>,
    pub reconstructed_frames: Vec<Frame>,
    pub checkpoints: Vec<[u16; 144]>,
    pub coverage: CoverageCounter,
}

/// Deterministic canonical IPPP encoder configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Encoder {
    sequence: SequenceHeader,
    qp: u8,
}

impl Encoder {
    pub fn new(sequence: SequenceHeader, qp: u8) -> Result<Self, EncodeError> {
        if qp > 63 {
            return Err(EncodeError::InvalidInput {
                element: "frame.qp",
            });
        }
        Ok(Self { sequence, qp })
    }

    /// Encodes the canonical deterministic IPPP stream with authoritative key
    /// and golden-refresh policy.
    pub fn encode(&self, frames: &[Frame]) -> Result<EncodedStream, EncodeError> {
        if frames.is_empty() {
            return Err(EncodeError::InvalidInput {
                element: "frames.empty",
            });
        }
        validate_sources(&self.sequence, frames)?;
        let mut bytes = self.sequence.encode().to_vec();
        let mut accounting = Vec::with_capacity(frames.len());
        let mut reconstructed_frames = Vec::with_capacity(frames.len());
        let mut planner = GopPlanner::new(
            self.sequence.keyframe_interval,
            self.sequence.golden_interval,
        )?;
        let mut contexts = ContextBank::initial();
        let mut last = None;
        let mut golden = None;
        let mut previous_source = None;
        let mut checkpoints = Vec::new();
        let mut coverage = CoverageCounter::new();

        for (frame_index, source) in frames.iter().enumerate() {
            let frame_index =
                u32::try_from(frame_index).map_err(|_| EncodeError::InvalidInput {
                    element: "frame.count",
                })?;
            let transition_sad = previous_source
                .map(|previous| frame_luma_sad(previous, source))
                .transpose()?;
            let decision = planner.next(frame_index, transition_sad)?;
            let frame_type = if decision.key {
                contexts = ContextBank::initial();
                FrameType::Key
            } else {
                FrameType::P
            };
            let references = match (&last, &golden) {
                (Some(last), Some(golden)) => Some(ReferenceFrames { last, golden }),
                _ => None,
            };
            let encoded = encode_video_frame(
                source,
                frame_index,
                self.qp,
                frame_type,
                decision,
                contexts,
                references,
            )?;
            contexts = encoded.contexts;
            reconstructed_frames.push(
                encoded
                    .reconstructed
                    .crop_420(
                        u32::from(self.sequence.width),
                        u32::from(self.sequence.height),
                    )
                    .map_err(|_| reconstruction("reconstruction.crop"))?,
            );
            last = Some(encoded.reconstructed.clone());
            if decision.key || decision.golden_refresh {
                golden = Some(encoded.reconstructed);
            }
            bytes.extend_from_slice(&encoded.packet.encode());
            accounting.push(encoded.accounting);
            checkpoints.extend(encoded.checkpoints);
            coverage.merge(&encoded.coverage);
            previous_source = Some(source);
        }
        Ok(EncodedStream {
            bytes,
            frames: accounting,
            reconstructed_frames,
            checkpoints,
            coverage,
        })
    }
}

/// Deterministic intra-only encoder configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntraEncoder {
    sequence: SequenceHeader,
    qp: u8,
}

impl IntraEncoder {
    pub fn new(sequence: SequenceHeader, qp: u8) -> Result<Self, EncodeError> {
        if qp > 63 {
            return Err(EncodeError::InvalidInput {
                element: "frame.qp",
            });
        }
        Ok(Self { sequence, qp })
    }

    /// Encodes all input frames as independent, canonical keyframes.
    pub fn encode(&self, frames: &[Frame]) -> Result<EncodedStream, EncodeError> {
        if frames.is_empty() {
            return Err(EncodeError::InvalidInput {
                element: "frames.empty",
            });
        }
        let mut bytes = self.sequence.encode().to_vec();
        let mut accounting = Vec::with_capacity(frames.len());
        let mut reconstructed_frames = Vec::with_capacity(frames.len());
        let mut checkpoints = Vec::new();
        let mut coverage = CoverageCounter::new();
        for (frame_index, source) in frames.iter().enumerate() {
            if source.width() != u32::from(self.sequence.width)
                || source.height() != u32::from(self.sequence.height)
            {
                return Err(EncodeError::InvalidInput {
                    element: "frame.dimensions",
                });
            }
            let frame_index =
                u32::try_from(frame_index).map_err(|_| EncodeError::InvalidInput {
                    element: "frame.count",
                })?;
            let encoded = self.encode_frame(source, frame_index)?;
            bytes.extend_from_slice(&encoded.packet.encode());
            accounting.push(encoded.accounting);
            reconstructed_frames.push(encoded.reconstructed);
            checkpoints.extend(encoded.checkpoints);
            coverage.merge(&encoded.coverage);
        }
        Ok(EncodedStream {
            bytes,
            frames: accounting,
            reconstructed_frames,
            checkpoints,
            coverage,
        })
    }

    fn encode_frame(
        &self,
        source: &Frame,
        frame_index: u32,
    ) -> Result<EncodedVideoFrame, EncodeError> {
        let padded_width = source.width().div_ceil(64) * 64;
        let padded_height = source.height().div_ceil(64) * 64;
        let source = source
            .pad_420_edge(padded_width, padded_height)
            .map_err(|_| reconstruction("source.pad"))?;
        let mut reconstructed = Frame::filled_420(padded_width, padded_height, 0)
            .map_err(|_| reconstruction("reconstruction.allocate"))?;
        let mut motion_field = MotionField::new(padded_width, padded_height)
            .map_err(|_| reconstruction("motion_field.allocate"))?;
        let mut writer = SyntaxWriter::new(ContextBank::initial());
        let mut superblocks = Vec::new();
        let mut checkpoints = Vec::new();
        for y in (0..padded_height).step_by(64) {
            for x in (0..padded_width).step_by(64) {
                let choice = select_partition(
                    writer.contexts().clone(),
                    &source,
                    &reconstructed,
                    x,
                    y,
                    BlockSize::N64,
                    self.qp,
                    FrameType::Key,
                    None,
                    &motion_field,
                )?;
                let structure_before = writer.stats().clone();
                writer.write_partition(&choice.tree)?;
                let structure_after = writer.stats().clone();
                let mut blocks = Vec::with_capacity(choice.blocks.len());
                for candidate in &choice.blocks {
                    let block_before = writer.stats().clone();
                    write_candidate(&mut writer, candidate, FrameType::Key)?;
                    let block_after = writer.stats().clone();
                    blocks.push(BlockAccounting {
                        x: candidate.x,
                        y: candidate.y,
                        size: candidate.size.side(),
                        prediction: candidate.prediction,
                        motion_vector_q4: candidate
                            .motion_vector
                            .map(|motion| [motion.x_q4, motion.y_q4]),
                        modeled_entropy_q16: block_after
                            .modeled_entropy_q16
                            .saturating_sub(block_before.modeled_entropy_q16),
                        emitted_payload_bytes: block_after
                            .emitted_bytes
                            .saturating_sub(block_before.emitted_bytes),
                    });
                }
                debug_assert_eq!(writer.contexts(), &choice.contexts);
                reconstructed = choice.reconstructed;
                motion_field = choice.motion_field;
                checkpoints.push(writer.contexts().p1_values());
                superblocks.push(SuperblockAccounting {
                    x,
                    y,
                    structure_modeled_entropy_q16: structure_after
                        .modeled_entropy_q16
                        .saturating_sub(structure_before.modeled_entropy_q16),
                    structure_emitted_payload_bytes: structure_after
                        .emitted_bytes
                        .saturating_sub(structure_before.emitted_bytes),
                    blocks,
                });
            }
        }
        let coverage = writer.coverage().clone();
        let (encoded, _contexts) = writer.finish();
        let frame_flush_bytes = encoded
            .stats
            .emission_events
            .iter()
            .filter(|event| event.finalization)
            .map(|event| u64::from(event.bytes))
            .sum();
        let packet = FramePacket::new(
            frame_index,
            FrameFlags {
                key: true,
                golden_refresh: true,
                show: true,
            },
            self.qp,
            encoded.bytes,
        )?;
        let stats = FrameAccounting {
            frame_index,
            key: true,
            golden_refresh: true,
            qp: self.qp,
            payload_len: packet.payload.len(),
            frame_flush_bytes,
            superblocks,
        };
        let visible = reconstructed
            .crop_420(
                u32::from(self.sequence.width),
                u32::from(self.sequence.height),
            )
            .map_err(|_| reconstruction("reconstruction.crop"))?;
        Ok(EncodedVideoFrame {
            packet,
            accounting: stats,
            reconstructed: visible,
            contexts: ContextBank::initial(),
            checkpoints,
            coverage,
        })
    }
}

struct EncodedVideoFrame {
    packet: FramePacket,
    accounting: FrameAccounting,
    reconstructed: Frame,
    contexts: ContextBank,
    checkpoints: Vec<[u16; 144]>,
    coverage: CoverageCounter,
}

#[allow(clippy::too_many_arguments)]
fn encode_video_frame(
    source: &Frame,
    frame_index: u32,
    qp: u8,
    frame_type: FrameType,
    decision: FrameDecision,
    contexts: ContextBank,
    references: Option<ReferenceFrames<'_>>,
) -> Result<EncodedVideoFrame, EncodeError> {
    if frame_type == FrameType::P && references.is_none() {
        return Err(reconstruction("reference.missing"));
    }
    let padded_width = source.width().div_ceil(64) * 64;
    let padded_height = source.height().div_ceil(64) * 64;
    let source = source
        .pad_420_edge(padded_width, padded_height)
        .map_err(|_| reconstruction("source.pad"))?;
    let mut reconstructed = Frame::filled_420(padded_width, padded_height, 0)
        .map_err(|_| reconstruction("reconstruction.allocate"))?;
    let mut motion_field = MotionField::new(padded_width, padded_height)
        .map_err(|_| reconstruction("motion_field.allocate"))?;
    let mut writer = SyntaxWriter::new(contexts);
    let mut superblocks = Vec::new();
    let mut checkpoints = Vec::new();
    for y in (0..padded_height).step_by(64) {
        for x in (0..padded_width).step_by(64) {
            let choice = select_partition(
                writer.contexts().clone(),
                &source,
                &reconstructed,
                x,
                y,
                BlockSize::N64,
                qp,
                frame_type,
                references,
                &motion_field,
            )?;
            let structure_before = writer.stats().clone();
            writer.write_partition(&choice.tree)?;
            let structure_after = writer.stats().clone();
            let mut blocks = Vec::with_capacity(choice.blocks.len());
            for candidate in &choice.blocks {
                let block_before = writer.stats().clone();
                write_candidate(&mut writer, candidate, frame_type)?;
                let block_after = writer.stats().clone();
                blocks.push(BlockAccounting {
                    x: candidate.x,
                    y: candidate.y,
                    size: candidate.size.side(),
                    prediction: candidate.prediction,
                    motion_vector_q4: candidate
                        .motion_vector
                        .map(|motion| [motion.x_q4, motion.y_q4]),
                    modeled_entropy_q16: block_after
                        .modeled_entropy_q16
                        .saturating_sub(block_before.modeled_entropy_q16),
                    emitted_payload_bytes: block_after
                        .emitted_bytes
                        .saturating_sub(block_before.emitted_bytes),
                });
            }
            debug_assert_eq!(writer.contexts(), &choice.contexts);
            reconstructed = choice.reconstructed;
            motion_field = choice.motion_field;
            checkpoints.push(writer.contexts().p1_values());
            superblocks.push(SuperblockAccounting {
                x,
                y,
                structure_modeled_entropy_q16: structure_after
                    .modeled_entropy_q16
                    .saturating_sub(structure_before.modeled_entropy_q16),
                structure_emitted_payload_bytes: structure_after
                    .emitted_bytes
                    .saturating_sub(structure_before.emitted_bytes),
                blocks,
            });
        }
    }
    let coverage = writer.coverage().clone();
    let (encoded, contexts) = writer.finish();
    let frame_flush_bytes = encoded
        .stats
        .emission_events
        .iter()
        .filter(|event| event.finalization)
        .map(|event| u64::from(event.bytes))
        .sum();
    let packet = FramePacket::new(
        frame_index,
        FrameFlags {
            key: decision.key,
            golden_refresh: decision.golden_refresh,
            show: true,
        },
        qp,
        encoded.bytes,
    )?;
    let accounting = FrameAccounting {
        frame_index,
        key: decision.key,
        golden_refresh: decision.golden_refresh,
        qp,
        payload_len: packet.payload.len(),
        frame_flush_bytes,
        superblocks,
    };
    Ok(EncodedVideoFrame {
        packet,
        accounting,
        reconstructed,
        contexts,
        checkpoints,
        coverage,
    })
}

fn validate_sources(sequence: &SequenceHeader, frames: &[Frame]) -> Result<(), EncodeError> {
    if frames.iter().any(|frame| {
        frame.width() != u32::from(sequence.width) || frame.height() != u32::from(sequence.height)
    }) {
        return Err(EncodeError::InvalidInput {
            element: "frame.dimensions",
        });
    }
    Ok(())
}

fn frame_luma_sad(previous: &Frame, current: &Frame) -> Result<u64, EncodeError> {
    if previous.width() != current.width() || previous.height() != current.height() {
        return Err(EncodeError::InvalidInput {
            element: "frame.dimensions",
        });
    }
    let mut sad = 0_u64;
    for y in 0..current.height() {
        for x in 0..current.width() {
            let previous = previous
                .y
                .get(x, y)
                .map_err(|_| reconstruction("scene.previous"))?;
            let current = current
                .y
                .get(x, y)
                .map_err(|_| reconstruction("scene.current"))?;
            sad = sad.saturating_add(u64::from(previous.abs_diff(current)));
        }
    }
    Ok(sad)
}

struct Candidate {
    x: u32,
    y: u32,
    size: BlockSize,
    prediction: Prediction,
    motion_vector: Option<PredictMotionVector>,
    levels: Vec<(PlaneClass, TransformBlockSize, Vec<i32>)>,
    reconstructed: Frame,
    motion_field: MotionField,
    distortion: u64,
    modeled_entropy_q16: u64,
}

#[derive(Clone, Copy)]
struct ReferenceFrames<'a> {
    last: &'a Frame,
    golden: &'a Frame,
}

impl<'a> ReferenceFrames<'a> {
    const fn get(self, reference: kf_bitstream::ReferenceFrame) -> &'a Frame {
        match reference {
            kf_bitstream::ReferenceFrame::Last => self.last,
            kf_bitstream::ReferenceFrame::Golden => self.golden,
        }
    }
}

struct PartitionChoice {
    tree: PartitionTree,
    blocks: Vec<Candidate>,
    reconstructed: Frame,
    motion_field: MotionField,
    contexts: ContextBank,
    distortion: u64,
    modeled_entropy_q16: u64,
}

#[allow(clippy::too_many_arguments)]
fn select_partition(
    contexts: ContextBank,
    source: &Frame,
    reconstructed: &Frame,
    x: u32,
    y: u32,
    size: BlockSize,
    qp: u8,
    frame_type: FrameType,
    references: Option<ReferenceFrames<'_>>,
    motion_field: &MotionField,
) -> Result<PartitionChoice, EncodeError> {
    let (leaf_contexts, leaf_structure_cost) = partition_transition(contexts.clone(), size, false)?;
    let leaf_writer = SyntaxWriter::new(leaf_contexts.clone());
    let leaf = select_prediction(
        &leaf_writer,
        source,
        reconstructed,
        x,
        y,
        size,
        qp,
        frame_type,
        references,
        motion_field,
    )?;
    let (leaf_contexts, leaf_modeled_cost) =
        candidate_transition(leaf_contexts, &leaf, frame_type)?;
    let mut best = PartitionChoice {
        tree: PartitionTree::Leaf(size),
        distortion: leaf.distortion,
        modeled_entropy_q16: leaf_structure_cost.saturating_add(leaf_modeled_cost),
        reconstructed: leaf.reconstructed.clone(),
        motion_field: leaf.motion_field.clone(),
        contexts: leaf_contexts,
        blocks: vec![leaf],
    };

    if let Ok(child_size) = size.child() {
        let (mut split_contexts, split_structure_cost) =
            partition_transition(contexts, size, true)?;
        let mut split_reconstruction = reconstructed.clone();
        let mut split_motion_field = motion_field.clone();
        let mut split_distortion = 0_u64;
        let mut split_modeled_cost = split_structure_cost;
        let mut split_blocks = Vec::new();
        let half = u32::from(size.side() / 2);
        let offsets = [(0, 0), (half, 0), (0, half), (half, half)];
        let mut children = Vec::with_capacity(4);
        for (dx, dy) in offsets {
            let child = select_partition(
                split_contexts,
                source,
                &split_reconstruction,
                x + dx,
                y + dy,
                child_size,
                qp,
                frame_type,
                references,
                &split_motion_field,
            )?;
            split_contexts = child.contexts;
            split_reconstruction = child.reconstructed;
            split_motion_field = child.motion_field;
            split_distortion = split_distortion.saturating_add(child.distortion);
            split_modeled_cost = split_modeled_cost.saturating_add(child.modeled_entropy_q16);
            split_blocks.extend(child.blocks);
            children.push(child.tree);
        }
        let split = PartitionChoice {
            tree: PartitionTree::Split {
                size,
                children: Box::new(
                    children
                        .try_into()
                        .expect("invariant: a split has four raster children"),
                ),
            },
            blocks: split_blocks,
            reconstructed: split_reconstruction,
            motion_field: split_motion_field,
            contexts: split_contexts,
            distortion: split_distortion,
            modeled_entropy_q16: split_modeled_cost,
        };
        let lambda = u64::from(lambda_q8(qp).map_err(|_| reconstruction("rdo.lambda"))?);
        if rd_cost(split.distortion, split.modeled_entropy_q16, lambda)
            < rd_cost(best.distortion, best.modeled_entropy_q16, lambda)
        {
            best = split;
        }
    }
    Ok(best)
}

fn partition_transition(
    contexts: ContextBank,
    size: BlockSize,
    split: bool,
) -> Result<(ContextBank, u64), EncodeError> {
    let mut writer = SyntaxWriter::new(contexts);
    writer.write_partition_decision(size, split)?;
    let modeled = writer.stats().modeled_entropy_q16;
    let (_encoded, contexts) = writer.finish();
    Ok((contexts, modeled))
}

fn candidate_transition(
    contexts: ContextBank,
    candidate: &Candidate,
    frame_type: FrameType,
) -> Result<(ContextBank, u64), EncodeError> {
    let mut writer = SyntaxWriter::new(contexts);
    write_candidate(&mut writer, candidate, frame_type)?;
    let modeled = writer.stats().modeled_entropy_q16;
    let (_encoded, contexts) = writer.finish();
    Ok((contexts, modeled))
}

#[allow(clippy::too_many_arguments)]
fn select_prediction(
    writer: &SyntaxWriter,
    source: &Frame,
    reconstructed: &Frame,
    x: u32,
    y: u32,
    size: BlockSize,
    qp: u8,
    frame_type: FrameType,
    references: Option<ReferenceFrames<'_>>,
    motion_field: &MotionField,
) -> Result<Candidate, EncodeError> {
    let lambda = u64::from(lambda_q8(qp).map_err(|_| reconstruction("rdo.lambda"))?);
    let mut best: Option<(u128, Candidate)> = None;
    if frame_type == FrameType::P {
        let references = references.ok_or_else(|| reconstruction("reference.missing"))?;
        for reference in [
            kf_bitstream::ReferenceFrame::Last,
            kf_bitstream::ReferenceFrame::Golden,
        ] {
            let predictor = motion_field
                .predictor(x, y, u32::from(size.side()), reference_slot(reference))
                .map_err(|_| reconstruction("motion.predictor"))?;
            let motion_vector = clamp_motion_vector(
                &references.get(reference).y,
                x,
                y,
                u32::from(size.side()),
                predictor,
                PlaneScale::Luma,
            )
            .map_err(|_| reconstruction("motion.clamp"))?;
            let candidate = build_inter_candidate(
                source,
                reconstructed,
                references.get(reference),
                motion_field,
                x,
                y,
                size,
                qp,
                Prediction::Skip { reference },
                motion_vector,
                true,
            )?;
            consider_candidate(writer, frame_type, candidate, lambda, &mut best)?;
        }
        for reference in [
            kf_bitstream::ReferenceFrame::Last,
            kf_bitstream::ReferenceFrame::Golden,
        ] {
            let predictor = motion_field
                .predictor(x, y, u32::from(size.side()), reference_slot(reference))
                .map_err(|_| reconstruction("motion.predictor"))?;
            let search = estimate_motion(
                &source.y,
                &references.get(reference).y,
                x,
                y,
                u32::from(size.side()),
                predictor,
            )?;
            let mvd = kf_bitstream::MotionVector {
                x_q4: i16::try_from(search.motion_vector.x_q4.saturating_sub(predictor.x_q4))
                    .map_err(|_| reconstruction("motion.mvd"))?,
                y_q4: i16::try_from(search.motion_vector.y_q4.saturating_sub(predictor.y_q4))
                    .map_err(|_| reconstruction("motion.mvd"))?,
            };
            let candidate = build_inter_candidate(
                source,
                reconstructed,
                references.get(reference),
                motion_field,
                x,
                y,
                size,
                qp,
                Prediction::Inter { reference, mvd },
                search.motion_vector,
                false,
            )?;
            consider_candidate(writer, frame_type, candidate, lambda, &mut best)?;
        }
    }
    for mode in [
        IntraMode::Dc,
        IntraMode::Planar,
        IntraMode::Horizontal,
        IntraMode::Vertical,
        IntraMode::D45,
        IntraMode::D135,
        IntraMode::D117,
        IntraMode::D153,
    ] {
        let candidate =
            build_intra_candidate(source, reconstructed, motion_field, x, y, size, qp, mode)?;
        consider_candidate(writer, frame_type, candidate, lambda, &mut best)?;
    }
    Ok(best
        .expect("invariant: closed intra mode set is nonempty")
        .1)
}

fn consider_candidate(
    writer: &SyntaxWriter,
    frame_type: FrameType,
    mut candidate: Candidate,
    lambda: u64,
    best: &mut Option<(u128, Candidate)>,
) -> Result<(), EncodeError> {
    let mut shadow = writer.clone();
    let before = shadow.stats().modeled_entropy_q16;
    write_candidate(&mut shadow, &candidate, frame_type)?;
    candidate.modeled_entropy_q16 = shadow.stats().modeled_entropy_q16.saturating_sub(before);
    let candidate_cost = rd_cost(candidate.distortion, candidate.modeled_entropy_q16, lambda);
    if best
        .as_ref()
        .is_none_or(|(best_cost, _)| candidate_cost < *best_cost)
    {
        *best = Some((candidate_cost, candidate));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_intra_candidate(
    source: &Frame,
    reconstructed: &Frame,
    motion_field: &MotionField,
    x: u32,
    y: u32,
    size: BlockSize,
    qp: u8,
    mode: IntraMode,
) -> Result<Candidate, EncodeError> {
    let mut output = reconstructed.clone();
    let mut levels = Vec::with_capacity(6);
    let side = u32::from(size.side());
    let mut distortion = encode_plane(
        &source.y,
        &mut output.y,
        x,
        y,
        side,
        qp,
        mode,
        PlaneClass::Luma,
        &mut levels,
    )?;
    distortion = distortion.saturating_add(encode_plane(
        &source.cb,
        &mut output.cb,
        x / 2,
        y / 2,
        side / 2,
        qp,
        mode,
        PlaneClass::Chroma,
        &mut levels,
    )?);
    distortion = distortion.saturating_add(encode_plane(
        &source.cr,
        &mut output.cr,
        x / 2,
        y / 2,
        side / 2,
        qp,
        mode,
        PlaneClass::Chroma,
        &mut levels,
    )?);
    let mut candidate_motion_field = motion_field.clone();
    candidate_motion_field
        .record(x, y, side, BlockMotion::Intra)
        .map_err(|_| reconstruction("motion_field.record"))?;
    Ok(Candidate {
        x,
        y,
        size,
        prediction: Prediction::Intra(mode),
        motion_vector: None,
        levels,
        reconstructed: output,
        motion_field: candidate_motion_field,
        distortion,
        modeled_entropy_q16: 0,
    })
}

#[allow(clippy::too_many_arguments)]
fn build_inter_candidate(
    source: &Frame,
    reconstructed: &Frame,
    reference_frame: &Frame,
    motion_field: &MotionField,
    x: u32,
    y: u32,
    size: BlockSize,
    qp: u8,
    prediction_kind: Prediction,
    motion_vector: PredictMotionVector,
    skip: bool,
) -> Result<Candidate, EncodeError> {
    let mut output = reconstructed.clone();
    let mut levels = Vec::with_capacity(6);
    let side = u32::from(size.side());
    let luma_prediction = predict_inter(
        &reference_frame.y,
        x,
        y,
        side,
        motion_vector,
        PlaneScale::Luma,
    )
    .map_err(|_| reconstruction("inter.predict_luma"))?;
    let cb_prediction = predict_inter(
        &reference_frame.cb,
        x / 2,
        y / 2,
        side / 2,
        motion_vector,
        PlaneScale::Chroma420,
    )
    .map_err(|_| reconstruction("inter.predict_chroma"))?;
    let cr_prediction = predict_inter(
        &reference_frame.cr,
        x / 2,
        y / 2,
        side / 2,
        motion_vector,
        PlaneScale::Chroma420,
    )
    .map_err(|_| reconstruction("inter.predict_chroma"))?;
    let mut distortion = if skip {
        install_prediction(&source.y, &mut output.y, x, y, side, &luma_prediction)?
    } else {
        encode_predicted_plane(
            &source.y,
            &mut output.y,
            x,
            y,
            side,
            qp,
            PlaneClass::Luma,
            &luma_prediction,
            &mut levels,
        )?
    };
    for (source_plane, output_plane, predicted) in [
        (&source.cb, &mut output.cb, cb_prediction),
        (&source.cr, &mut output.cr, cr_prediction),
    ] {
        let plane_distortion = if skip {
            install_prediction(
                source_plane,
                output_plane,
                x / 2,
                y / 2,
                side / 2,
                &predicted,
            )?
        } else {
            encode_predicted_plane(
                source_plane,
                output_plane,
                x / 2,
                y / 2,
                side / 2,
                qp,
                PlaneClass::Chroma,
                &predicted,
                &mut levels,
            )?
        };
        distortion = distortion.saturating_add(plane_distortion);
    }
    let reference = match prediction_kind {
        Prediction::Skip { reference } | Prediction::Inter { reference, .. } => reference,
        Prediction::Intra(_) => return Err(reconstruction("inter.prediction_kind")),
    };
    let mut candidate_motion_field = motion_field.clone();
    candidate_motion_field
        .record(
            x,
            y,
            side,
            BlockMotion::Inter {
                reference: reference_slot(reference),
                motion_vector,
            },
        )
        .map_err(|_| reconstruction("motion_field.record"))?;
    Ok(Candidate {
        x,
        y,
        size,
        prediction: prediction_kind,
        motion_vector: Some(motion_vector),
        levels,
        reconstructed: output,
        motion_field: candidate_motion_field,
        distortion,
        modeled_entropy_q16: 0,
    })
}

fn rd_cost(distortion: u64, modeled_entropy_q16: u64, lambda_q8: u64) -> u128 {
    let rate_cost = u128::from(lambda_q8) * u128::from(modeled_entropy_q16);
    u128::from(distortion) + ((rate_cost + (1 << 23)) >> 24)
}

#[allow(clippy::too_many_arguments)]
fn encode_plane(
    source: &Plane,
    reconstructed: &mut Plane,
    x: u32,
    y: u32,
    prediction_side: u32,
    qp: u8,
    mode: IntraMode,
    plane_class: PlaneClass,
    levels_out: &mut Vec<(PlaneClass, TransformBlockSize, Vec<i32>)>,
) -> Result<u64, EncodeError> {
    let prediction = predict_intra(reconstructed, x, y, prediction_side, predict_mode(mode))
        .map_err(|_| reconstruction("intra.predict"))?;
    encode_predicted_plane(
        source,
        reconstructed,
        x,
        y,
        prediction_side,
        qp,
        plane_class,
        &prediction,
        levels_out,
    )
}

#[allow(clippy::too_many_arguments)]
fn encode_predicted_plane(
    source: &Plane,
    reconstructed: &mut Plane,
    x: u32,
    y: u32,
    prediction_side: u32,
    qp: u8,
    plane_class: PlaneClass,
    prediction: &[u8],
    levels_out: &mut Vec<(PlaneClass, TransformBlockSize, Vec<i32>)>,
) -> Result<u64, EncodeError> {
    let transform_side = prediction_side.min(32);
    let transform = TransformSize::try_from(u8::try_from(transform_side).unwrap())
        .map_err(|_| reconstruction("transform.size"))?;
    let syntax_transform = match transform_side {
        4 => TransformBlockSize::N4,
        8 => TransformBlockSize::N8,
        16 => TransformBlockSize::N16,
        32 => TransformBlockSize::N32,
        _ => return Err(reconstruction("syntax.transform_size")),
    };
    let blocks_per_axis = prediction_side / transform_side;
    let mut distortion = 0_u64;
    for transform_y in 0..blocks_per_axis {
        for transform_x in 0..blocks_per_axis {
            let mut residual = Vec::with_capacity(usize::try_from(transform_side.pow(2)).unwrap());
            for row in 0..transform_side {
                for column in 0..transform_side {
                    let source_sample = source
                        .get(
                            x + transform_x * transform_side + column,
                            y + transform_y * transform_side + row,
                        )
                        .map_err(|_| reconstruction("source.read"))?;
                    let prediction_index = usize::try_from(
                        (transform_y * transform_side + row) * prediction_side
                            + transform_x * transform_side
                            + column,
                    )
                    .unwrap();
                    residual
                        .push(i32::from(source_sample) - i32::from(prediction[prediction_index]));
                }
            }
            let coefficients = forward_transform(&residual, transform)
                .map_err(|_| reconstruction("transform.forward"))?;
            let levels =
                quantize_block(&coefficients, qp).map_err(|_| reconstruction("quantize"))?;
            let dequantized =
                dequantize_block(&levels, qp).map_err(|_| reconstruction("dequantize"))?;
            let decoded = inverse_transform(&dequantized, transform)
                .map_err(|_| reconstruction("transform.inverse"))?;
            let transform_side_usize = usize::try_from(transform_side).unwrap();
            for row in 0..transform_side_usize {
                for column in 0..transform_side_usize {
                    let prediction_index =
                        (usize::try_from(transform_y).unwrap() * transform_side_usize + row)
                            * usize::try_from(prediction_side).unwrap()
                            + usize::try_from(transform_x).unwrap() * transform_side_usize
                            + column;
                    let reconstructed_sample = (i32::from(prediction[prediction_index])
                        + decoded[row * transform_side_usize + column])
                        .clamp(0, 255);
                    let destination_x =
                        x + transform_x * transform_side + u32::try_from(column).unwrap();
                    let destination_y =
                        y + transform_y * transform_side + u32::try_from(row).unwrap();
                    reconstructed
                        .set(
                            destination_x,
                            destination_y,
                            u8::try_from(reconstructed_sample).unwrap(),
                        )
                        .map_err(|_| reconstruction("reconstruction.write"))?;
                    let original = i32::from(
                        source
                            .get(destination_x, destination_y)
                            .map_err(|_| reconstruction("source.read"))?,
                    );
                    let difference = original - reconstructed_sample;
                    distortion =
                        distortion.saturating_add(u64::from(difference.unsigned_abs()).pow(2));
                }
            }
            levels_out.push((plane_class, syntax_transform, levels));
        }
    }
    Ok(distortion)
}

fn install_prediction(
    source: &Plane,
    reconstructed: &mut Plane,
    x: u32,
    y: u32,
    side: u32,
    prediction: &[u8],
) -> Result<u64, EncodeError> {
    let side_usize = usize::try_from(side).map_err(|_| reconstruction("prediction.size"))?;
    let mut distortion = 0_u64;
    for row in 0..side_usize {
        for column in 0..side_usize {
            let destination_x =
                x + u32::try_from(column).map_err(|_| reconstruction("prediction.x"))?;
            let destination_y =
                y + u32::try_from(row).map_err(|_| reconstruction("prediction.y"))?;
            let predicted = prediction[row * side_usize + column];
            let original = source
                .get(destination_x, destination_y)
                .map_err(|_| reconstruction("source.read"))?;
            let difference = original.abs_diff(predicted);
            distortion = distortion.saturating_add(u64::from(difference).pow(2));
            reconstructed
                .set(destination_x, destination_y, predicted)
                .map_err(|_| reconstruction("reconstruction.write"))?;
        }
    }
    Ok(distortion)
}

fn write_candidate(
    writer: &mut SyntaxWriter,
    candidate: &Candidate,
    frame_type: FrameType,
) -> Result<(), EncodeError> {
    writer.write_prediction(frame_type, candidate.prediction)?;
    for (plane, size, levels) in &candidate.levels {
        writer.write_coefficients(*plane, *size, levels)?;
    }
    Ok(())
}

const fn reference_slot(reference: kf_bitstream::ReferenceFrame) -> ReferenceSlot {
    match reference {
        kf_bitstream::ReferenceFrame::Last => ReferenceSlot::Last,
        kf_bitstream::ReferenceFrame::Golden => ReferenceSlot::Golden,
    }
}

const fn predict_mode(mode: IntraMode) -> PredictMode {
    match mode {
        IntraMode::Dc => PredictMode::Dc,
        IntraMode::Planar => PredictMode::Planar,
        IntraMode::Horizontal => PredictMode::Horizontal,
        IntraMode::Vertical => PredictMode::Vertical,
        IntraMode::D45 => PredictMode::D45,
        IntraMode::D135 => PredictMode::D135,
        IntraMode::D117 => PredictMode::D117,
        IntraMode::D153 => PredictMode::D153,
    }
}

const fn reconstruction(element: &'static str) -> EncodeError {
    EncodeError::Reconstruction { element }
}

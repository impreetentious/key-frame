use kf_bitstream::{
    BlockSize, FrameType, IntraMode, PartitionTree, PlaneClass, Prediction, SequenceHeader,
    SyntaxReader, TransformBlockSize,
};
use kf_frame::{Frame, Plane};
use kf_predict::{
    BlockMotion, IntraMode as PredictMode, MotionField, MotionVector, PlaneScale, ReferenceSlot,
    clamp_motion_vector, predict_inter, predict_intra,
};
use kf_range::ContextBank;
use kf_transform::{TransformSize, dequantize_block, inverse_transform};

use crate::DecodeError;

#[derive(Clone, Copy)]
pub(crate) struct ReferenceFrames<'a> {
    pub(crate) last: &'a Frame,
    pub(crate) golden: &'a Frame,
}

impl<'a> ReferenceFrames<'a> {
    const fn get(self, reference: kf_bitstream::ReferenceFrame) -> &'a Frame {
        match reference {
            kf_bitstream::ReferenceFrame::Last => self.last,
            kf_bitstream::ReferenceFrame::Golden => self.golden,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn decode_payload(
    sequence: &SequenceHeader,
    payload: &[u8],
    frame_index: u32,
    qp: u8,
    contexts: ContextBank,
    frame_type: FrameType,
    references: Option<ReferenceFrames<'_>>,
) -> Result<(Frame, ContextBank, Vec<[u16; 144]>), DecodeError> {
    if frame_type == FrameType::P && references.is_none() {
        return Err(invalid(frame_index, "reference.missing"));
    }
    let padded_width = u32::from(sequence.width).div_ceil(64) * 64;
    let padded_height = u32::from(sequence.height).div_ceil(64) * 64;
    let mut frame = Frame::filled_420(padded_width, padded_height, 0)
        .map_err(|_| invalid(frame_index, "frame.allocate"))?;
    let mut motion_field = MotionField::new(padded_width, padded_height)
        .map_err(|_| invalid(frame_index, "motion_field.allocate"))?;
    let mut reader = SyntaxReader::new(payload, contexts)?;
    let mut checkpoints = Vec::new();

    for superblock_y in (0..padded_height).step_by(64) {
        for superblock_x in (0..padded_width).step_by(64) {
            let partition = reader.read_partition()?;
            for (x, y, size) in block_positions(&partition, superblock_x, superblock_y) {
                let prediction = reader.read_prediction(frame_type)?;
                reconstruct_block(
                    &mut reader,
                    &mut frame,
                    &mut motion_field,
                    references,
                    x,
                    y,
                    size,
                    prediction,
                    qp,
                    frame_index,
                )?;
            }
            checkpoints.push(reader.contexts().p1_values());
        }
    }
    let final_contexts = reader.into_contexts();
    Ok((frame, final_contexts, checkpoints))
}

#[allow(clippy::too_many_arguments)]
fn reconstruct_block(
    reader: &mut SyntaxReader<'_>,
    frame: &mut Frame,
    motion_field: &mut MotionField,
    references: Option<ReferenceFrames<'_>>,
    x: u32,
    y: u32,
    size: BlockSize,
    prediction: Prediction,
    qp: u8,
    frame_index: u32,
) -> Result<(), DecodeError> {
    let side = u32::from(size.side());
    match prediction {
        Prediction::Intra(mode) => {
            reconstruct_intra_plane(
                reader,
                &mut frame.y,
                x,
                y,
                side,
                mode,
                qp,
                PlaneClass::Luma,
                frame_index,
            )?;
            for plane in [&mut frame.cb, &mut frame.cr] {
                reconstruct_intra_plane(
                    reader,
                    plane,
                    x / 2,
                    y / 2,
                    side / 2,
                    mode,
                    qp,
                    PlaneClass::Chroma,
                    frame_index,
                )?;
            }
            motion_field
                .record(x, y, side, BlockMotion::Intra)
                .map_err(|_| invalid(frame_index, "motion_field.record"))?;
        }
        Prediction::Skip { reference } | Prediction::Inter { reference, .. } => {
            let references = references.ok_or_else(|| invalid(frame_index, "reference.missing"))?;
            let predictor = motion_field
                .predictor(x, y, side, reference_slot(reference))
                .map_err(|_| invalid(frame_index, "motion.predictor"))?;
            let requested = match prediction {
                Prediction::Skip { .. } => predictor,
                Prediction::Inter { mvd, .. } => MotionVector {
                    x_q4: predictor.x_q4.saturating_add(i32::from(mvd.x_q4)),
                    y_q4: predictor.y_q4.saturating_add(i32::from(mvd.y_q4)),
                },
                Prediction::Intra(_) => unreachable!("matched inter branches"),
            };
            let motion_vector = clamp_motion_vector(
                &references.get(reference).y,
                x,
                y,
                side,
                requested,
                PlaneScale::Luma,
            )
            .map_err(|_| invalid(frame_index, "motion.clamp"))?;
            let skip = matches!(prediction, Prediction::Skip { .. });
            reconstruct_inter_plane(
                reader,
                &mut frame.y,
                &references.get(reference).y,
                x,
                y,
                side,
                motion_vector,
                PlaneScale::Luma,
                qp,
                PlaneClass::Luma,
                frame_index,
                skip,
            )?;
            for (plane, reference_plane) in [
                (&mut frame.cb, &references.get(reference).cb),
                (&mut frame.cr, &references.get(reference).cr),
            ] {
                reconstruct_inter_plane(
                    reader,
                    plane,
                    reference_plane,
                    x / 2,
                    y / 2,
                    side / 2,
                    motion_vector,
                    PlaneScale::Chroma420,
                    qp,
                    PlaneClass::Chroma,
                    frame_index,
                    skip,
                )?;
            }
            motion_field
                .record(
                    x,
                    y,
                    side,
                    BlockMotion::Inter {
                        reference: reference_slot(reference),
                        motion_vector,
                    },
                )
                .map_err(|_| invalid(frame_index, "motion_field.record"))?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn reconstruct_intra_plane(
    reader: &mut SyntaxReader<'_>,
    plane: &mut Plane,
    x: u32,
    y: u32,
    prediction_side: u32,
    mode: IntraMode,
    qp: u8,
    plane_class: PlaneClass,
    frame_index: u32,
) -> Result<(), DecodeError> {
    let prediction = predict_intra(plane, x, y, prediction_side, predict_mode(mode))
        .map_err(|_| invalid(frame_index, "intra.predict"))?;
    reconstruct_residual_plane(
        reader,
        plane,
        x,
        y,
        prediction_side,
        qp,
        plane_class,
        frame_index,
        &prediction,
    )
}

#[allow(clippy::too_many_arguments)]
fn reconstruct_inter_plane(
    reader: &mut SyntaxReader<'_>,
    plane: &mut Plane,
    reference: &Plane,
    x: u32,
    y: u32,
    prediction_side: u32,
    motion_vector: MotionVector,
    scale: PlaneScale,
    qp: u8,
    plane_class: PlaneClass,
    frame_index: u32,
    skip: bool,
) -> Result<(), DecodeError> {
    let prediction = predict_inter(reference, x, y, prediction_side, motion_vector, scale)
        .map_err(|_| invalid(frame_index, "inter.predict"))?;
    if skip {
        return add_prediction(plane, x, y, prediction_side, &prediction, frame_index);
    }
    reconstruct_residual_plane(
        reader,
        plane,
        x,
        y,
        prediction_side,
        qp,
        plane_class,
        frame_index,
        &prediction,
    )
}

#[allow(clippy::too_many_arguments)]
fn reconstruct_residual_plane(
    reader: &mut SyntaxReader<'_>,
    plane: &mut Plane,
    x: u32,
    y: u32,
    prediction_side: u32,
    qp: u8,
    plane_class: PlaneClass,
    frame_index: u32,
    prediction: &[u8],
) -> Result<(), DecodeError> {
    let transform_side = prediction_side.min(32);
    let transform = transform_size(transform_side, frame_index)?;
    let syntax_transform = syntax_transform_size(transform_side, frame_index)?;
    let blocks_per_axis = prediction_side / transform_side;
    for transform_y in 0..blocks_per_axis {
        for transform_x in 0..blocks_per_axis {
            let levels = reader.read_coefficients(plane_class, syntax_transform)?;
            let coefficients = dequantize_block(&levels, qp)
                .map_err(|_| invalid(frame_index, "residual.dequantize"))?;
            let residual = inverse_transform(&coefficients, transform)
                .map_err(|_| invalid(frame_index, "residual.inverse_transform"))?;
            add_residual(
                plane,
                prediction,
                prediction_side,
                x + transform_x * transform_side,
                y + transform_y * transform_side,
                transform_x * transform_side,
                transform_y * transform_side,
                transform_side,
                &residual,
            )?;
        }
    }
    Ok(())
}

fn add_prediction(
    plane: &mut Plane,
    x: u32,
    y: u32,
    side: u32,
    prediction: &[u8],
    frame_index: u32,
) -> Result<(), DecodeError> {
    let side = usize::try_from(side).map_err(|_| invalid(frame_index, "prediction.size"))?;
    for row in 0..side {
        for column in 0..side {
            plane
                .set(
                    x + u32::try_from(column).map_err(|_| invalid(frame_index, "prediction.x"))?,
                    y + u32::try_from(row).map_err(|_| invalid(frame_index, "prediction.y"))?,
                    prediction[row * side + column],
                )
                .map_err(|_| invalid(frame_index, "prediction.write"))?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn add_residual(
    plane: &mut Plane,
    prediction: &[u8],
    prediction_side: u32,
    destination_x: u32,
    destination_y: u32,
    source_x: u32,
    source_y: u32,
    transform_side: u32,
    residual: &[i32],
) -> Result<(), DecodeError> {
    let prediction_side = usize::try_from(prediction_side).unwrap();
    let transform_side_usize = usize::try_from(transform_side).unwrap();
    for row in 0..transform_side_usize {
        for column in 0..transform_side_usize {
            let source = (usize::try_from(source_y).unwrap() + row) * prediction_side
                + usize::try_from(source_x).unwrap()
                + column;
            let value = i32::from(prediction[source])
                .saturating_add(residual[row * transform_side_usize + column])
                .clamp(0, 255);
            plane
                .set(
                    destination_x + u32::try_from(column).unwrap(),
                    destination_y + u32::try_from(row).unwrap(),
                    u8::try_from(value).expect("invariant: reconstructed sample is clipped"),
                )
                .map_err(|_| invalid(0, "reconstruction.write"))?;
        }
    }
    Ok(())
}

fn block_positions(tree: &PartitionTree, x: u32, y: u32) -> Vec<(u32, u32, BlockSize)> {
    match tree {
        PartitionTree::Leaf(size) => vec![(x, y, *size)],
        PartitionTree::Split { size, children } => {
            let half = u32::from(size.side() / 2);
            let offsets = [(0, 0), (half, 0), (0, half), (half, half)];
            children
                .iter()
                .zip(offsets)
                .flat_map(|(child, (dx, dy))| block_positions(child, x + dx, y + dy))
                .collect()
        }
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

fn transform_size(side: u32, frame_index: u32) -> Result<TransformSize, DecodeError> {
    TransformSize::try_from(u8::try_from(side).unwrap())
        .map_err(|_| invalid(frame_index, "transform.size"))
}

fn syntax_transform_size(side: u32, frame_index: u32) -> Result<TransformBlockSize, DecodeError> {
    match side {
        4 => Ok(TransformBlockSize::N4),
        8 => Ok(TransformBlockSize::N8),
        16 => Ok(TransformBlockSize::N16),
        32 => Ok(TransformBlockSize::N32),
        _ => Err(invalid(frame_index, "syntax.transform_size")),
    }
}

const fn reference_slot(reference: kf_bitstream::ReferenceFrame) -> ReferenceSlot {
    match reference {
        kf_bitstream::ReferenceFrame::Last => ReferenceSlot::Last,
        kf_bitstream::ReferenceFrame::Golden => ReferenceSlot::Golden,
    }
}

const fn invalid(frame_index: u32, element: &'static str) -> DecodeError {
    DecodeError::InvalidFrame {
        frame_index,
        element,
    }
}

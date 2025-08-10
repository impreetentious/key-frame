use kf_frame::{Frame, Plane};

use crate::{
    ReferenceError,
    crc::crc32c,
    deblock::{RefCodedBlock, loop_filter_frame},
    motion::{RefMotionField, RefMotionVector, RefReference, clamp_motion, predict_inter},
    predict::predict,
    range::ReferenceRange,
    reader::ByteReader,
    syntax::{RefBlock, RefPrediction, read_coefficients, read_partition, read_prediction},
    transform::inverse_levels,
};

#[derive(Clone, Copy)]
struct RefFrames<'a> {
    last: &'a Frame,
    golden: &'a Frame,
}

impl<'a> RefFrames<'a> {
    const fn get(self, reference: RefReference) -> &'a Frame {
        match reference {
            RefReference::Last => self.last,
            RefReference::Golden => self.golden,
        }
    }
}

struct RefPacket<'a> {
    payload: &'a [u8],
    frame_index: u32,
    qp: u8,
    key: bool,
    golden_refresh: bool,
    consumed: usize,
}

/// Independent decode state. Frames and contexts commit only after success.
#[derive(Clone, Debug, Default)]
pub struct ReferenceDecoder {
    last: Option<Frame>,
    golden: Option<Frame>,
    contexts: Option<[u16; 144]>,
    last_frame_index: Option<u32>,
}

impl ReferenceDecoder {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last: None,
            golden: None,
            contexts: None,
            last_frame_index: None,
        }
    }

    /// Independently decodes every key and P frame in a complete stream.
    pub fn decode_stream(&mut self, bytes: &[u8]) -> Result<Vec<Frame>, ReferenceError> {
        Ok(self.decode_stream_traced(bytes)?.0)
    }

    /// Decodes a stream and records the context bank after every superblock.
    pub fn decode_stream_traced(
        &mut self,
        bytes: &[u8],
    ) -> Result<(Vec<Frame>, Vec<[u16; 144]>), ReferenceError> {
        self.invalidate();
        let result = self.decode_all(bytes);
        if result.is_err() {
            self.invalidate();
        }
        result
    }

    /// Compatibility entry point returning the first pre-inter staging frame.
    pub fn decode_intra_stream(&mut self, bytes: &[u8]) -> Result<Frame, ReferenceError> {
        self.decode_stream(bytes)?
            .into_iter()
            .next()
            .ok_or_else(|| ReferenceError::new(0, "initial_keyframe"))
    }

    #[must_use]
    pub const fn has_references(&self) -> bool {
        self.last.is_some() && self.golden.is_some()
    }

    /// Committed context bank, or `None` after invalidation.
    #[must_use]
    pub const fn committed_p1(&self) -> Option<[u16; 144]> {
        self.contexts
    }

    fn decode_all(
        &mut self,
        bytes: &[u8],
    ) -> Result<(Vec<Frame>, Vec<[u16; 144]>), ReferenceError> {
        let (width, height, mut packet_offset) = read_sequence(bytes)?;
        let mut frames = Vec::new();
        let mut checkpoints = Vec::new();
        while packet_offset < bytes.len() {
            let packet = read_packet(&bytes[packet_offset..])?;
            let expected_index = self
                .last_frame_index
                .map_or(0, |index| index.saturating_add(1));
            if packet.frame_index != expected_index {
                return Err(ReferenceError::new(
                    u32::try_from(packet_offset + 8).unwrap_or(u32::MAX),
                    "packet.frame_index_gap",
                ));
            }
            let references = match (&self.last, &self.golden) {
                (Some(last), Some(golden)) => Some(RefFrames { last, golden }),
                _ => None,
            };
            if !packet.key && references.is_none() {
                return Err(ReferenceError::new(
                    u32::try_from(packet_offset + 12).unwrap_or(u32::MAX),
                    "pframe.references",
                ));
            }
            let mut range = if packet.key {
                ReferenceRange::new(packet.payload)?
            } else {
                ReferenceRange::with_contexts(
                    packet.payload,
                    self.contexts
                        .ok_or_else(|| ReferenceError::new(0, "pframe.contexts"))?,
                )?
            };
            let padded_width = u32::from(width).div_ceil(64) * 64;
            let padded_height = u32::from(height).div_ceil(64) * 64;
            let mut frame = Frame::filled_420(padded_width, padded_height, 0)
                .map_err(|_| ReferenceError::new(0, "frame.allocate"))?;
            let mut motion_field = RefMotionField::new(padded_width, padded_height)?;
            let mut coded_blocks = Vec::new();
            for superblock_y in (0..padded_height).step_by(64) {
                for superblock_x in (0..padded_width).step_by(64) {
                    let blocks = read_partition(&mut range, superblock_x, superblock_y)?;
                    for block in blocks {
                        let prediction = read_prediction(&mut range, packet.key)?;
                        coded_blocks.push(reconstruct_block(
                            &mut range,
                            &mut frame,
                            &mut motion_field,
                            references,
                            block,
                            prediction,
                            packet.qp,
                        )?);
                    }
                    checkpoints.push(range.p1_values());
                }
            }
            loop_filter_frame(&mut frame, packet.qp, &coded_blocks)?;
            let final_contexts = range.contexts();
            let visible = frame
                .crop_420(u32::from(width), u32::from(height))
                .map_err(|_| ReferenceError::new(0, "frame.crop"))?;

            self.last = Some(frame.clone());
            if packet.key || packet.golden_refresh {
                self.golden = Some(frame);
            }
            self.contexts = Some(final_contexts);
            self.last_frame_index = Some(packet.frame_index);
            frames.push(visible);
            packet_offset = packet_offset
                .checked_add(packet.consumed)
                .ok_or_else(|| ReferenceError::new(u32::MAX, "packet.offset"))?;
        }
        if frames.is_empty() {
            return Err(ReferenceError::new(0, "initial_keyframe"));
        }
        Ok((frames, checkpoints))
    }

    fn invalidate(&mut self) {
        self.last = None;
        self.golden = None;
        self.contexts = None;
        self.last_frame_index = None;
    }
}

fn reconstruct_block(
    range: &mut ReferenceRange<'_>,
    frame: &mut Frame,
    motion_field: &mut RefMotionField,
    references: Option<RefFrames<'_>>,
    block: RefBlock,
    prediction: RefPrediction,
    qp: u8,
) -> Result<RefCodedBlock, ReferenceError> {
    let mut coded = false;
    match prediction {
        RefPrediction::Intra(mode) => {
            let luma_prediction = predict(&frame.y, block.x, block.y, block.size, mode)?;
            coded |= reconstruct_residual(
                range,
                &mut frame.y,
                block.x,
                block.y,
                block.size,
                &luma_prediction,
                qp,
                false,
            )?;
            for plane in [&mut frame.cb, &mut frame.cr] {
                let chroma_prediction =
                    predict(plane, block.x / 2, block.y / 2, block.size / 2, mode)?;
                coded |= reconstruct_residual(
                    range,
                    plane,
                    block.x / 2,
                    block.y / 2,
                    block.size / 2,
                    &chroma_prediction,
                    qp,
                    true,
                )?;
            }
            motion_field.record_intra(block.x, block.y, block.size);
            Ok(RefCodedBlock {
                x: block.x,
                y: block.y,
                size: block.size,
                intra: true,
                coded,
            })
        }
        RefPrediction::Skip(reference) | RefPrediction::Inter { reference, .. } => {
            let references =
                references.ok_or_else(|| ReferenceError::new(0, "reference.missing"))?;
            let predictor = motion_field.predictor(block.x, block.y, block.size, reference);
            let requested = match prediction {
                RefPrediction::Skip(_) => predictor,
                RefPrediction::Inter { mvd, .. } => RefMotionVector {
                    x_q4: predictor.x_q4.saturating_add(mvd.x_q4),
                    y_q4: predictor.y_q4.saturating_add(mvd.y_q4),
                },
                RefPrediction::Intra(_) => unreachable!("matched reference prediction"),
            };
            let motion_vector = clamp_motion(
                &references.get(reference).y,
                block.x,
                block.y,
                block.size,
                requested,
                false,
            );
            let skip = matches!(prediction, RefPrediction::Skip(_));
            let luma_prediction = predict_inter(
                &references.get(reference).y,
                block.x,
                block.y,
                block.size,
                motion_vector,
                false,
            );
            coded |= reconstruct_inter_or_skip(
                range,
                &mut frame.y,
                block.x,
                block.y,
                block.size,
                &luma_prediction,
                qp,
                false,
                skip,
            )?;
            for (plane, reference_plane) in [
                (&mut frame.cb, &references.get(reference).cb),
                (&mut frame.cr, &references.get(reference).cr),
            ] {
                let chroma_prediction = predict_inter(
                    reference_plane,
                    block.x / 2,
                    block.y / 2,
                    block.size / 2,
                    motion_vector,
                    true,
                );
                coded |= reconstruct_inter_or_skip(
                    range,
                    plane,
                    block.x / 2,
                    block.y / 2,
                    block.size / 2,
                    &chroma_prediction,
                    qp,
                    true,
                    skip,
                )?;
            }
            motion_field.record_inter(block.x, block.y, block.size, reference, motion_vector);
            Ok(RefCodedBlock {
                x: block.x,
                y: block.y,
                size: block.size,
                intra: false,
                coded,
            })
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn reconstruct_inter_or_skip(
    range: &mut ReferenceRange<'_>,
    plane: &mut Plane,
    x: u32,
    y: u32,
    side: u32,
    prediction: &[u8],
    qp: u8,
    chroma: bool,
    skip: bool,
) -> Result<bool, ReferenceError> {
    if skip {
        install_prediction(plane, x, y, side, prediction)?;
        return Ok(false);
    }
    reconstruct_residual(range, plane, x, y, side, prediction, qp, chroma)
}

#[allow(clippy::too_many_arguments)]
fn reconstruct_residual(
    range: &mut ReferenceRange<'_>,
    plane: &mut Plane,
    x: u32,
    y: u32,
    prediction_side: u32,
    prediction: &[u8],
    qp: u8,
    chroma: bool,
) -> Result<bool, ReferenceError> {
    let transform_side = prediction_side.min(32);
    let blocks_per_axis = prediction_side / transform_side;
    let transform_side_usize = usize::try_from(transform_side).unwrap();
    let prediction_side_usize = usize::try_from(prediction_side).unwrap();
    let mut coded = false;
    for transform_y in 0..blocks_per_axis {
        for transform_x in 0..blocks_per_axis {
            let levels = read_coefficients(range, chroma, transform_side)?;
            coded |= levels.iter().any(|&level| level != 0);
            let residual = inverse_levels(&levels, qp, transform_side_usize)?;
            for row in 0..transform_side_usize {
                for column in 0..transform_side_usize {
                    let prediction_index =
                        (usize::try_from(transform_y).unwrap() * transform_side_usize + row)
                            * prediction_side_usize
                            + usize::try_from(transform_x).unwrap() * transform_side_usize
                            + column;
                    let value = (i32::from(prediction[prediction_index])
                        + residual[row * transform_side_usize + column])
                        .clamp(0, 255);
                    plane
                        .set(
                            x + transform_x * transform_side + u32::try_from(column).unwrap(),
                            y + transform_y * transform_side + u32::try_from(row).unwrap(),
                            u8::try_from(value).unwrap(),
                        )
                        .map_err(|_| ReferenceError::new(0, "reconstruction.write"))?;
                }
            }
        }
    }
    Ok(coded)
}

fn install_prediction(
    plane: &mut Plane,
    x: u32,
    y: u32,
    side: u32,
    prediction: &[u8],
) -> Result<(), ReferenceError> {
    let side = usize::try_from(side).map_err(|_| ReferenceError::new(0, "prediction.size"))?;
    for row in 0..side {
        for column in 0..side {
            plane
                .set(
                    x + u32::try_from(column).unwrap(),
                    y + u32::try_from(row).unwrap(),
                    prediction[row * side + column],
                )
                .map_err(|_| ReferenceError::new(0, "prediction.write"))?;
        }
    }
    Ok(())
}

fn read_sequence(bytes: &[u8]) -> Result<(u16, u16, usize), ReferenceError> {
    let mut reader = ByteReader::new(bytes);
    if reader.bytes(4, "sequence.magic")? != b"KFV1" {
        return Err(ReferenceError::new(0, "sequence.magic"));
    }
    if reader.u16("sequence.version")? != 1 {
        return Err(ReferenceError::new(4, "sequence.version"));
    }
    let width = reader.u16("sequence.width")?;
    let height = reader.u16("sequence.height")?;
    if !(64..=4096).contains(&width)
        || !width.is_multiple_of(2)
        || !(64..=2304).contains(&height)
        || !height.is_multiple_of(2)
    {
        return Err(ReferenceError::new(6, "sequence.dimensions"));
    }
    if reader.u8("sequence.chroma")? != 1 || reader.u8("sequence.depth")? != 8 {
        return Err(ReferenceError::new(10, "sequence.format"));
    }
    for element in ["fps_num", "fps_den", "kf_interval"] {
        if reader.u16(element)? == 0 {
            return Err(ReferenceError::new(12, element));
        }
    }
    if reader.u8("golden_interval")? == 0 || reader.u8("sequence.flags")? != 0 {
        return Err(ReferenceError::new(18, "sequence.policy"));
    }
    let expected_crc = reader.u32("sequence.crc")?;
    if crc32c(&bytes[..20]) != expected_crc {
        return Err(ReferenceError::new(20, "sequence.crc"));
    }
    Ok((width, height, reader.offset()))
}

fn read_packet(bytes: &[u8]) -> Result<RefPacket<'_>, ReferenceError> {
    let mut reader = ByteReader::new(bytes);
    if reader.bytes(4, "packet.sync")? != b"KFP1" {
        return Err(ReferenceError::new(0, "packet.sync"));
    }
    let payload_len = reader.u32("packet.payload_len")?;
    if !(5..=16 * 1024 * 1024).contains(&payload_len) {
        return Err(ReferenceError::new(4, "packet.payload_len"));
    }
    let frame_index = reader.u32("packet.frame_index")?;
    let flag_bits = reader.u8("packet.flags")?;
    let key = flag_bits & 1 != 0;
    let golden_refresh = flag_bits & 2 != 0;
    let show = flag_bits & 4 != 0;
    if flag_bits & 0xf8 != 0 || !show || (key && !golden_refresh) {
        return Err(ReferenceError::new(12, "packet.flags"));
    }
    let qp = reader.u8("packet.qp")?;
    if qp > 63 || reader.u16("packet.reserved")? != 0 {
        return Err(ReferenceError::new(13, "packet.qp_or_reserved"));
    }
    let header_crc = reader.u32("packet.header_crc")?;
    let payload_crc = reader.u32("packet.payload_crc")?;
    if crc32c(&bytes[4..16]) != header_crc {
        return Err(ReferenceError::new(16, "packet.header_crc"));
    }
    let payload = reader.bytes(
        usize::try_from(payload_len).map_err(|_| ReferenceError::new(4, "packet.payload_len"))?,
        "packet.payload",
    )?;
    if crc32c(payload) != payload_crc {
        return Err(ReferenceError::new(20, "packet.payload_crc"));
    }
    Ok(RefPacket {
        payload,
        frame_index,
        qp,
        key,
        golden_refresh,
        consumed: reader.offset(),
    })
}

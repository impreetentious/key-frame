use kf_frame::{Frame, Plane};

use crate::{
    ReferenceError,
    coverage::ReferenceCoverage,
    crc::crc32c,
    deblock::{RefCodedBlock, loop_filter_frame},
    motion::{RefMotionField, RefMotionVector, RefReference, clamp_motion, predict_inter},
    predict::predict,
    range::ReferenceRange,
    reader::ByteReader,
    scan::{PacketCursor, RefPacket, RefScanEvent},
    status::{RefFrameStatus, RefRecovery, RefStreamReport},
    syntax::{RefBlock, RefPrediction, read_coefficients, read_partition, read_prediction},
    transform::inverse_levels,
};

/// One frame recovered by random access, in this decoder's own vocabulary.
#[derive(Clone, Debug)]
pub struct RefSeek {
    /// The requested frame, cropped to the display dimensions.
    pub frame: Frame,
    /// The keyframe this decoder restarted from.
    pub keyframe_index: u32,
    /// How many frames it decoded to get there, including that keyframe.
    pub frames_decoded: usize,
}

/// A decode plus everything it was instrumented for.
struct RefInstrumented {
    frames: Vec<Frame>,
    checkpoints: Vec<[u16; 144]>,
    coverage: ReferenceCoverage,
}

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
        let decoded = self.decode_stream_instrumented(bytes)?;
        Ok((decoded.frames, decoded.checkpoints))
    }

    /// Decodes a stream and independently tallies what it coded.
    pub fn decode_stream_coverage(
        &mut self,
        bytes: &[u8],
    ) -> Result<(Vec<Frame>, ReferenceCoverage), ReferenceError> {
        let decoded = self.decode_stream_instrumented(bytes)?;
        Ok((decoded.frames, decoded.coverage))
    }

    fn decode_stream_instrumented(
        &mut self,
        bytes: &[u8],
    ) -> Result<RefInstrumented, ReferenceError> {
        self.invalidate();
        let result = self.decode_all(bytes);
        if result.is_err() {
            self.invalidate();
        }
        result
    }

    /// Compatibility entry point returning the first decoded frame.
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

    fn decode_all(&mut self, bytes: &[u8]) -> Result<RefInstrumented, ReferenceError> {
        let (width, height, packet_start) = read_sequence(bytes)?;
        let mut frames = Vec::new();
        let mut checkpoints = Vec::new();
        let mut coverage = ReferenceCoverage::new();
        let mut cursor = PacketCursor::new(&bytes[packet_start..]);
        while let Some(packet) = cursor.next_packet()? {
            let expected_index = self
                .last_frame_index
                .map_or(0, |index| index.saturating_add(1));
            if packet.frame_index != expected_index {
                return Err(ReferenceError::new(
                    packet.frame_index,
                    "packet.frame_index_gap",
                ));
            }
            if !packet.key && !self.has_references() {
                return Err(ReferenceError::new(packet.frame_index, "pframe.references"));
            }
            let decoded = self.decode_packet(width, height, &packet)?;
            checkpoints.extend(decoded.checkpoints);
            coverage.merge(&decoded.coverage);
            frames.push(self.install(&packet, decoded.padded, decoded.contexts, width, height)?);
        }
        if frames.is_empty() {
            return Err(ReferenceError::new(0, "initial_keyframe"));
        }
        Ok(RefInstrumented {
            frames,
            checkpoints,
            coverage,
        })
    }

    /// Decodes one validated packet without touching committed state, so a
    /// failure leaves the decoder exactly as it was.
    fn decode_packet(
        &self,
        width: u16,
        height: u16,
        packet: &RefPacket<'_>,
    ) -> Result<DecodedPacket, ReferenceError> {
        let references = match (&self.last, &self.golden) {
            (Some(last), Some(golden)) => Some(RefFrames { last, golden }),
            _ => None,
        };
        let mut range = if packet.key {
            ReferenceRange::new(packet.payload)?
        } else {
            ReferenceRange::with_contexts(
                packet.payload,
                self.contexts
                    .ok_or_else(|| ReferenceError::new(0, "pframe.contexts"))?,
            )?
        };
        // The superblock side is declared, not sixty-four written four times.
        let superblock = crate::limits().superblock_size;
        let padded_width = u32::from(width).div_ceil(superblock) * superblock;
        let padded_height = u32::from(height).div_ceil(superblock) * superblock;
        let mut frame = Frame::filled_420(padded_width, padded_height, 0)
            .map_err(|_| ReferenceError::new(0, "frame.allocate"))?;
        let mut motion_field = RefMotionField::new(padded_width, padded_height)?;
        let mut coded_blocks = Vec::new();
        let mut checkpoints = Vec::new();
        let stride = usize::try_from(superblock).expect("a superblock side fits an index");
        for superblock_y in (0..padded_height).step_by(stride) {
            for superblock_x in (0..padded_width).step_by(stride) {
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
        let coverage = range.coverage().clone();
        Ok(DecodedPacket {
            contexts: range.contexts(),
            padded: frame,
            checkpoints,
            coverage,
        })
    }

    /// Commits a fully decoded packet and returns its visible image.
    fn install(
        &mut self,
        packet: &RefPacket<'_>,
        padded: Frame,
        contexts: [u16; 144],
        width: u16,
        height: u16,
    ) -> Result<Frame, ReferenceError> {
        let visible = padded
            .crop_420(u32::from(width), u32::from(height))
            .map_err(|_| ReferenceError::new(0, "frame.crop"))?;
        self.last = Some(padded.clone());
        if packet.key || packet.golden_refresh {
            self.golden = Some(padded);
        }
        self.contexts = Some(contexts);
        self.last_frame_index = Some(packet.frame_index);
        Ok(visible)
    }

    /// Independently recovers one frame by random access.
    ///
    /// Written against the rule rather than against the fast decoder: a P frame
    /// means nothing without its references and its carried context bank, and a
    /// keyframe needs neither, so the only legal place to begin part-way into a
    /// stream is the last keyframe at or before the requested frame. Everything
    /// from there to the target is decoded in order; everything before it is
    /// scanned and discarded.
    ///
    /// The two passes are deliberate. The first proves the packet sequence is
    /// intact and finds the entry point; the second decodes. A stream a linear
    /// decode would reject must not become seekable by looking at less of it.
    pub fn seek_frame(&mut self, bytes: &[u8], target: u32) -> Result<RefSeek, ReferenceError> {
        self.invalidate();
        let result = self.seek_run(bytes, target);
        if result.is_err() {
            // Nothing half-decoded may survive a failed seek, or the next
            // decode could predict from a frame nobody asked for.
            self.invalidate();
        }
        result
    }

    fn seek_run(&mut self, bytes: &[u8], target: u32) -> Result<RefSeek, ReferenceError> {
        let (width, height, packet_start) = read_sequence(bytes)?;

        let mut keys = Vec::new();
        let mut cursor = PacketCursor::new(&bytes[packet_start..]);
        let mut expected = 0_u32;
        while let Some(packet) = cursor.next_packet()? {
            if packet.frame_index != expected {
                return Err(ReferenceError::new(
                    packet.frame_index,
                    "packet.frame_index_gap",
                ));
            }
            if packet.key {
                keys.push(packet.frame_index);
            }
            expected = expected.saturating_add(1);
        }
        if expected == 0 {
            return Err(ReferenceError::new(0, "initial_keyframe"));
        }
        if target >= expected {
            return Err(ReferenceError::new(target, "seek.target"));
        }
        let entry = keys
            .into_iter()
            .rfind(|index| *index <= target)
            .ok_or_else(|| ReferenceError::new(target, "seek.entry_missing"))?;

        let mut cursor = PacketCursor::new(&bytes[packet_start..]);
        let mut frame = None;
        let mut decoded_count = 0_usize;
        while let Some(packet) = cursor.next_packet()? {
            if packet.frame_index < entry {
                continue;
            }
            if packet.frame_index == entry && !packet.key {
                return Err(ReferenceError::new(
                    packet.frame_index,
                    "seek.entry_not_key",
                ));
            }
            if !packet.key && !self.has_references() {
                return Err(ReferenceError::new(packet.frame_index, "pframe.references"));
            }
            let decoded = self.decode_packet(width, height, &packet)?;
            let visible = self.install(&packet, decoded.padded, decoded.contexts, width, height)?;
            decoded_count += 1;
            if packet.frame_index == target {
                frame = Some(visible);
                break;
            }
        }
        let frame = frame.ok_or_else(|| ReferenceError::new(target, "seek.target"))?;
        Ok(RefSeek {
            frame,
            keyframe_index: entry,
            frames_decoded: decoded_count,
        })
    }

    /// Independently decodes a stream that may be damaged, classifying every
    /// structurally accepted packet rather than stopping at the first fault.
    ///
    /// Written against the normative rules directly, not against the fast
    /// decoder: an index gap invalidates before the packet is classified, a
    /// keyframe may always be attempted because it needs neither old references
    /// nor carried contexts, and nothing is committed until a packet decodes
    /// completely.
    pub fn decode_stream_resilient(
        &mut self,
        bytes: &[u8],
    ) -> Result<RefStreamReport, ReferenceError> {
        self.invalidate();
        let (width, height, packet_start) = read_sequence(bytes)?;
        let mut cursor = PacketCursor::new(&bytes[packet_start..]);
        let mut report = RefStreamReport::default();
        let mut needs_keyframe = true;
        let mut next_expected_index = 0u32;
        loop {
            let event = cursor.next_event();
            let (index, key, packet) = match &event {
                RefScanEvent::Packet(packet) => (packet.frame_index, packet.key, Some(packet)),
                RefScanEvent::PayloadCorrupt(facts) | RefScanEvent::Truncated(facts) => {
                    (facts.frame_index, facts.key, None)
                }
                RefScanEvent::End => break,
            };

            let gap = index != next_expected_index;
            let recovery = if gap {
                self.invalidate();
                needs_keyframe = true;
                Some(if next_expected_index == 0 {
                    RefRecovery::LeadingLoss
                } else {
                    RefRecovery::Gap
                })
            } else {
                None
            };
            next_expected_index = index.saturating_add(1);

            let Some(packet) = packet else {
                self.invalidate();
                needs_keyframe = true;
                report.statuses.push(RefFrameStatus::Corrupt);
                continue;
            };
            if !key && (needs_keyframe || !self.has_references()) {
                self.invalidate();
                needs_keyframe = true;
                report.statuses.push(RefFrameStatus::DependencyLost);
                continue;
            }
            let Ok(decoded) = self.decode_packet(width, height, packet) else {
                self.invalidate();
                needs_keyframe = true;
                report.statuses.push(RefFrameStatus::Corrupt);
                continue;
            };
            let Ok(visible) = self.install(packet, decoded.padded, decoded.contexts, width, height)
            else {
                self.invalidate();
                needs_keyframe = true;
                report.statuses.push(RefFrameStatus::Corrupt);
                continue;
            };
            needs_keyframe = false;
            report.frames.push(visible);
            report.statuses.push(match recovery {
                Some(recovery) if key => RefFrameStatus::RecoveredKeyframe(recovery),
                _ => RefFrameStatus::Shown,
            });
        }
        Ok(report)
    }

    fn invalidate(&mut self) {
        self.last = None;
        self.golden = None;
        self.contexts = None;
        self.last_frame_index = None;
    }
}

/// One packet decoded but not yet committed.
struct DecodedPacket {
    padded: Frame,
    contexts: [u16; 144],
    checkpoints: Vec<[u16; 144]>,
    coverage: ReferenceCoverage,
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
    let bounds = crate::limits();
    // The declared bounds are widths, and a header field is a `u16`. Comparing
    // in the wider type keeps the declaration free to name a bound this field
    // could not hold, which would then be caught here rather than truncated
    // into range.
    let (wide_width, wide_height) = (u32::from(width), u32::from(height));
    if !(bounds.min_width..=bounds.max_width).contains(&wide_width)
        || !width.is_multiple_of(2)
        || !(bounds.min_height..=bounds.max_height).contains(&wide_height)
        || !height.is_multiple_of(2)
    {
        return Err(ReferenceError::new(6, "sequence.dimensions"));
    }
    if reader.u8("sequence.chroma")? != bounds.chroma_code
        || reader.u8("sequence.depth")? != bounds.bit_depth
    {
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

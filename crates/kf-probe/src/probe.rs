use kf_bitstream::{BitstreamError, PacketScanner};
use kf_bitstream::{
    BlockSize, FrameType, PartitionTree, PlaneClass, Prediction, ReferenceFrame,
    SEQUENCE_HEADER_SIZE, SequenceHeader, SyntaxReader, SyntaxWriter, TransformBlockSize,
};
use kf_range::ContextBank;

/// Canonical replay accounting for one coding block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockProbe {
    pub x: u16,
    pub y: u16,
    pub size: u8,
    pub mode: &'static str,
    pub reference: Option<&'static str>,
    pub motion_vector_q4: Option<[i16; 2]>,
    pub qp: u8,
    pub modeled_entropy_q16: u64,
    pub emitted_payload_bytes: u64,
    pub dc_energy: u64,
}

/// Canonical replay accounting for one superblock and its coding blocks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuperblockProbe {
    pub x: u16,
    pub y: u16,
    pub structure_modeled_entropy_q16: u64,
    pub structure_emitted_payload_bytes: u64,
    pub blocks: Vec<BlockProbe>,
}

/// Versioned probe result for one currently supported frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbeReport {
    pub width: u16,
    pub height: u16,
    pub frame_index: u32,
    pub key: bool,
    pub golden_refresh: bool,
    pub show: bool,
    pub frame_qp: u8,
    pub input_payload_len: usize,
    pub canonical_replay_payload_len: usize,
    pub canonical_payload_match: bool,
    pub first_mismatch_offset: Option<usize>,
    pub frame_flush_bytes: u64,
    pub superblocks: Vec<SuperblockProbe>,
}

impl ProbeReport {
    /// Emits the frozen probe-version-one JSON shape without a runtime schema dependency.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mismatch = self
            .first_mismatch_offset
            .map_or_else(|| "null".to_owned(), |value| value.to_string());
        let superblocks = self
            .superblocks
            .iter()
            .map(|superblock| {
                let blocks = superblock
                    .blocks
                    .iter()
                    .map(block_json)
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"pos\":[{},{}],\"size\":64,\"structure_modeled_entropy_q16\":{},\"structure_emitted_payload_bytes\":{},\"cbs\":[{}]}}",
                    superblock.x,
                    superblock.y,
                    superblock.structure_modeled_entropy_q16,
                    superblock.structure_emitted_payload_bytes,
                    blocks,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"probe_version\":1,\"stream\":{{\"bitstream_version\":1,\"width\":{},\"height\":{},\"chroma\":\"420jpeg\",\"depth\":8}},\"frame\":{{\"frame_index\":{},\"flags\":{{\"key\":{},\"golden_refresh\":{},\"show\":{}}},\"qp\":{},\"input_payload_len\":{},\"canonical_replay_payload_len\":{},\"canonical_payload_match\":{},\"first_mismatch_offset\":{},\"frame_flush_bytes\":{},\"superblocks\":[{}]}}}}",
            self.width,
            self.height,
            self.frame_index,
            self.key,
            self.golden_refresh,
            self.show,
            self.frame_qp,
            self.input_payload_len,
            self.canonical_replay_payload_len,
            self.canonical_payload_match,
            mismatch,
            self.frame_flush_bytes,
            superblocks
        )
    }
}

fn block_json(block: &BlockProbe) -> String {
    let prediction = match (block.reference, block.motion_vector_q4) {
        (None, None) => format!("{{\"kind\":\"intra\",\"mode\":\"{}\"}}", block.mode),
        (Some(reference), None) => {
            format!("{{\"kind\":\"skip\",\"reference\":\"{reference}\"}}")
        }
        (Some(reference), Some([x_q4, y_q4])) => format!(
            "{{\"kind\":\"inter\",\"reference\":\"{reference}\",\"mv_q4\":[{x_q4},{y_q4}]}}"
        ),
        (None, Some(_)) => unreachable!("invariant: motion vectors require a reference"),
    };
    format!(
        "{{\"pos\":[{},{}],\"size\":{},\"prediction\":{},\"qp\":{},\"modeled_entropy_q16\":{},\"emitted_payload_bytes\":{},\"dc_energy\":{}}}",
        block.x,
        block.y,
        block.size,
        prediction,
        block.qp,
        block.modeled_entropy_q16,
        block.emitted_payload_bytes,
        block.dc_energy
    )
}

/// Parses and canonical-shadow-replays the first keyframe in a stream.
pub fn probe_stream(bytes: &[u8]) -> Result<ProbeReport, BitstreamError> {
    let sequence = SequenceHeader::decode(bytes)?;
    let packet_region = bytes
        .get(SEQUENCE_HEADER_SIZE..)
        .ok_or_else(|| invalid("probe.packet_region"))?;
    let mut scanner = PacketScanner::new(packet_region);
    let packet = scanner
        .next_packet()?
        .ok_or_else(|| invalid("probe.packet"))?;
    if !packet.flags.key {
        return Err(invalid("probe.initial_nonkey"));
    }

    let mut reader = SyntaxReader::new(&packet.payload, ContextBank::initial())?;
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    let padded_width = sequence.width.div_ceil(64) * 64;
    let padded_height = sequence.height.div_ceil(64) * 64;
    let mut superblocks = Vec::new();
    for superblock_y in (0..padded_height).step_by(64) {
        for superblock_x in (0..padded_width).step_by(64) {
            let structure_before = writer.stats().clone();
            let partition = reader.read_partition()?;
            writer.write_partition(&partition)?;
            let structure_after = writer.stats().clone();
            let positions = block_positions(&partition, superblock_x, superblock_y);
            let mut blocks = Vec::with_capacity(positions.len());

            for (x, y, size) in positions {
                let before = writer.stats().clone();
                let prediction = reader.read_prediction(FrameType::Key)?;
                writer.write_prediction(FrameType::Key, prediction)?;
                let mut dc_energy = 0_u64;
                for (plane, transform, count) in transform_schedule(size) {
                    for _ in 0..count {
                        let levels = reader.read_coefficients(plane, transform)?;
                        dc_energy = dc_energy.saturating_add(
                            levels
                                .first()
                                .map_or(0, |level| u64::from(level.unsigned_abs())),
                        );
                        writer.write_coefficients(plane, transform, &levels)?;
                    }
                }
                let after = writer.stats().clone();
                let (mode, reference, motion_vector_q4) = prediction_details(prediction);
                blocks.push(BlockProbe {
                    x,
                    y,
                    size: size.side(),
                    mode,
                    reference,
                    motion_vector_q4,
                    qp: packet.frame_qp,
                    modeled_entropy_q16: after
                        .modeled_entropy_q16
                        .saturating_sub(before.modeled_entropy_q16),
                    emitted_payload_bytes: after.emitted_bytes.saturating_sub(before.emitted_bytes),
                    dc_energy,
                });
            }
            superblocks.push(SuperblockProbe {
                x: superblock_x,
                y: superblock_y,
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

    let (replay, replay_contexts) = writer.finish();
    if reader.contexts() != &replay_contexts {
        return Err(invalid("probe.context_lockstep"));
    }
    let first_mismatch_offset = packet
        .payload
        .iter()
        .zip(&replay.bytes)
        .position(|(input, canonical)| input != canonical)
        .or_else(|| {
            (packet.payload.len() != replay.bytes.len())
                .then_some(packet.payload.len().min(replay.bytes.len()))
        });
    let frame_flush_bytes = replay
        .stats
        .emission_events
        .iter()
        .filter(|event| event.finalization)
        .map(|event| u64::from(event.bytes))
        .sum();
    Ok(ProbeReport {
        width: sequence.width,
        height: sequence.height,
        frame_index: packet.frame_index,
        key: packet.flags.key,
        golden_refresh: packet.flags.golden_refresh,
        show: packet.flags.show,
        frame_qp: packet.frame_qp,
        input_payload_len: packet.payload.len(),
        canonical_replay_payload_len: replay.bytes.len(),
        canonical_payload_match: first_mismatch_offset.is_none(),
        first_mismatch_offset,
        frame_flush_bytes,
        superblocks,
    })
}

fn block_positions(tree: &PartitionTree, x: u16, y: u16) -> Vec<(u16, u16, BlockSize)> {
    match tree {
        PartitionTree::Leaf(size) => vec![(x, y, *size)],
        PartitionTree::Split { size, children } => {
            let half = u16::from(size.side() / 2);
            let offsets = [(0, 0), (half, 0), (0, half), (half, half)];
            children
                .iter()
                .zip(offsets)
                .flat_map(|(child, (dx, dy))| block_positions(child, x + dx, y + dy))
                .collect()
        }
    }
}

fn transform_schedule(size: BlockSize) -> [(PlaneClass, TransformBlockSize, usize); 3] {
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

fn prediction_details(
    prediction: Prediction,
) -> (&'static str, Option<&'static str>, Option<[i16; 2]>) {
    match prediction {
        Prediction::Intra(mode) => (
            match mode {
                kf_bitstream::IntraMode::Dc => "dc",
                kf_bitstream::IntraMode::Planar => "planar",
                kf_bitstream::IntraMode::Horizontal => "horizontal",
                kf_bitstream::IntraMode::Vertical => "vertical",
                kf_bitstream::IntraMode::D45 => "d45",
                kf_bitstream::IntraMode::D135 => "d135",
                kf_bitstream::IntraMode::D117 => "d117",
                kf_bitstream::IntraMode::D153 => "d153",
            },
            None,
            None,
        ),
        Prediction::Skip { reference } => ("skip", Some(reference_name(reference)), None),
        Prediction::Inter { reference, mvd } => (
            "inter",
            Some(reference_name(reference)),
            Some([mvd.x_q4, mvd.y_q4]),
        ),
    }
}

const fn reference_name(reference: ReferenceFrame) -> &'static str {
    match reference {
        ReferenceFrame::Last => "last",
        ReferenceFrame::Golden => "golden",
    }
}

fn invalid(element: &'static str) -> BitstreamError {
    BitstreamError::InvalidField { offset: 0, element }
}

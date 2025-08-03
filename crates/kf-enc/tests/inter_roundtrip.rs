use kf_bitstream::{
    BlockSize, FrameFlags, FramePacket, FrameType, MotionVector, PartitionTree, PlaneClass,
    Prediction, ReferenceFrame, SEQUENCE_HEADER_SIZE, SequenceHeader, SyntaxReader, SyntaxWriter,
    TransformBlockSize,
};
use kf_dec::FastDecoder;
use kf_enc::{Encoder, IntraEncoder};
use kf_frame::Frame;
use kf_range::ContextBank;
use kf_ref::ReferenceDecoder;

fn sequence() -> SequenceHeader {
    SequenceHeader::new(64, 64, 24, 1, 120, 16).unwrap()
}

fn noise_frame() -> Frame {
    let mut frame = Frame::filled_420(64, 64, 0).unwrap();
    let mut state = 0x8f31_a25c_u32;
    for sample in frame
        .y
        .data_mut()
        .iter_mut()
        .chain(frame.cb.data_mut())
        .chain(frame.cr.data_mut())
    {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *sample = state.to_le_bytes()[0];
    }
    frame
}

#[test]
fn trap_encdec_mismatch_and_all_pframe_branches() {
    let gray = Frame::filled_420(64, 64, 96).unwrap();
    let sources = vec![gray.clone(), gray, noise_frame()];
    let encoder = Encoder::new(sequence(), 32).unwrap();
    let first = encoder.encode(&sources).unwrap();
    let second = encoder.encode(&sources).unwrap();
    assert_eq!(first.bytes, second.bytes);
    assert_eq!(first.frames.len(), 3);
    assert!(first.frames[0].key);
    assert!(!first.frames[1].key);
    assert!(!first.frames[2].key);
    assert!(first.frames[1].superblocks.iter().any(|superblock| {
        superblock.blocks.iter().any(|block| {
            matches!(
                block.prediction,
                Prediction::Skip { .. } | Prediction::Inter { .. }
            )
        })
    }));
    assert!(first.frames[2].superblocks.iter().any(|superblock| {
        superblock
            .blocks
            .iter()
            .any(|block| matches!(block.prediction, Prediction::Intra(_)))
    }));

    let decoded = FastDecoder::new().decode_stream(&first.bytes).unwrap();
    assert_eq!(decoded, first.reconstructed_frames);
    let reference = ReferenceDecoder::new().decode_stream(&first.bytes).unwrap();
    assert_eq!(decoded, reference);
}

#[test]
fn trap_refresh_flags_authoritative() {
    let mut key_source = Frame::filled_420(64, 64, 0).unwrap();
    for y in 0..64 {
        for x in 0..64 {
            key_source
                .y
                .set(x, y, u8::try_from((x * 3 + y * 5) % 256).unwrap())
                .unwrap();
        }
    }
    let key_stream = IntraEncoder::new(sequence(), 0)
        .unwrap()
        .encode(&[key_source])
        .unwrap();
    let (key_packet, consumed) =
        FramePacket::decode(&key_stream.bytes[SEQUENCE_HEADER_SIZE..]).unwrap();
    assert_eq!(consumed, key_stream.bytes.len() - SEQUENCE_HEADER_SIZE);
    let contexts = contexts_after_key(&key_packet);

    let mut first_p = SyntaxWriter::new(contexts);
    first_p
        .write_partition(&PartitionTree::Leaf(BlockSize::N64))
        .unwrap();
    first_p
        .write_prediction(
            FrameType::P,
            Prediction::Inter {
                reference: ReferenceFrame::Last,
                mvd: MotionVector { x_q4: 4, y_q4: 0 },
            },
        )
        .unwrap();
    write_zero_residual(&mut first_p, BlockSize::N64);
    let (first_payload, contexts) = first_p.finish();
    let first_packet = FramePacket::new(
        1,
        FrameFlags {
            key: false,
            golden_refresh: true,
            show: true,
        },
        0,
        first_payload.bytes,
    )
    .unwrap();

    let mut second_p = SyntaxWriter::new(contexts);
    second_p
        .write_partition(&PartitionTree::Leaf(BlockSize::N64))
        .unwrap();
    second_p
        .write_prediction(
            FrameType::P,
            Prediction::Skip {
                reference: ReferenceFrame::Golden,
            },
        )
        .unwrap();
    let (second_payload, _contexts) = second_p.finish();
    let second_packet = FramePacket::new(
        2,
        FrameFlags {
            key: false,
            golden_refresh: false,
            show: true,
        },
        0,
        second_payload.bytes,
    )
    .unwrap();

    let mut stream = sequence().encode().to_vec();
    stream.extend_from_slice(&key_packet.encode());
    stream.extend_from_slice(&first_packet.encode());
    stream.extend_from_slice(&second_packet.encode());
    let fast = FastDecoder::new().decode_stream(&stream).unwrap();
    let reference = ReferenceDecoder::new().decode_stream(&stream).unwrap();
    assert_eq!(fast, reference);
    assert_ne!(fast[0], fast[1]);
    assert_eq!(fast[1], fast[2]);
}

fn contexts_after_key(packet: &FramePacket) -> ContextBank {
    let mut reader = SyntaxReader::new(&packet.payload, ContextBank::initial()).unwrap();
    let partition = reader.read_partition().unwrap();
    for size in partition.leaves() {
        assert!(matches!(
            reader.read_prediction(FrameType::Key).unwrap(),
            Prediction::Intra(_)
        ));
        for (plane, transform, count) in transform_schedule(size) {
            for _ in 0..count {
                reader.read_coefficients(plane, transform).unwrap();
            }
        }
    }
    reader.into_contexts()
}

fn write_zero_residual(writer: &mut SyntaxWriter, size: BlockSize) {
    for (plane, transform, count) in transform_schedule(size) {
        for _ in 0..count {
            writer
                .write_coefficients(plane, transform, &vec![0; transform.side().pow(2)])
                .unwrap();
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

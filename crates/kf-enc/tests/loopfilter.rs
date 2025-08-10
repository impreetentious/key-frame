use kf_bitstream::{
    BlockSize, FrameFlags, FramePacket, FrameType, PartitionTree, PlaneClass, Prediction,
    ReferenceFrame, SEQUENCE_HEADER_SIZE, SequenceHeader, SyntaxReader, SyntaxWriter,
    TransformBlockSize,
};
use kf_dec::FastDecoder;
use kf_enc::IntraEncoder;
use kf_frame::Frame;
use kf_predict::{CodedBlock, deblock_frame};
use kf_range::ContextBank;
use kf_ref::ReferenceDecoder;

fn sequence() -> SequenceHeader {
    SequenceHeader::new(64, 64, 24, 1, 120, 16).unwrap()
}

fn seam_frame() -> Frame {
    let mut frame = Frame::filled_420(64, 64, 100).unwrap();
    for y in 0..64 {
        for x in 32..64 {
            frame.y.set(x, y, 140).unwrap();
        }
    }
    frame
}

fn skip_quarters() -> PartitionTree {
    PartitionTree::Split {
        size: BlockSize::N64,
        children: Box::new([
            PartitionTree::Leaf(BlockSize::N32),
            PartitionTree::Leaf(BlockSize::N32),
            PartitionTree::Leaf(BlockSize::N32),
            PartitionTree::Leaf(BlockSize::N32),
        ]),
    }
}

#[test]
fn trap_loopfilter_ref_identity() {
    let source = seam_frame();
    let key_stream = IntraEncoder::new(sequence(), 32)
        .unwrap()
        .encode(std::slice::from_ref(&source))
        .unwrap();
    let (key_packet, _) = FramePacket::decode(&key_stream.bytes[SEQUENCE_HEADER_SIZE..]).unwrap();
    let contexts = contexts_after_key(&key_packet);

    let mut skip = SyntaxWriter::new(contexts);
    skip.write_partition(&skip_quarters()).unwrap();
    for _ in 0..4 {
        skip.write_prediction(
            FrameType::P,
            Prediction::Skip {
                reference: ReferenceFrame::Last,
            },
        )
        .unwrap();
    }
    let (payload, _) = skip.finish();
    let skip_packet = FramePacket::new(
        1,
        FrameFlags {
            key: false,
            golden_refresh: false,
            show: true,
        },
        32,
        payload.bytes,
    )
    .unwrap();

    let mut stream = sequence().encode().to_vec();
    stream.extend_from_slice(&key_packet.encode());
    stream.extend_from_slice(&skip_packet.encode());

    let fast = FastDecoder::new().decode_stream(&stream).unwrap();
    let reference = ReferenceDecoder::new().decode_stream(&stream).unwrap();
    assert_eq!(fast, reference);
    assert_eq!(fast[0], key_stream.reconstructed_frames[0]);
    assert_ne!(fast[0].y.get(31, 8).unwrap(), source.y.get(31, 8).unwrap());

    let p_blocks = [
        CodedBlock {
            x: 0,
            y: 0,
            size: 32,
            intra: false,
            coded: false,
        },
        CodedBlock {
            x: 32,
            y: 0,
            size: 32,
            intra: false,
            coded: false,
        },
        CodedBlock {
            x: 0,
            y: 32,
            size: 32,
            intra: false,
            coded: false,
        },
        CodedBlock {
            x: 32,
            y: 32,
            size: 32,
            intra: false,
            coded: false,
        },
    ];
    let mut filtered_again = fast[0].clone();
    deblock_frame(&mut filtered_again, 32, &p_blocks).unwrap();
    assert_eq!(fast[1], filtered_again);
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

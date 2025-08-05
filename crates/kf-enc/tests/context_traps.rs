use kf_bitstream::{
    BlockSize, FramePacket, FrameType, PlaneClass, Prediction, SEQUENCE_HEADER_SIZE,
    SequenceHeader, SyntaxReader, TransformBlockSize,
};
use kf_dec::FastDecoder;
use kf_enc::Encoder;
use kf_frame::Frame;
use kf_range::ContextBank;
use kf_ref::ReferenceDecoder;

fn sequence() -> SequenceHeader {
    SequenceHeader::new(64, 64, 24, 1, 120, 16).unwrap()
}

fn gray() -> Frame {
    Frame::filled_420(64, 64, 80).unwrap()
}

fn noise() -> Frame {
    let mut frame = Frame::filled_420(64, 64, 0).unwrap();
    let mut state = 0x51a3_c9e2_u32;
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

fn packets(bytes: &[u8]) -> Vec<FramePacket> {
    let mut rest = &bytes[SEQUENCE_HEADER_SIZE..];
    let mut packets = Vec::new();
    while !rest.is_empty() {
        let (packet, consumed) = FramePacket::decode(rest).unwrap();
        packets.push(packet);
        rest = &rest[consumed..];
    }
    packets
}

fn replay(payload: &[u8], key: bool, bank: ContextBank) -> Result<[u16; 144], String> {
    let mut reader = SyntaxReader::new(payload, bank).map_err(|error| error.to_string())?;
    let partition = reader.read_partition().map_err(|error| error.to_string())?;
    let frame_type = if key { FrameType::Key } else { FrameType::P };
    for size in partition.leaves() {
        let prediction = reader
            .read_prediction(frame_type)
            .map_err(|error| error.to_string())?;
        if matches!(prediction, Prediction::Skip { .. }) {
            continue;
        }
        for (plane, transform, count) in transform_schedule(size) {
            for _ in 0..count {
                reader
                    .read_coefficients(plane, transform)
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(reader.into_contexts().p1_values())
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

#[test]
fn trap_context_frame_carry() {
    let encoded = Encoder::new(sequence(), 32)
        .unwrap()
        .encode(&[gray(), noise(), noise()])
        .unwrap();
    assert!(encoded.frames[0].key);
    assert!(!encoded.frames[1].key);
    assert!(!encoded.frames[2].key);
    let after_key = encoded.checkpoints[0];
    let after_first_p = encoded.checkpoints[1];
    assert_ne!(after_key, ContextBank::initial().p1_values());
    assert_ne!(after_first_p, after_key);

    let stream_packets = packets(&encoded.bytes);
    let carried = replay(
        &stream_packets[1].payload,
        false,
        ContextBank::from_p1_values(after_key).unwrap(),
    )
    .unwrap();
    assert_eq!(carried, after_first_p);

    let reset = replay(&stream_packets[1].payload, false, ContextBank::initial());
    if let Ok(values) = reset {
        assert_ne!(values, after_first_p);
    }

    let key_only: Vec<u8> =
        encoded.bytes[..SEQUENCE_HEADER_SIZE + stream_packets[0].encode().len()].to_vec();
    let mut fast = FastDecoder::new();
    let mut reference = ReferenceDecoder::new();
    fast.decode_stream(&key_only).unwrap();
    reference.decode_stream(&key_only).unwrap();
    assert_eq!(fast.committed_p1(), Some(after_key));
    assert_eq!(reference.committed_p1(), Some(after_key));
}

#[test]
fn trap_corrupt_context_transaction() {
    let encoded = Encoder::new(sequence(), 32)
        .unwrap()
        .encode(&[gray(), noise()])
        .unwrap();
    let stream_packets = packets(&encoded.bytes);
    let mut truncated = stream_packets[1].payload.clone();
    assert!(truncated.len() > 5);
    truncated.pop();
    let corrupt = FramePacket::new(
        1,
        stream_packets[1].flags,
        stream_packets[1].frame_qp,
        truncated,
    )
    .unwrap();
    let mut corrupt_stream = encoded.bytes[..SEQUENCE_HEADER_SIZE].to_vec();
    corrupt_stream.extend_from_slice(&stream_packets[0].encode());
    corrupt_stream.extend_from_slice(&corrupt.encode());

    let mut fast = FastDecoder::new();
    let mut reference = ReferenceDecoder::new();
    assert!(fast.decode_stream(&corrupt_stream).is_err());
    assert!(reference.decode_stream(&corrupt_stream).is_err());
    assert_eq!(fast.committed_p1(), None);
    assert_eq!(reference.committed_p1(), None);
    assert!(!fast.has_references());
    assert!(!reference.has_references());

    let key_only: Vec<u8> =
        encoded.bytes[..SEQUENCE_HEADER_SIZE + stream_packets[0].encode().len()].to_vec();
    let mut recovered = FastDecoder::new();
    recovered.decode_stream(&key_only).unwrap();
    assert_eq!(recovered.committed_p1(), Some(encoded.checkpoints[0]));
    assert!(recovered.has_references());
}

use kf_frame::Frame;
use kf_tools::{Y4mStream, decode_y4m, encode_y4m};

#[test]
fn strict_y4m_round_trip_preserves_planes_and_rate() {
    let mut frame = Frame::filled_420(64, 64, 17).unwrap();
    frame.y.set(63, 63, 99).unwrap();
    frame.cb.set(31, 31, 77).unwrap();
    let stream = Y4mStream {
        width: 64,
        height: 64,
        fps_num: 30_000,
        fps_den: 1_001,
        frames: vec![frame],
    };
    let bytes = encode_y4m(&stream).unwrap();
    assert_eq!(decode_y4m(&bytes).unwrap(), stream);
}

#[test]
fn frame_tags_from_standard_producers_are_accepted() {
    let stream = Y4mStream {
        width: 64,
        height: 64,
        fps_num: 24,
        fps_den: 1,
        frames: vec![Frame::filled_420(64, 64, 128).unwrap()],
    };
    let bytes = encode_y4m(&stream).unwrap();
    let marker = bytes
        .windows(6)
        .position(|window| window == b"FRAME\n")
        .unwrap();
    let mut tagged = bytes[..marker].to_vec();
    tagged.extend_from_slice(b"FRAME Xsource=standard\n");
    tagged.extend_from_slice(&bytes[marker + 6..]);
    assert_eq!(decode_y4m(&tagged).unwrap(), stream);
}

#[test]
fn omitted_chroma_tag_means_c420jpeg() {
    let stream = Y4mStream {
        width: 64,
        height: 64,
        fps_num: 24,
        fps_den: 1,
        frames: vec![Frame::filled_420(64, 64, 128).unwrap()],
    };
    let encoded = encode_y4m(&stream).unwrap();
    let marker = encoded.iter().position(|&byte| byte == b'\n').unwrap();
    let header = core::str::from_utf8(&encoded[..marker])
        .unwrap()
        .replace(" C420jpeg", "");
    let mut omitted = header.into_bytes();
    omitted.extend_from_slice(&encoded[marker..]);
    assert_eq!(decode_y4m(&omitted).unwrap(), stream);
}

#[test]
fn unsupported_chroma_and_truncated_payload_are_named_errors() {
    let bad_chroma = b"YUV4MPEG2 W64 H64 F24:1 Ip C420mpeg2\n";
    assert_eq!(decode_y4m(bad_chroma).unwrap_err().element, "header.chroma");
    let truncated = b"YUV4MPEG2 W64 H64 F24:1 Ip C420jpeg\nFRAME\n\0";
    assert_eq!(decode_y4m(truncated).unwrap_err().element, "frame.payload");
}

use kf_frame::{Frame, Plane};
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

/// A header token whose first character is more than one byte wide.
///
/// The reader split every token at byte one, which panics on a character
/// boundary it lands inside. A Y4M header is UTF-8 here, so this aborted the
/// process — `kfenc` on a file with an unusual header did not print an error,
/// it died — and the whole point of `Y4mError` is that this module answers a
/// bad file with an offset and an element name.
#[test]
fn a_multibyte_header_token_is_named_rather_than_fatal() {
    for header in [
        "YUV4MPEG2 W64 H64 F24:1 Ip Ünknown C420jpeg",
        "YUV4MPEG2 W64 H64 F24:1 Ip ★ C420jpeg",
        "YUV4MPEG2 W64 H64 F24:1 Ip 😀tag C420jpeg",
    ] {
        let mut bytes = header.as_bytes().to_vec();
        bytes.push(b'\n');
        bytes.extend_from_slice(b"FRAME\n");
        bytes.extend_from_slice(&vec![0_u8; 64 * 64 * 3 / 2]);
        assert_eq!(
            decode_y4m(&bytes).unwrap_err().element,
            "header.token",
            "{header}"
        );
    }
}

#[test]
fn an_unknown_single_letter_token_is_also_named() {
    let mut bytes = b"YUV4MPEG2 W64 H64 F24:1 Ip Q7 C420jpeg\n".to_vec();
    bytes.extend_from_slice(b"FRAME\n");
    bytes.extend_from_slice(&vec![0_u8; 64 * 64 * 3 / 2]);
    assert_eq!(decode_y4m(&bytes).unwrap_err().element, "header.token");
}

#[test]
fn dimensions_outside_the_format_are_refused() {
    // The format's declared bounds, refused at the reader rather than several
    // layers later where the message would be about a picture size.
    for header in [
        "YUV4MPEG2 W32 H64 F24:1 Ip C420jpeg",
        "YUV4MPEG2 W64 H32 F24:1 Ip C420jpeg",
        "YUV4MPEG2 W65 H64 F24:1 Ip C420jpeg",
        "YUV4MPEG2 W64 H65 F24:1 Ip C420jpeg",
    ] {
        let mut bytes = header.as_bytes().to_vec();
        bytes.push(b'\n');
        assert_eq!(
            decode_y4m(&bytes).unwrap_err().element,
            "header.dimensions",
            "{header}"
        );
    }
}

#[test]
fn a_padded_plane_is_written_as_the_picture_rather_than_the_buffer() {
    // A plane may be wider in memory than in picture. The writer appended the
    // whole backing buffer, so a padded frame produced a file whose frames were
    // `stride x height` bytes long and whose rows did not line up — silently,
    // and only on the next read.
    let mut frame = Frame::filled_420(64, 64, 0).unwrap();
    frame.y = Plane::with_stride(64, 64, 96, 0).unwrap();
    frame.cb = Plane::with_stride(32, 32, 48, 128).unwrap();
    frame.cr = Plane::with_stride(32, 32, 48, 128).unwrap();
    // A picture that is not uniform, so a misaligned row cannot pass unnoticed.
    for y in 0..64 {
        for x in 0..64 {
            frame
                .y
                .set(x, y, u8::try_from((x * 3 + y * 5) % 251).unwrap())
                .unwrap();
        }
    }

    let stream = Y4mStream {
        width: 64,
        height: 64,
        fps_num: 24,
        fps_den: 1,
        frames: vec![frame.clone()],
    };
    let bytes = encode_y4m(&stream).unwrap();

    // The file is the size the format says, not the size the buffer happens to be.
    let header_end = bytes.iter().position(|&byte| byte == b'\n').unwrap() + 1;
    assert_eq!(bytes.len() - header_end, b"FRAME\n".len() + 64 * 64 * 3 / 2);

    // And every displayed sample survives the round trip, padding discarded.
    let read_back = decode_y4m(&bytes).unwrap();
    assert_eq!(read_back.frames.len(), 1);
    for y in 0..64 {
        for x in 0..64 {
            assert_eq!(
                read_back.frames[0].y.get(x, y).unwrap(),
                frame.y.get(x, y).unwrap(),
                "luma ({x}, {y})"
            );
        }
    }
}

#[test]
fn a_frame_with_wrong_sized_chroma_is_refused() {
    // The planes are public fields, so a frame can be assembled with chroma
    // that is not half the luma. Writing it would produce frames of the wrong
    // length rather than an error.
    let mut frame = Frame::filled_420(64, 64, 0).unwrap();
    frame.cb = Plane::filled(16, 16, 128).unwrap();
    let stream = Y4mStream {
        width: 64,
        height: 64,
        fps_num: 24,
        fps_den: 1,
        frames: vec![frame],
    };
    assert_eq!(
        encode_y4m(&stream).unwrap_err().element,
        "frame.chroma_dimensions"
    );
}

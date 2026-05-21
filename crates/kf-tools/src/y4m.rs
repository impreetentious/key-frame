use core::fmt;

use kf_frame::Frame;

/// One strict 8-bit 4:2:0 YUV4MPEG2 stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Y4mStream {
    pub width: u16,
    pub height: u16,
    pub fps_num: u16,
    pub fps_den: u16,
    pub frames: Vec<Frame>,
}

/// Named Y4M parse or serialization error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Y4mError {
    pub offset: usize,
    pub element: &'static str,
}

impl fmt::Display for Y4mError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid Y4M {} at byte {}",
            self.element, self.offset
        )
    }
}

impl std::error::Error for Y4mError {}

/// Parses a complete progressive `C420jpeg` YUV4MPEG2 stream.
pub fn decode_y4m(bytes: &[u8]) -> Result<Y4mStream, Y4mError> {
    let header_end = bytes
        .iter()
        .position(|&byte| byte == b'\n')
        .ok_or_else(|| error(bytes.len(), "header.newline"))?;
    let header = core::str::from_utf8(&bytes[..header_end]).map_err(|_| error(0, "header.utf8"))?;
    let mut tokens = header.split_ascii_whitespace();
    if tokens.next() != Some("YUV4MPEG2") {
        return Err(error(0, "header.magic"));
    }
    let mut width = None;
    let mut height = None;
    let mut fps = None;
    let mut chroma = None;
    for token in tokens {
        // By character, not by byte. `split_at(1)` panics when the first
        // character is not one byte wide, and a Y4M header is only required to
        // be UTF-8 here — so a header carrying any non-ASCII token aborted the
        // process instead of producing the named error this module exists to
        // produce. Nothing reaches this parser but files a caller chose, and
        // that is exactly the argument for refusing rather than panicking.
        let mut characters = token.chars();
        let kind = characters.next().ok_or_else(|| error(0, "header.token"))?;
        let value = characters.as_str();
        match kind {
            'W' => width = Some(parse_u16(value, 0, "header.width")?),
            'H' => height = Some(parse_u16(value, 0, "header.height")?),
            'F' => fps = Some(parse_ratio(value, "header.fps")?),
            'I' if value == "p" => {}
            'I' => return Err(error(0, "header.interlace")),
            'C' => chroma = Some(value),
            'A' | 'X' => {}
            _ => return Err(error(0, "header.token")),
        }
    }
    let width = width.ok_or_else(|| error(0, "header.width"))?;
    let height = height.ok_or_else(|| error(0, "header.height"))?;
    let (fps_num, fps_den) = fps.ok_or_else(|| error(0, "header.fps"))?;
    if chroma.is_some_and(|value| value != "420jpeg") {
        return Err(error(0, "header.chroma"));
    }
    if !(64..=4096).contains(&width)
        || !(64..=2304).contains(&height)
        || !width.is_multiple_of(2)
        || !height.is_multiple_of(2)
    {
        return Err(error(0, "header.dimensions"));
    }
    let y_length = usize::from(width) * usize::from(height);
    let chroma_length = y_length / 4;
    let frame_length = y_length + chroma_length * 2;
    let mut offset = header_end + 1;
    let mut frames = Vec::new();
    while offset < bytes.len() {
        let marker_end = bytes[offset..]
            .iter()
            .position(|&byte| byte == b'\n')
            .map(|relative| offset + relative)
            .ok_or_else(|| error(offset, "frame.marker"))?;
        let marker = core::str::from_utf8(&bytes[offset..marker_end])
            .map_err(|_| error(offset, "frame.marker"))?;
        if marker != "FRAME" && !marker.starts_with("FRAME ") {
            return Err(error(offset, "frame.marker"));
        }
        offset = marker_end + 1;
        let end = offset
            .checked_add(frame_length)
            .ok_or_else(|| error(offset, "frame.length"))?;
        let payload = bytes
            .get(offset..end)
            .ok_or_else(|| error(bytes.len(), "frame.payload"))?;
        let mut frame = Frame::filled_420(u32::from(width), u32::from(height), 0)
            .map_err(|_| error(offset, "frame.allocate"))?;
        frame.y.data_mut().copy_from_slice(&payload[..y_length]);
        frame
            .cb
            .data_mut()
            .copy_from_slice(&payload[y_length..y_length + chroma_length]);
        frame
            .cr
            .data_mut()
            .copy_from_slice(&payload[y_length + chroma_length..]);
        frames.push(frame);
        offset = end;
    }
    if frames.is_empty() {
        return Err(error(offset, "frames.empty"));
    }
    Ok(Y4mStream {
        width,
        height,
        fps_num,
        fps_den,
        frames,
    })
}

/// Serializes tight 4:2:0 frames as progressive `C420jpeg` Y4M.
pub fn encode_y4m(stream: &Y4mStream) -> Result<Vec<u8>, Y4mError> {
    if stream.frames.is_empty() || stream.fps_num == 0 || stream.fps_den == 0 {
        return Err(error(0, "stream.metadata"));
    }
    if stream.frames.iter().any(|frame| {
        frame.width() != u32::from(stream.width) || frame.height() != u32::from(stream.height)
    }) {
        return Err(error(0, "frame.dimensions"));
    }
    // The chroma planes are checked too. `Frame::filled_420` sizes them, but
    // the planes are public fields, and a frame carrying a wrong-sized chroma
    // plane would otherwise be written as a file whose frames are the wrong
    // length — corrupt, and only detectably so on the next read.
    if stream.frames.iter().any(|frame| {
        let (half_width, half_height) = (frame.width() / 2, frame.height() / 2);
        frame.cb.width() != half_width
            || frame.cb.height() != half_height
            || frame.cr.width() != half_width
            || frame.cr.height() != half_height
    }) {
        return Err(error(0, "frame.chroma_dimensions"));
    }
    let mut bytes = format!(
        "YUV4MPEG2 W{} H{} F{}:{} Ip A0:0 C420jpeg\n",
        stream.width, stream.height, stream.fps_num, stream.fps_den
    )
    .into_bytes();
    for frame in &stream.frames {
        bytes.extend_from_slice(b"FRAME\n");
        append_displayed(&frame.y, &mut bytes);
        append_displayed(&frame.cb, &mut bytes);
        append_displayed(&frame.cr, &mut bytes);
    }
    Ok(bytes)
}

/// Appends one plane's displayed samples, row by row, skipping stride padding.
///
/// A plane may be wider in memory than it is in picture — `Plane::with_stride`
/// allows it and the decoder's internal buffers use it — and this wrote the
/// backing buffer whole. Every path that reaches it today happens to hand it
/// tightly packed planes, so the file was right by luck rather than by rule:
/// the first padded frame written here would have produced a Y4M whose frames
/// are `stride x height` bytes long, which the reader on the other side would
/// have taken apart into rows that do not line up.
///
/// It is the same defect the quality metrics carried until they were made to
/// read displayed samples, and the same one the WebAssembly boundary packs
/// against. This is the third place it could hide and the last that was open.
fn append_displayed(plane: &kf_frame::Plane, into: &mut Vec<u8>) {
    let width = plane.width() as usize;
    let stride = plane.stride() as usize;
    let data = plane.data();
    for row in 0..plane.height() as usize {
        let start = row * stride;
        into.extend_from_slice(&data[start..start + width]);
    }
}

fn parse_u16(value: &str, offset: usize, element: &'static str) -> Result<u16, Y4mError> {
    value.parse::<u16>().map_err(|_| error(offset, element))
}

fn parse_ratio(value: &str, element: &'static str) -> Result<(u16, u16), Y4mError> {
    let (numerator, denominator) = value.split_once(':').ok_or_else(|| error(0, element))?;
    let ratio = (
        parse_u16(numerator, 0, element)?,
        parse_u16(denominator, 0, element)?,
    );
    if ratio.0 == 0 || ratio.1 == 0 {
        return Err(error(0, element));
    }
    Ok(ratio)
}

const fn error(offset: usize, element: &'static str) -> Y4mError {
    Y4mError { offset, element }
}

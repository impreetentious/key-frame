use crate::{ReferenceError, crc::crc32c, limits, reader::ByteReader};

const FRAME_HEADER_SIZE: usize = 24;

pub(crate) struct RefPacket<'a> {
    pub payload: &'a [u8],
    pub frame_index: u32,
    pub qp: u8,
    pub key: bool,
    pub golden_refresh: bool,
    pub consumed: usize,
}

/// Header facts a recovering decoder still needs when the payload is unusable.
#[derive(Clone, Copy)]
pub(crate) struct RefHeaderFacts {
    pub frame_index: u32,
    pub key: bool,
}

/// One outcome of advancing the independent cursor.
pub(crate) enum RefScanEvent<'a> {
    Packet(RefPacket<'a>),
    /// Header validated, payload CRC failed; the declared extent was consumed.
    PayloadCorrupt(RefHeaderFacts),
    /// Header validated, payload runs past the end of the region.
    Truncated(RefHeaderFacts),
    End,
}

/// Independent packet cursor. Failed sync or header candidates advance one byte.
pub(crate) struct PacketCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
    last_seen_index: Option<u32>,
}

impl<'a> PacketCursor<'a> {
    pub(crate) const fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            offset: 0,
            last_seen_index: None,
        }
    }

    pub(crate) fn next_packet(&mut self) -> Result<Option<RefPacket<'a>>, ReferenceError> {
        while self.offset + 4 <= self.bytes.len() {
            if &self.bytes[self.offset..self.offset + 4] != b"KFP1" {
                self.offset += 1;
                continue;
            }
            let remaining = &self.bytes[self.offset..];
            let header = match read_header(remaining) {
                Ok(header) => header,
                Err(_) => {
                    self.offset += 1;
                    continue;
                }
            };
            if self
                .last_seen_index
                .is_some_and(|previous| header.frame_index <= previous)
            {
                self.offset += 1;
                continue;
            }
            let extent = FRAME_HEADER_SIZE
                + usize::try_from(header.payload_len)
                    .map_err(|_| ReferenceError::new(4, "packet.payload_len"))?;
            self.last_seen_index = Some(header.frame_index);
            if remaining.len() < extent {
                self.offset = self.bytes.len();
                return Err(ReferenceError::new(
                    u32::try_from(self.bytes.len()).unwrap_or(u32::MAX),
                    "packet.payload",
                ));
            }
            match read_packet(remaining) {
                Ok(packet) => {
                    self.offset += packet.consumed;
                    return Ok(Some(packet));
                }
                Err(error) if error.element == "packet.payload_crc" => {
                    self.offset += extent;
                    return Err(error);
                }
                Err(_) => {
                    self.offset += 1;
                }
            }
        }
        Ok(None)
    }
}

impl<'a> PacketCursor<'a> {
    /// Advances to the next outcome, distinguishing damage that can be stepped
    /// over from damage that ends the region. Byte advancement is identical to
    /// `next_packet`, so the two agree on where every packet sits.
    pub(crate) fn next_event(&mut self) -> RefScanEvent<'a> {
        while self.offset + 4 <= self.bytes.len() {
            if &self.bytes[self.offset..self.offset + 4] != b"KFP1" {
                self.offset += 1;
                continue;
            }
            let remaining = &self.bytes[self.offset..];
            let Ok(header) = read_header(remaining) else {
                self.offset += 1;
                continue;
            };
            if self
                .last_seen_index
                .is_some_and(|previous| header.frame_index <= previous)
            {
                self.offset += 1;
                continue;
            }
            let Ok(payload_len) = usize::try_from(header.payload_len) else {
                self.offset += 1;
                continue;
            };
            let extent = FRAME_HEADER_SIZE + payload_len;
            self.last_seen_index = Some(header.frame_index);
            let facts = RefHeaderFacts {
                frame_index: header.frame_index,
                key: header.key,
            };
            if remaining.len() < extent {
                self.offset = self.bytes.len();
                return RefScanEvent::Truncated(facts);
            }
            match read_packet(remaining) {
                Ok(packet) => {
                    self.offset += packet.consumed;
                    return RefScanEvent::Packet(packet);
                }
                Err(error) if error.element == "packet.payload_crc" => {
                    self.offset += extent;
                    return RefScanEvent::PayloadCorrupt(facts);
                }
                Err(_) => {
                    self.offset += 1;
                }
            }
        }
        RefScanEvent::End
    }
}

#[derive(Debug)]
struct PacketHeader {
    payload_len: u32,
    frame_index: u32,
    key: bool,
}

fn read_header(bytes: &[u8]) -> Result<PacketHeader, ReferenceError> {
    if bytes.len() < FRAME_HEADER_SIZE {
        return Err(ReferenceError::new(
            u32::try_from(bytes.len()).unwrap_or(u32::MAX),
            "packet.header",
        ));
    }
    if &bytes[..4] != b"KFP1" {
        return Err(ReferenceError::new(0, "packet.sync"));
    }
    let payload_len = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let bounds = limits();
    if !(bounds.payload_minimum..=bounds.payload_maximum).contains(&payload_len) {
        return Err(ReferenceError::new(4, "packet.payload_len"));
    }
    let flag_bits = bytes[12];
    let key = flag_bits & 1 != 0;
    let golden_refresh = flag_bits & 2 != 0;
    let show = flag_bits & 4 != 0;
    if flag_bits & 0xf8 != 0 || !show || (key && !golden_refresh) {
        return Err(ReferenceError::new(12, "packet.flags"));
    }
    if bytes[13] > limits().qp_max || u16::from_le_bytes([bytes[14], bytes[15]]) != 0 {
        return Err(ReferenceError::new(13, "packet.qp_or_reserved"));
    }
    let header_crc = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    if crc32c(&bytes[4..16]) != header_crc {
        return Err(ReferenceError::new(16, "packet.header_crc"));
    }
    Ok(PacketHeader {
        payload_len,
        frame_index: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
        key,
    })
}

fn read_packet(bytes: &[u8]) -> Result<RefPacket<'_>, ReferenceError> {
    let mut reader = ByteReader::new(bytes);
    if reader.bytes(4, "packet.sync")? != b"KFP1" {
        return Err(ReferenceError::new(0, "packet.sync"));
    }
    let payload_len = reader.u32("packet.payload_len")?;
    let bounds = limits();
    if !(bounds.payload_minimum..=bounds.payload_maximum).contains(&payload_len) {
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
    if qp > limits().qp_max || reader.u16("packet.reserved")? != 0 {
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

#[cfg(test)]
mod tests {
    use super::{FRAME_HEADER_SIZE, read_header};
    use crate::crc::crc32c;
    use crate::limits;

    /// A structurally valid header declaring `payload_len`, sealed so that the
    /// only thing that can refuse it is the bound under test.
    fn header(payload_len: u32) -> Vec<u8> {
        let mut bytes = vec![0_u8; FRAME_HEADER_SIZE];
        bytes[..4].copy_from_slice(b"KFP1");
        bytes[4..8].copy_from_slice(&payload_len.to_le_bytes());
        // Frame zero, key, golden refresh, shown: the one flag combination a
        // first packet may carry.
        bytes[12] = 0b0000_0111;
        bytes[13] = 32;
        let checksum = crc32c(&bytes[4..16]);
        bytes[16..20].copy_from_slice(&checksum.to_le_bytes());
        bytes
    }

    #[test]
    fn the_payload_bounds_are_the_ones_the_specification_declares() {
        // The reference decoder held these as literals matching the production
        // decoder's literals, so the two agreed with each other rather than
        // with the document, and nothing compared them. No committed vector
        // reaches either boundary — the upper one is sixteen megabytes — so the
        // header is driven directly.
        let bounds = limits();

        assert_eq!(
            read_header(&header(bounds.payload_maximum + 1))
                .expect_err("a payload past the declared cap was admitted")
                .element,
            "packet.payload_len"
        );
        assert_eq!(
            read_header(&header(bounds.payload_minimum - 1))
                .expect_err("a payload under the decoder's initial read was admitted")
                .element,
            "packet.payload_len"
        );

        // And the positive half, so this cannot pass by refusing everything.
        // Only the length is under test here; the header carries no payload, so
        // whatever happens next is a different question.
        for length in [bounds.payload_minimum, bounds.payload_maximum] {
            assert_eq!(
                read_header(&header(length))
                    .expect("a length inside the declared bounds was refused")
                    .payload_len,
                length
            );
        }
    }
}

#[cfg(test)]
mod declared_limits {
    use crate::{ReferenceDecoder, limits};

    /// A sequence header carrying the fields given, checksum resealed so that
    /// the only thing that can refuse it is the limit under test.
    fn sequence(width: u16, height: u16, chroma: u8, depth: u8) -> Vec<u8> {
        let mut bytes = vec![0_u8; 24];
        bytes[..4].copy_from_slice(b"KFV1");
        bytes[4..6].copy_from_slice(&1_u16.to_le_bytes());
        bytes[6..8].copy_from_slice(&width.to_le_bytes());
        bytes[8..10].copy_from_slice(&height.to_le_bytes());
        bytes[10] = chroma;
        bytes[11] = depth;
        bytes[12..14].copy_from_slice(&24_u16.to_le_bytes());
        bytes[14..16].copy_from_slice(&1_u16.to_le_bytes());
        bytes[16..18].copy_from_slice(&120_u16.to_le_bytes());
        bytes[18] = 16;
        let checksum = crate::crc::crc32c(&bytes[..20]);
        bytes[20..24].copy_from_slice(&checksum.to_le_bytes());
        bytes
    }

    /// Whether this decoder gets past the sequence header of `bytes`.
    ///
    /// A header-only stream never decodes, so the question asked is narrower:
    /// did it fail on the header, or on the absence of packets after it?
    fn header_is_accepted(bytes: &[u8]) -> bool {
        match ReferenceDecoder::new().decode_stream(bytes) {
            Ok(_) => true,
            Err(error) => !error.element.starts_with("sequence."),
        }
    }

    #[test]
    fn the_picture_bounds_are_the_ones_the_specification_declares() {
        // The production decoder is driven this way by
        // `crates/kf-bitstream/tests/normative_limits.rs`. This side carried
        // `64..=4096` and `64..=2304` as literals and nothing compared them
        // with the declaration or with the other decoder.
        let bounds = limits();
        let side = |value: u32| u16::try_from(value).expect("a declared picture bound fits u16");
        let (min_width, max_width) = (side(bounds.min_width), side(bounds.max_width));
        let (min_height, max_height) = (side(bounds.min_height), side(bounds.max_height));

        assert!(header_is_accepted(&sequence(
            min_width,
            min_height,
            bounds.chroma_code,
            bounds.bit_depth
        )));
        assert!(header_is_accepted(&sequence(
            max_width,
            max_height,
            bounds.chroma_code,
            bounds.bit_depth
        )));
        assert!(!header_is_accepted(&sequence(
            min_width - 2,
            min_height,
            bounds.chroma_code,
            bounds.bit_depth
        )));
        assert!(!header_is_accepted(&sequence(
            min_width,
            min_height - 2,
            bounds.chroma_code,
            bounds.bit_depth
        )));
        assert!(!header_is_accepted(&sequence(
            max_width + 2,
            max_height,
            bounds.chroma_code,
            bounds.bit_depth
        )));
        assert!(!header_is_accepted(&sequence(
            min_width,
            max_height + 2,
            bounds.chroma_code,
            bounds.bit_depth
        )));
    }

    #[test]
    fn the_sample_format_is_the_one_the_specification_declares() {
        let bounds = limits();
        let side = |value: u32| u16::try_from(value).expect("a declared picture bound fits u16");
        let (width, height) = (side(bounds.min_width), side(bounds.min_height));
        assert!(header_is_accepted(&sequence(
            width,
            height,
            bounds.chroma_code,
            bounds.bit_depth
        )));
        assert!(!header_is_accepted(&sequence(
            width,
            height,
            bounds.chroma_code + 1,
            bounds.bit_depth
        )));
        assert!(!header_is_accepted(&sequence(
            width,
            height,
            bounds.chroma_code,
            bounds.bit_depth + 2
        )));
    }

    #[test]
    fn the_declared_limits_are_the_shape_a_reader_would_expect() {
        // Not a comparison of two literals: this asserts the relations that
        // make the declarations usable at all, so a declaration edited into
        // nonsense fails here rather than several layers away.
        let bounds = limits();
        assert!(bounds.min_width <= bounds.max_width);
        assert!(bounds.min_height <= bounds.max_height);
        assert_eq!(bounds.min_width % 2, 0);
        assert_eq!(bounds.max_width % 2, 0);
        assert!(bounds.min_width >= bounds.superblock_size);
        assert!(bounds.payload_minimum < bounds.payload_maximum);
        assert!(bounds.qp_max > 0);
        assert!(bounds.coefficient_abs_max > 0);
    }
}

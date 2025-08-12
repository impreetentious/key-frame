use crate::{ReferenceError, crc::crc32c, reader::ByteReader};

const FRAME_HEADER_SIZE: usize = 24;
const MAX_PAYLOAD: u32 = 16 * 1024 * 1024;

pub(crate) struct RefPacket<'a> {
    pub payload: &'a [u8],
    pub frame_index: u32,
    pub qp: u8,
    pub key: bool,
    pub golden_refresh: bool,
    pub consumed: usize,
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

struct PacketHeader {
    payload_len: u32,
    frame_index: u32,
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
    if !(5..=MAX_PAYLOAD).contains(&payload_len) {
        return Err(ReferenceError::new(4, "packet.payload_len"));
    }
    let flag_bits = bytes[12];
    let key = flag_bits & 1 != 0;
    let golden_refresh = flag_bits & 2 != 0;
    let show = flag_bits & 4 != 0;
    if flag_bits & 0xf8 != 0 || !show || (key && !golden_refresh) {
        return Err(ReferenceError::new(12, "packet.flags"));
    }
    if bytes[13] > 63 || u16::from_le_bytes([bytes[14], bytes[15]]) != 0 {
        return Err(ReferenceError::new(13, "packet.qp_or_reserved"));
    }
    let header_crc = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    if crc32c(&bytes[4..16]) != header_crc {
        return Err(ReferenceError::new(16, "packet.header_crc"));
    }
    Ok(PacketHeader {
        payload_len,
        frame_index: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
    })
}

fn read_packet(bytes: &[u8]) -> Result<RefPacket<'_>, ReferenceError> {
    let mut reader = ByteReader::new(bytes);
    if reader.bytes(4, "packet.sync")? != b"KFP1" {
        return Err(ReferenceError::new(0, "packet.sync"));
    }
    let payload_len = reader.u32("packet.payload_len")?;
    if !(5..=MAX_PAYLOAD).contains(&payload_len) {
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

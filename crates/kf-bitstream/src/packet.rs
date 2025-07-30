use kf_core::crc32c;

use crate::BitstreamError;

/// Fixed B.2 frame-header extent before payload bytes.
pub const FRAME_HEADER_SIZE: usize = 24;
const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

/// Authoritative version-one packet flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameFlags {
    pub key: bool,
    pub golden_refresh: bool,
    pub show: bool,
}

impl FrameFlags {
    fn bits(self) -> u8 {
        u8::from(self.key) | (u8::from(self.golden_refresh) << 1) | (u8::from(self.show) << 2)
    }

    fn decode(bits: u8) -> Result<Self, BitstreamError> {
        if bits & 0xF8 != 0 {
            return Err(BitstreamError::InvalidField {
                offset: 12,
                element: "packet.flags.reserved",
            });
        }
        let flags = Self {
            key: bits & 1 != 0,
            golden_refresh: bits & 2 != 0,
            show: bits & 4 != 0,
        };
        if !flags.show || (flags.key && !flags.golden_refresh) {
            return Err(BitstreamError::InvalidField {
                offset: 12,
                element: "packet.flags.combination",
            });
        }
        Ok(flags)
    }
}

/// Structurally validated fixed packet header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PacketHeader {
    pub payload_len: u32,
    pub frame_index: u32,
    pub flags: FrameFlags,
    pub frame_qp: u8,
    pub payload_crc32c: u32,
}

impl PacketHeader {
    /// Validates a fixed B.2 header without touching the payload.
    pub fn decode(bytes: &[u8]) -> Result<Self, BitstreamError> {
        if bytes.len() < FRAME_HEADER_SIZE {
            return Err(BitstreamError::UnexpectedEof {
                offset: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
                element: "packet.header",
            });
        }
        if &bytes[..4] != b"KFP1" {
            return Err(BitstreamError::InvalidField {
                offset: 0,
                element: "packet.sync",
            });
        }
        let payload_len = get_u32(bytes, 4);
        if !(5..=u32::try_from(MAX_PAYLOAD).expect("invariant: payload cap fits u32"))
            .contains(&payload_len)
        {
            return Err(BitstreamError::InvalidField {
                offset: 4,
                element: "packet.payload_len",
            });
        }
        let flags = FrameFlags::decode(bytes[12])?;
        if bytes[13] > 63 {
            return Err(BitstreamError::InvalidField {
                offset: 13,
                element: "packet.frame_qp",
            });
        }
        if get_u16(bytes, 14) != 0 {
            return Err(BitstreamError::InvalidField {
                offset: 14,
                element: "packet.reserved",
            });
        }
        if crc32c(&bytes[4..16]) != get_u32(bytes, 16) {
            return Err(BitstreamError::CrcMismatch {
                offset: 16,
                element: "packet.header_crc32c",
            });
        }
        Ok(Self {
            payload_len,
            frame_index: get_u32(bytes, 8),
            flags,
            frame_qp: bytes[13],
            payload_crc32c: get_u32(bytes, 20),
        })
    }

    /// Header plus declared payload extent.
    pub fn extent(self) -> Result<usize, BitstreamError> {
        FRAME_HEADER_SIZE
            .checked_add(usize::try_from(self.payload_len).map_err(|_| {
                BitstreamError::InvalidField {
                    offset: 4,
                    element: "packet.payload_len",
                }
            })?)
            .ok_or(BitstreamError::InvalidField {
                offset: 4,
                element: "packet.payload_len",
            })
    }
}

/// One validated packet and its CRC-bound range payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FramePacket {
    pub frame_index: u32,
    pub flags: FrameFlags,
    pub frame_qp: u8,
    pub payload: Vec<u8>,
}

impl FramePacket {
    /// Constructs a packet after validating flags, QP, and payload bounds.
    pub fn new(
        frame_index: u32,
        flags: FrameFlags,
        frame_qp: u8,
        payload: Vec<u8>,
    ) -> Result<Self, BitstreamError> {
        FrameFlags::decode(flags.bits())?;
        if frame_qp > 63 {
            return Err(BitstreamError::InvalidField {
                offset: 13,
                element: "packet.frame_qp",
            });
        }
        if !(5..=MAX_PAYLOAD).contains(&payload.len()) {
            return Err(BitstreamError::InvalidField {
                offset: 4,
                element: "packet.payload_len",
            });
        }
        Ok(Self {
            frame_index,
            flags,
            frame_qp,
            payload,
        })
    }

    /// Serializes the exact B.2 header and payload.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let payload_len = u32::try_from(self.payload.len())
            .expect("invariant: validated packet payload length fits u32");
        let mut bytes = vec![0_u8; FRAME_HEADER_SIZE + self.payload.len()];
        bytes[..4].copy_from_slice(b"KFP1");
        put_u32(&mut bytes, 4, payload_len);
        put_u32(&mut bytes, 8, self.frame_index);
        bytes[12] = self.flags.bits();
        bytes[13] = self.frame_qp;
        put_u16(&mut bytes, 14, 0);
        let header_crc = crc32c(&bytes[4..16]);
        put_u32(&mut bytes, 16, header_crc);
        let payload_crc = crc32c(&self.payload);
        put_u32(&mut bytes, 20, payload_crc);
        bytes[FRAME_HEADER_SIZE..].copy_from_slice(&self.payload);
        bytes
    }

    /// Parses one packet at the beginning of `bytes`.
    pub fn decode(bytes: &[u8]) -> Result<(Self, usize), BitstreamError> {
        let header = PacketHeader::decode(bytes)?;
        let extent = header.extent()?;
        if bytes.len() < extent {
            return Err(BitstreamError::UnexpectedEof {
                offset: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
                element: "packet.payload",
            });
        }
        let payload = &bytes[FRAME_HEADER_SIZE..extent];
        if crc32c(payload) != header.payload_crc32c {
            return Err(BitstreamError::CrcMismatch {
                offset: 20,
                element: "packet.payload_crc32c",
            });
        }
        Ok((
            Self {
                frame_index: header.frame_index,
                flags: header.flags,
                frame_qp: header.frame_qp,
                payload: payload.to_vec(),
            },
            extent,
        ))
    }
}

fn get_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn get_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

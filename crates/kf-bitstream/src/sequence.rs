use kf_core::crc32c;

use crate::BitstreamError;

/// Only accepted bitstream version.
pub const BITSTREAM_VERSION: u16 = 1;
/// Fixed sequence-header extent.
pub const SEQUENCE_HEADER_SIZE: usize = 24;

/// Validated version-one sequence metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SequenceHeader {
    pub width: u16,
    pub height: u16,
    pub fps_num: u16,
    pub fps_den: u16,
    pub keyframe_interval: u16,
    pub golden_interval: u8,
}

impl SequenceHeader {
    /// Constructs and validates version-one sequence metadata.
    pub fn new(
        width: u16,
        height: u16,
        fps_num: u16,
        fps_den: u16,
        keyframe_interval: u16,
        golden_interval: u8,
    ) -> Result<Self, BitstreamError> {
        let header = Self {
            width,
            height,
            fps_num,
            fps_den,
            keyframe_interval,
            golden_interval,
        };
        header.validate()?;
        Ok(header)
    }

    /// Serializes the exact 24-byte B.1 representation.
    #[must_use]
    pub fn encode(self) -> [u8; SEQUENCE_HEADER_SIZE] {
        let mut bytes = [0_u8; SEQUENCE_HEADER_SIZE];
        bytes[0..4].copy_from_slice(b"KFV1");
        put_u16(&mut bytes, 4, BITSTREAM_VERSION);
        put_u16(&mut bytes, 6, self.width);
        put_u16(&mut bytes, 8, self.height);
        bytes[10] = 1;
        bytes[11] = 8;
        put_u16(&mut bytes, 12, self.fps_num);
        put_u16(&mut bytes, 14, self.fps_den);
        put_u16(&mut bytes, 16, self.keyframe_interval);
        bytes[18] = self.golden_interval;
        bytes[19] = 0;
        let checksum = crc32c(&bytes[..20]);
        put_u32(&mut bytes, 20, checksum);
        bytes
    }

    /// Parses and validates one B.1 header.
    pub fn decode(bytes: &[u8]) -> Result<Self, BitstreamError> {
        if bytes.len() < SEQUENCE_HEADER_SIZE {
            return Err(BitstreamError::UnexpectedEof {
                offset: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
                element: "sequence_header",
            });
        }
        if &bytes[0..4] != b"KFV1" {
            return Err(BitstreamError::InvalidField {
                offset: 0,
                element: "sequence.magic",
            });
        }
        let version = get_u16(bytes, 4);
        if version != BITSTREAM_VERSION {
            return Err(BitstreamError::UnsupportedVersion { version });
        }
        if bytes[10] != 1 {
            return Err(BitstreamError::InvalidField {
                offset: 10,
                element: "sequence.chroma",
            });
        }
        if bytes[11] != 8 {
            return Err(BitstreamError::InvalidField {
                offset: 11,
                element: "sequence.depth",
            });
        }
        if bytes[19] != 0 {
            return Err(BitstreamError::InvalidField {
                offset: 19,
                element: "sequence.flags",
            });
        }
        let expected = get_u32(bytes, 20);
        if crc32c(&bytes[..20]) != expected {
            return Err(BitstreamError::CrcMismatch {
                offset: 20,
                element: "sequence.header_crc32c",
            });
        }
        Self::new(
            get_u16(bytes, 6),
            get_u16(bytes, 8),
            get_u16(bytes, 12),
            get_u16(bytes, 14),
            get_u16(bytes, 16),
            bytes[18],
        )
    }

    fn validate(self) -> Result<(), BitstreamError> {
        // Width and height are checked apart, because the offset an error
        // carries exists to name the field that was wrong. Both used to be
        // reported as `sequence.dimensions` at byte 6, so a header whose only
        // fault was its height sent a reader to the two bytes that hold the
        // width. `crates/kf-bitstream/tests/offset_probe.rs` printed exactly
        // that mismatch for as long as the file existed and asserted nothing.
        if self.width < 64 || self.width > 4096 || !self.width.is_multiple_of(2) {
            return Err(BitstreamError::InvalidField {
                offset: 6,
                element: "sequence.width",
            });
        }
        if self.height < 64 || self.height > 2304 || !self.height.is_multiple_of(2) {
            return Err(BitstreamError::InvalidField {
                offset: 8,
                element: "sequence.height",
            });
        }
        for (value, offset, element) in [
            (self.fps_num, 12, "sequence.fps_num"),
            (self.fps_den, 14, "sequence.fps_den"),
            (self.keyframe_interval, 16, "sequence.kf_interval"),
            (
                u16::from(self.golden_interval),
                18,
                "sequence.golden_interval",
            ),
        ] {
            if value == 0 {
                return Err(BitstreamError::InvalidField { offset, element });
            }
        }
        Ok(())
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

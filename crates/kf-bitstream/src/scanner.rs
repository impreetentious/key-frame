use crate::{BitstreamError, FRAME_HEADER_SIZE, FramePacket, PacketHeader};

/// Resynchronizing packet scanner over bytes following a sequence header.
#[derive(Clone, Debug)]
pub struct PacketScanner<'a> {
    bytes: &'a [u8],
    offset: usize,
    last_seen_index: Option<u32>,
}

impl<'a> PacketScanner<'a> {
    /// Starts scanning at byte zero of a packet region.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            offset: 0,
            last_seen_index: None,
        }
    }

    /// Locates the next structurally admissible packet.
    pub fn next_packet(&mut self) -> Result<Option<FramePacket>, BitstreamError> {
        while self.offset + 4 <= self.bytes.len() {
            if &self.bytes[self.offset..self.offset + 4] != b"KFP1" {
                self.offset += 1;
                continue;
            }
            let remaining = &self.bytes[self.offset..];
            let header = match PacketHeader::decode(remaining) {
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
            let extent = header.extent()?;
            self.last_seen_index = Some(header.frame_index);
            if remaining.len() < extent {
                self.offset = self.bytes.len();
                return Err(BitstreamError::UnexpectedEof {
                    offset: u32::try_from(self.bytes.len()).unwrap_or(u32::MAX),
                    element: "packet.payload",
                });
            }
            match FramePacket::decode(remaining) {
                Ok((packet, consumed)) => {
                    self.offset += consumed;
                    return Ok(Some(packet));
                }
                Err(
                    error @ BitstreamError::CrcMismatch {
                        element: "packet.payload_crc32c",
                        ..
                    },
                ) => {
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

    /// Current scanner byte offset.
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Most recent structurally accepted frame index.
    #[must_use]
    pub const fn last_seen_index(&self) -> Option<u32> {
        self.last_seen_index
    }

    /// Minimum bytes needed before fixed-header parsing can begin.
    #[must_use]
    pub const fn fixed_header_size() -> usize {
        FRAME_HEADER_SIZE
    }
}

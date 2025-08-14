use crate::{BitstreamError, FRAME_HEADER_SIZE, FramePacket, PacketHeader};

/// One structurally accepted scan outcome.
///
/// A strict caller wants "packet or error" and uses [`PacketScanner::next_packet`].
/// A recovering decoder has to distinguish damage it can step over from damage
/// that ends the region, and it needs the header even when the payload is
/// unusable: the recovery rules key off the frame index and the key flag of a
/// packet whose payload never decodes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScanEvent {
    /// Header and payload both validated.
    Packet(FramePacket),
    /// Header validated, payload CRC failed. The declared extent was consumed.
    PayloadCorrupt(PacketHeader),
    /// Header validated, payload runs past the end of the region.
    Truncated(PacketHeader),
    /// No further sync word admits a header.
    End,
}

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

    /// Locates the next scan outcome, reporting recoverable damage instead of
    /// collapsing it into an error.
    ///
    /// Advancement matches [`Self::next_packet`] exactly, so a caller that
    /// switches between them sees the same byte positions: a failed sync or
    /// header candidate advances one byte, a payload-CRC failure consumes the
    /// capped declared extent, and a truncated payload ends the region.
    pub fn next_event(&mut self) -> ScanEvent {
        while self.offset + 4 <= self.bytes.len() {
            if &self.bytes[self.offset..self.offset + 4] != b"KFP1" {
                self.offset += 1;
                continue;
            }
            let remaining = &self.bytes[self.offset..];
            let Ok(header) = PacketHeader::decode(remaining) else {
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
            let Ok(extent) = header.extent() else {
                self.offset += 1;
                continue;
            };
            self.last_seen_index = Some(header.frame_index);
            if remaining.len() < extent {
                self.offset = self.bytes.len();
                return ScanEvent::Truncated(header);
            }
            match FramePacket::decode(remaining) {
                Ok((packet, consumed)) => {
                    self.offset += consumed;
                    return ScanEvent::Packet(packet);
                }
                Err(BitstreamError::CrcMismatch {
                    element: "packet.payload_crc32c",
                    ..
                }) => {
                    self.offset += extent;
                    return ScanEvent::PayloadCorrupt(header);
                }
                Err(_) => {
                    self.offset += 1;
                }
            }
        }
        ScanEvent::End
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

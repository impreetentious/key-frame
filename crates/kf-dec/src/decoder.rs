use kf_bitstream::{FrameType, PacketScanner, SEQUENCE_HEADER_SIZE, SequenceHeader};
use kf_frame::Frame;
use kf_range::ContextBank;

use crate::{
    DecodeError,
    reconstruct::{ReferenceFrames, decode_payload},
};

/// Transactional scalar decode state.
#[derive(Clone, Debug, Default)]
pub struct FastDecoder {
    last: Option<Frame>,
    golden: Option<Frame>,
    contexts: Option<ContextBank>,
    last_frame_index: Option<u32>,
}

impl FastDecoder {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last: None,
            golden: None,
            contexts: None,
            last_frame_index: None,
        }
    }

    /// Decodes every frame in one complete version-one stream.
    pub fn decode_stream(&mut self, bytes: &[u8]) -> Result<Vec<Frame>, DecodeError> {
        Ok(self.decode_stream_traced(bytes)?.0)
    }

    /// Decodes a stream and records the context bank after every superblock.
    pub fn decode_stream_traced(
        &mut self,
        bytes: &[u8],
    ) -> Result<(Vec<Frame>, Vec<[u16; 144]>), DecodeError> {
        self.invalidate();
        let sequence = SequenceHeader::decode(bytes)?;
        let packet_bytes = bytes
            .get(SEQUENCE_HEADER_SIZE..)
            .ok_or(DecodeError::InvalidFrame {
                frame_index: 0,
                element: "packet_region",
            })?;
        let mut scanner = PacketScanner::new(packet_bytes);
        let mut frames = Vec::new();
        let mut checkpoints = Vec::new();
        loop {
            let packet = match scanner.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => break,
                Err(error) => {
                    self.invalidate();
                    return Err(error.into());
                }
            };
            let expected_index = self
                .last_frame_index
                .map_or(0, |index| index.saturating_add(1));
            if packet.frame_index != expected_index {
                self.invalidate();
                return Err(DecodeError::InvalidFrame {
                    frame_index: packet.frame_index,
                    element: "packet.frame_index_gap",
                });
            }
            let frame_type = if packet.flags.key {
                FrameType::Key
            } else {
                FrameType::P
            };
            let contexts = if packet.flags.key {
                ContextBank::initial()
            } else {
                self.contexts.clone().ok_or(DecodeError::InvalidFrame {
                    frame_index: packet.frame_index,
                    element: "pframe.contexts",
                })?
            };
            let references = match (&self.last, &self.golden) {
                (Some(last), Some(golden)) => Some(ReferenceFrames { last, golden }),
                _ => None,
            };
            if frame_type == FrameType::P && references.is_none() {
                self.invalidate();
                return Err(DecodeError::InvalidFrame {
                    frame_index: packet.frame_index,
                    element: "pframe.references",
                });
            }
            let (padded_frame, final_contexts, frame_checkpoints) = match decode_payload(
                &sequence,
                &packet.payload,
                packet.frame_index,
                packet.frame_qp,
                contexts,
                frame_type,
                references,
            ) {
                Ok(decoded) => decoded,
                Err(error) => {
                    self.invalidate();
                    return Err(error);
                }
            };
            let visible_frame = padded_frame
                .crop_420(u32::from(sequence.width), u32::from(sequence.height))
                .map_err(|_| DecodeError::InvalidFrame {
                    frame_index: packet.frame_index,
                    element: "frame.crop",
                })?;
            self.last = Some(padded_frame.clone());
            if packet.flags.key || packet.flags.golden_refresh {
                self.golden = Some(padded_frame);
            }
            self.contexts = Some(final_contexts);
            self.last_frame_index = Some(packet.frame_index);
            checkpoints.extend(frame_checkpoints);
            frames.push(visible_frame);
        }
        if frames.is_empty() {
            return Err(DecodeError::InvalidFrame {
                frame_index: 0,
                element: "initial_keyframe",
            });
        }
        Ok((frames, checkpoints))
    }

    /// Compatibility entry point for the pre-inter staging suite.
    pub fn decode_intra_stream(&mut self, bytes: &[u8]) -> Result<Vec<Frame>, DecodeError> {
        self.decode_stream(bytes)
    }

    #[must_use]
    pub const fn has_references(&self) -> bool {
        self.last.is_some() && self.golden.is_some()
    }

    /// Committed context bank, or `None` after invalidation.
    #[must_use]
    pub fn committed_p1(&self) -> Option<[u16; 144]> {
        self.contexts.as_ref().map(ContextBank::p1_values)
    }

    fn invalidate(&mut self) {
        self.last = None;
        self.golden = None;
        self.contexts = None;
        self.last_frame_index = None;
    }
}

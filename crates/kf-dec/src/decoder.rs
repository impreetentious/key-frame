use kf_bitstream::{FrameType, PacketScanner, SEQUENCE_HEADER_SIZE, ScanEvent, SequenceHeader};
use kf_frame::Frame;
use kf_range::ContextBank;

use crate::{
    DecodeError,
    reconstruct::{ReferenceFrames, decode_payload},
    status::{FrameStatus, Recovery, StreamReport},
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

    /// Compatibility entry point for single-frame intra callers.
    pub fn decode_intra_stream(&mut self, bytes: &[u8]) -> Result<Vec<Frame>, DecodeError> {
        self.decode_stream(bytes)
    }

    /// Decodes a stream that may be damaged, reporting every packet's outcome
    /// instead of failing at the first one.
    ///
    /// A malformed sequence header is still fatal: without it nothing in the
    /// region can be interpreted. Everything after it is recoverable. The rules
    /// this implements, in the order they apply to each packet:
    ///
    /// - An index gap is a dependency-loss event decided *before* the packet is
    ///   classified, so references and context continuity are dropped first and
    ///   a good keyframe in the same packet still recovers immediately.
    /// - While a keyframe is required, a structurally accepted header still
    ///   advances the index, but a non-key is `DependencyLost` and a bad payload
    ///   is `Corrupt`; neither is entropy-decoded.
    /// - A keyframe starts from literal context initials and needs no old
    ///   reference, so it can always be attempted.
    /// - Installation into LAST, GOLDEN, and the context bank happens only after
    ///   a completely successful decode, so no corrupt, held, or dependency-lost
    ///   state can reach later prediction.
    pub fn decode_stream_resilient(&mut self, bytes: &[u8]) -> Result<StreamReport, DecodeError> {
        self.invalidate();
        let sequence = SequenceHeader::decode(bytes)?;
        let packet_bytes = bytes
            .get(SEQUENCE_HEADER_SIZE..)
            .ok_or(DecodeError::InvalidFrame {
                frame_index: 0,
                element: "packet_region",
            })?;
        let mut scanner = PacketScanner::new(packet_bytes);
        let mut report = StreamReport::default();
        let mut needs_keyframe = true;
        let mut next_expected_index = 0u32;
        loop {
            let (header_index, header_key, payload) = match scanner.next_event() {
                ScanEvent::Packet(packet) => (packet.frame_index, packet.flags.key, Some(packet)),
                ScanEvent::PayloadCorrupt(header) | ScanEvent::Truncated(header) => {
                    (header.frame_index, header.flags.key, None)
                }
                ScanEvent::End => break,
            };

            // Dependency loss is decided before the packet is classified.
            let recovery = if header_index != next_expected_index {
                self.invalidate();
                needs_keyframe = true;
                Some(if next_expected_index == 0 {
                    Recovery::LeadingLoss
                } else {
                    Recovery::Gap
                })
            } else {
                None
            };
            next_expected_index = header_index.saturating_add(1);

            let Some(packet) = payload else {
                self.invalidate();
                needs_keyframe = true;
                report.statuses.push(FrameStatus::Corrupt);
                continue;
            };
            if !header_key && (needs_keyframe || !self.has_references()) {
                self.invalidate();
                needs_keyframe = true;
                report.statuses.push(FrameStatus::DependencyLost);
                continue;
            }

            let frame_type = if header_key {
                FrameType::Key
            } else {
                FrameType::P
            };
            let contexts = if header_key {
                ContextBank::initial()
            } else {
                match self.contexts.clone() {
                    Some(contexts) => contexts,
                    None => {
                        self.invalidate();
                        needs_keyframe = true;
                        report.statuses.push(FrameStatus::DependencyLost);
                        continue;
                    }
                }
            };
            let references = match (&self.last, &self.golden) {
                (Some(last), Some(golden)) => Some(ReferenceFrames { last, golden }),
                _ => None,
            };
            let decoded = decode_payload(
                &sequence,
                &packet.payload,
                packet.frame_index,
                packet.frame_qp,
                contexts,
                frame_type,
                references,
            );
            let Ok((padded_frame, final_contexts, _)) = decoded else {
                self.invalidate();
                needs_keyframe = true;
                report.statuses.push(FrameStatus::Corrupt);
                continue;
            };
            let Ok(visible_frame) =
                padded_frame.crop_420(u32::from(sequence.width), u32::from(sequence.height))
            else {
                self.invalidate();
                needs_keyframe = true;
                report.statuses.push(FrameStatus::Corrupt);
                continue;
            };

            self.last = Some(padded_frame.clone());
            if header_key || packet.flags.golden_refresh {
                self.golden = Some(padded_frame);
            }
            self.contexts = Some(final_contexts);
            self.last_frame_index = Some(packet.frame_index);
            needs_keyframe = false;
            report.frames.push(visible_frame);
            report.statuses.push(match recovery {
                Some(recovery) if header_key => FrameStatus::RecoveredKeyframe(recovery),
                _ => FrameStatus::Shown,
            });
        }
        Ok(report)
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

use kf_bitstream::{
    ElementCoverage, FrameType, PacketScanner, SEQUENCE_HEADER_SIZE, ScanEvent, SequenceHeader,
};
use kf_frame::Frame;
use kf_range::{ContextBank, CoverageCounter};

use crate::{
    DecodeError,
    reconstruct::{ReferenceFrames, decode_payload},
    seek::{SeekOutcome, StreamIndex},
    status::{FrameStatus, Recovery, StreamReport},
};

/// A decode plus everything it was instrumented for.
struct Instrumented {
    frames: Vec<Frame>,
    checkpoints: Vec<[u16; 144]>,
    coverage: StreamCoverage,
}

/// What a whole stream coded, measured while decoding it.
///
/// This is the only honest way to state conformance coverage: an inventory
/// file can claim a vector exercises an element, but only a decode proves it.
#[derive(Clone, Debug)]
pub struct StreamCoverage {
    /// Frozen context ids touched anywhere in the stream.
    pub contexts: CoverageCounter,
    /// Named syntax elements coded anywhere in the stream.
    pub elements: ElementCoverage,
}

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

    /// Decodes a stream and reports which contexts and elements it coded.
    pub fn decode_stream_coverage(
        &mut self,
        bytes: &[u8],
    ) -> Result<(Vec<Frame>, StreamCoverage), DecodeError> {
        let decoded = self.decode_stream_instrumented(bytes)?;
        Ok((decoded.frames, decoded.coverage))
    }

    /// Decodes the single frame at `target` by restarting from the keyframe
    /// that governs it.
    ///
    /// Afterwards the decoder holds the state it would hold had it decoded
    /// linearly to `target`, so the caller can keep going from there.
    pub fn seek_frame(&mut self, bytes: &[u8], target: u32) -> Result<SeekOutcome, DecodeError> {
        // Before anything else, including the index scan: a seek that fails
        // must not leave the previous stream's references or context bank
        // installed, or a later decode could predict from an image the caller
        // never asked for.
        self.invalidate();
        let index = StreamIndex::scan(bytes)?;
        let keyframe_index = index.entry_point(target)?;
        let decoded = self.decode_window(bytes, Some((keyframe_index, target)))?;
        let frames_decoded = decoded.frames.len();
        let frame = decoded
            .frames
            .into_iter()
            .next_back()
            .ok_or(DecodeError::InvalidFrame {
                frame_index: target,
                element: "seek.target",
            })?;
        Ok(SeekOutcome {
            frame,
            keyframe_index,
            frames_decoded,
        })
    }

    /// Decodes from `target` to the end of the stream, restarting from the
    /// keyframe that governs `target` and discarding the frames before it.
    pub fn decode_from(&mut self, bytes: &[u8], target: u32) -> Result<Vec<Frame>, DecodeError> {
        self.invalidate();
        let index = StreamIndex::scan(bytes)?;
        let keyframe_index = index.entry_point(target)?;
        let last = u32::try_from(index.frame_count().saturating_sub(1)).map_err(|_| {
            DecodeError::InvalidFrame {
                frame_index: target,
                element: "seek.target",
            }
        })?;
        let decoded = self.decode_window(bytes, Some((keyframe_index, last)))?;
        let skip =
            usize::try_from(target - keyframe_index).map_err(|_| DecodeError::InvalidFrame {
                frame_index: target,
                element: "seek.target",
            })?;
        Ok(decoded.frames.into_iter().skip(skip).collect())
    }

    /// Decodes a stream and records the context bank after every superblock.
    pub fn decode_stream_traced(
        &mut self,
        bytes: &[u8],
    ) -> Result<(Vec<Frame>, Vec<[u16; 144]>), DecodeError> {
        let decoded = self.decode_stream_instrumented(bytes)?;
        Ok((decoded.frames, decoded.checkpoints))
    }

    fn decode_stream_instrumented(&mut self, bytes: &[u8]) -> Result<Instrumented, DecodeError> {
        self.decode_window(bytes, None)
    }

    /// Decodes either a whole stream (`window` absent) or the inclusive index
    /// range a seek asked for.
    ///
    /// A window's first packet must be the keyframe the caller resolved: a
    /// keyframe needs neither carried contexts nor an old reference, so it is
    /// the only place a decode can legitimately begin part-way into a stream.
    /// Packets before it are scanned — their headers and payload checksums are
    /// still validated — but never entropy-decoded, which is the whole point of
    /// seeking.
    fn decode_window(
        &mut self,
        bytes: &[u8],
        window: Option<(u32, u32)>,
    ) -> Result<Instrumented, DecodeError> {
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
        let mut coverage = StreamCoverage {
            contexts: CoverageCounter::new(),
            elements: ElementCoverage::new(),
        };
        loop {
            let packet = match scanner.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => break,
                Err(error) => {
                    self.invalidate();
                    return Err(error.into());
                }
            };
            if let Some((start, _)) = window
                && packet.frame_index < start
            {
                continue;
            }
            let expected_index = self
                .last_frame_index
                .map_or(window.map_or(0, |(start, _)| start), |index| {
                    index.saturating_add(1)
                });
            if packet.frame_index != expected_index {
                self.invalidate();
                return Err(DecodeError::InvalidFrame {
                    frame_index: packet.frame_index,
                    element: "packet.frame_index_gap",
                });
            }
            if window.is_some_and(|(start, _)| packet.frame_index == start) && !packet.flags.key {
                self.invalidate();
                return Err(DecodeError::InvalidFrame {
                    frame_index: packet.frame_index,
                    element: "seek.entry_not_key",
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
            let decoded = match decode_payload(
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
            coverage.contexts.merge(&decoded.coverage);
            coverage.elements.merge(&decoded.elements);
            let padded_frame = decoded.frame;
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
            self.contexts = Some(decoded.contexts);
            self.last_frame_index = Some(packet.frame_index);
            checkpoints.extend(decoded.checkpoints);
            frames.push(visible_frame);
            if window.is_some_and(|(_, end)| packet.frame_index == end) {
                break;
            }
        }
        if frames.is_empty() {
            return Err(DecodeError::InvalidFrame {
                frame_index: 0,
                element: if window.is_some() {
                    "seek.entry_missing"
                } else {
                    "initial_keyframe"
                },
            });
        }
        if let Some((_, end)) = window
            && self.last_frame_index != Some(end)
        {
            self.invalidate();
            return Err(DecodeError::InvalidFrame {
                frame_index: end,
                element: "seek.target",
            });
        }
        Ok(Instrumented {
            frames,
            checkpoints,
            coverage,
        })
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
            let Ok(decoded) = decoded else {
                self.invalidate();
                needs_keyframe = true;
                report.statuses.push(FrameStatus::Corrupt);
                continue;
            };
            let padded_frame = decoded.frame;
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
            self.contexts = Some(decoded.contexts);
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

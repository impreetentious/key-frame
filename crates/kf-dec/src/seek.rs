//! Random access to a frame without decoding everything before it.
//!
//! A P frame is only meaningful after its references and its context bank, so
//! the earliest point a decode can begin is a keyframe: it resets the bank to
//! the literal initials and predicts from nothing. Seeking therefore means
//! finding the last keyframe at or before the requested frame and decoding
//! forward from there.
//!
//! The saving is real but bounded, and the boundary is the keyframe interval,
//! not the frame index. Seeking to the frame just before a keyframe is the
//! worst case and decodes almost a whole interval; seeking to a keyframe itself
//! decodes exactly one frame. `SeekOutcome` reports how many frames the seek
//! actually decoded so a caller can tell the difference rather than assume it.

use kf_bitstream::{PacketScanner, SEQUENCE_HEADER_SIZE, SequenceHeader};
use kf_frame::Frame;

use crate::DecodeError;

/// One frame recovered by seeking, with the evidence of how it was recovered.
#[derive(Clone, Debug)]
pub struct SeekOutcome {
    /// The requested frame, cropped to the display dimensions.
    pub frame: Frame,
    /// The keyframe the decode restarted from.
    pub keyframe_index: u32,
    /// How many frames were decoded to produce it, including the keyframe.
    pub frames_decoded: usize,
}

/// The index layout of a stream, read without entropy-decoding anything.
///
/// Building this validates every packet header and payload checksum in the
/// stream, exactly as a linear decode would, so a seek cannot succeed on a
/// stream a linear decode would reject.
#[derive(Clone, Debug, Default)]
pub struct StreamIndex {
    entries: Vec<Entry>,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    frame_index: u32,
    key: bool,
}

impl StreamIndex {
    /// Scans a complete stream and records what each packet claims to be.
    pub fn scan(bytes: &[u8]) -> Result<Self, DecodeError> {
        let _ = SequenceHeader::decode(bytes)?;
        let packet_bytes = bytes
            .get(SEQUENCE_HEADER_SIZE..)
            .ok_or(DecodeError::InvalidFrame {
                frame_index: 0,
                element: "packet_region",
            })?;
        let mut scanner = PacketScanner::new(packet_bytes);
        let mut entries: Vec<Entry> = Vec::new();
        while let Some(packet) = scanner.next_packet()? {
            let expected = entries
                .last()
                .map_or(0, |entry| entry.frame_index.saturating_add(1));
            if packet.frame_index != expected {
                return Err(DecodeError::InvalidFrame {
                    frame_index: packet.frame_index,
                    element: "packet.frame_index_gap",
                });
            }
            entries.push(Entry {
                frame_index: packet.frame_index,
                key: packet.flags.key,
            });
        }
        if entries.is_empty() {
            return Err(DecodeError::InvalidFrame {
                frame_index: 0,
                element: "initial_keyframe",
            });
        }
        Ok(Self { entries })
    }

    /// How many frames the stream holds.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.entries.len()
    }

    /// Every keyframe index, ascending. These are the only points a decode may
    /// begin at, and the only points a damaged stream may resume at.
    #[must_use]
    pub fn keyframes(&self) -> Vec<u32> {
        self.entries
            .iter()
            .filter(|entry| entry.key)
            .map(|entry| entry.frame_index)
            .collect()
    }

    /// The keyframe a seek to `target` must restart from.
    pub fn entry_point(&self, target: u32) -> Result<u32, DecodeError> {
        let position = usize::try_from(target).map_err(|_| DecodeError::InvalidFrame {
            frame_index: target,
            element: "seek.target",
        })?;
        if position >= self.entries.len() {
            return Err(DecodeError::InvalidFrame {
                frame_index: target,
                element: "seek.target",
            });
        }
        self.entries[..=position]
            .iter()
            .rev()
            .find(|entry| entry.key)
            .map(|entry| entry.frame_index)
            .ok_or(DecodeError::InvalidFrame {
                frame_index: target,
                element: "seek.entry_missing",
            })
    }
}

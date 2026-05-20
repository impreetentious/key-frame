//! The decode session behind the WebAssembly boundary.
//!
//! Everything here is ordinary safe Rust. The module is deliberately separate
//! from the exported ABI so that the boundary's pointer arithmetic stays in one
//! small, readable file and the logic it guards can be tested natively.

use kf_bitstream::{SEQUENCE_HEADER_SIZE, SequenceHeader};
use kf_dec::{FastDecoder, StreamIndex};
use kf_frame::Frame;
use kf_probe::probe_frame;

/// Every way a call across the boundary can fail.
///
/// The numbering is part of the ABI: a host reads the code, and the message is
/// only for humans. Codes are never reused for a different meaning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Status {
    Ok = 0,
    NoStream = 1,
    BadStream = 2,
    BadFrameIndex = 3,
    DecodeFailed = 4,
    ProbeFailed = 5,
}

impl Status {
    #[must_use]
    pub const fn code(self) -> u32 {
        // cast: the discriminant of a fieldless enum whose six values are zero
        // through five and are pinned one by one by
        // `status_codes_are_distinct_and_stable`. Nothing is discarded; the
        // codes are the boundary contract, so they are asserted rather than
        // left to declaration order.
        self as u32
    }
}

/// What a host learns about a stream before decoding any of it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StreamInfo {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub frame_count: u32,
}

/// One opened stream and the last thing asked of it.
///
/// The session keeps the stream bytes because random access needs them: a scrub
/// to an arbitrary frame re-enters at that frame's keyframe, which means reading
/// the packets again rather than replaying a decode the host has already
/// discarded.
#[derive(Default)]
pub struct Session {
    bytes: Vec<u8>,
    info: Option<StreamInfo>,
    keyframes: Vec<u32>,
    output: Vec<u8>,
    message: String,
    last_keyframe: u32,
    last_frames_decoded: u32,
}

impl Session {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Accepts a complete stream, validating its header and packet sequence
    /// without decoding any pixels.
    pub fn open(&mut self, bytes: Vec<u8>) -> Status {
        self.close();
        let Ok(sequence) = SequenceHeader::decode(&bytes) else {
            self.fail("the sequence header is not a version-one Key Frame header");
            return Status::BadStream;
        };
        if bytes.len() <= SEQUENCE_HEADER_SIZE {
            self.fail("the stream carries no packets");
            return Status::BadStream;
        }
        let index = match StreamIndex::scan(&bytes) {
            Ok(index) => index,
            Err(error) => {
                self.fail(&format!("the packet sequence is unusable: {error}"));
                return Status::BadStream;
            }
        };
        let Ok(frame_count) = u32::try_from(index.frame_count()) else {
            self.fail("the stream declares more frames than an index can hold");
            return Status::BadStream;
        };
        self.keyframes = index.keyframes();
        self.info = Some(StreamInfo {
            width: u32::from(sequence.width),
            height: u32::from(sequence.height),
            fps_num: u32::from(sequence.fps_num),
            fps_den: u32::from(sequence.fps_den),
            frame_count,
        });
        self.bytes = bytes;
        Status::Ok
    }

    /// Decodes one frame by random access and holds its raw planes for the host
    /// to read.
    pub fn decode_frame(&mut self, index: u32) -> Status {
        let Some(info) = self.info else {
            self.fail("no stream is open");
            return Status::NoStream;
        };
        if index >= info.frame_count {
            self.fail(&format!(
                "frame {index} is past the last frame of {}",
                info.frame_count
            ));
            return Status::BadFrameIndex;
        }
        match FastDecoder::new().seek_frame(&self.bytes, index) {
            Ok(outcome) => {
                self.output = raw_planes(&outcome.frame);
                self.last_keyframe = outcome.keyframe_index;
                self.last_frames_decoded =
                    u32::try_from(outcome.frames_decoded).unwrap_or(u32::MAX);
                self.message.clear();
                Status::Ok
            }
            Err(error) => {
                self.output.clear();
                self.fail(&format!("frame {index} did not decode: {error}"));
                Status::DecodeFailed
            }
        }
    }

    /// Reports one frame's syntax as JSON, holding it for the host to read as
    /// UTF-8 bytes.
    ///
    /// The probe parses; it does not reconstruct pixels. A host asking for both
    /// makes two calls, because the two answers have nothing to do with each
    /// other and batching them would only make the larger one wait.
    pub fn probe(&mut self, index: u32) -> Status {
        let Some(info) = self.info else {
            self.fail("no stream is open");
            return Status::NoStream;
        };
        if index >= info.frame_count {
            self.fail(&format!(
                "frame {index} is past the last frame of {}",
                info.frame_count
            ));
            return Status::BadFrameIndex;
        }
        match probe_frame(&self.bytes, index) {
            Ok(report) => {
                self.output = report.to_json().into_bytes();
                self.message.clear();
                Status::Ok
            }
            Err(error) => {
                self.output.clear();
                self.fail(&format!("frame {index} did not probe: {error}"));
                Status::ProbeFailed
            }
        }
    }

    /// Decodes every frame in order, holding them back to back. This is the
    /// path an equality check uses; a player scrubs with `decode_frame`.
    pub fn decode_all(&mut self) -> Status {
        if self.info.is_none() {
            self.fail("no stream is open");
            return Status::NoStream;
        }
        match FastDecoder::new().decode_stream(&self.bytes) {
            Ok(frames) => {
                self.output = frames.iter().flat_map(raw_planes).collect();
                self.message.clear();
                Status::Ok
            }
            Err(error) => {
                self.output.clear();
                self.fail(&format!("the stream did not decode: {error}"));
                Status::DecodeFailed
            }
        }
    }

    #[must_use]
    pub const fn info(&self) -> Option<StreamInfo> {
        self.info
    }

    /// Keyframe indices, which are the only points a scrub can jump to without
    /// decoding what precedes them.
    #[must_use]
    pub fn keyframes(&self) -> &[u32] {
        &self.keyframes
    }

    #[must_use]
    pub fn output(&self) -> &[u8] {
        &self.output
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The keyframe the last decode restarted from, and how many frames it
    /// cost. A scrubbing host uses these to show what random access is doing
    /// rather than guessing at it.
    #[must_use]
    pub const fn last_entry(&self) -> (u32, u32) {
        (self.last_keyframe, self.last_frames_decoded)
    }

    fn close(&mut self) {
        self.bytes.clear();
        self.info = None;
        self.keyframes.clear();
        self.output.clear();
        self.message.clear();
        self.last_keyframe = 0;
        self.last_frames_decoded = 0;
    }

    fn fail(&mut self, message: &str) {
        self.message.clear();
        self.message.push_str(message);
    }
}

/// The three planes as the host expects them: tightly packed, display samples
/// only.
///
/// A plane carries its own stride, and a padded plane's backing buffer holds
/// samples to the right of the picture that were never displayed. Copying the
/// buffer wholesale would hand the browser those samples as if they were
/// pixels, and the page would draw a skewed picture from a correct decode.
/// Frames reaching here are cropped today, so this reads the same bytes it
/// always did; it reads them from the geometry rather than from the allocation
/// so that it stays correct if a padded frame ever arrives.
fn raw_planes(frame: &Frame) -> Vec<u8> {
    let mut bytes =
        Vec::with_capacity(packed_len(&frame.y) + packed_len(&frame.cb) + packed_len(&frame.cr));
    pack_plane(&frame.y, &mut bytes);
    pack_plane(&frame.cb, &mut bytes);
    pack_plane(&frame.cr, &mut bytes);
    bytes
}

fn packed_len(plane: &kf_frame::Plane) -> usize {
    let width = as_index(plane.width());
    let height = as_index(plane.height());
    width * height
}

/// A plane extent as a buffer index.
///
/// Fallible rather than an `as` cast, because the cast is the one conversion
/// the compiler will not argue with: on a target where `usize` is narrower than
/// a plane dimension it would discard the high bits and produce a shorter
/// buffer than the picture, which the host would read as a valid frame of the
/// wrong size. Every supported target is wide enough, and that is a fact worth
/// stating where it is relied on.
fn as_index(extent: u32) -> usize {
    usize::try_from(extent).expect("invariant: a plane extent fits an index on this target")
}

/// Appends one plane's display samples, row by row, skipping any stride
/// padding the backing buffer carries.
fn pack_plane(plane: &kf_frame::Plane, into: &mut Vec<u8>) {
    let width = as_index(plane.width());
    let height = as_index(plane.height());
    let stride = as_index(plane.stride());
    let data = plane.data();
    for row in 0..height {
        let start = row * stride;
        into.extend_from_slice(&data[start..start + width]);
    }
}

#[cfg(test)]
mod tests {
    use super::{Session, Status};

    /// Padding is outside the picture and outside what the host receives.
    ///
    /// The browser is handed raw planes and told only the width and height, so
    /// a plane whose backing buffer is wider than its picture must still
    /// produce exactly width×height bytes. Reading the allocation instead of
    /// the geometry would hand the page never-displayed samples and skew every
    /// row of the image.
    #[test]
    fn stride_padding_never_reaches_the_host() {
        use kf_frame::Plane;
        let mut plane = Plane::with_stride(4, 2, 6, 0xEE).unwrap();
        for row in 0..2 {
            for x in 0..4 {
                plane
                    .set(x, row, 10 + u8::try_from(row * 4 + x).unwrap())
                    .unwrap();
            }
        }
        let mut packed = Vec::new();
        super::pack_plane(&plane, &mut packed);
        assert_eq!(packed, vec![10, 11, 12, 13, 14, 15, 16, 17]);
        assert_eq!(packed.len(), super::packed_len(&plane));
        assert!(
            !packed.contains(&0xEE),
            "a padding sample reached the host: {packed:?}"
        );
    }

    /// One oracle-authored keyframe, 64×64, DC intra, no coefficients.
    const ORACLE_STREAM: [u8; 54] = [
        0x4b, 0x46, 0x56, 0x31, 0x01, 0x00, 0x40, 0x00, 0x40, 0x00, 0x01, 0x08, 0x18, 0x00, 0x01,
        0x00, 0x78, 0x00, 0x10, 0x00, 0x0e, 0x72, 0xb9, 0x5d, 0x4b, 0x46, 0x50, 0x31, 0x06, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, 0x20, 0x00, 0x00, 0x23, 0x0e, 0x00, 0x74, 0x8a,
        0x7c, 0x2a, 0x57, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn opening_reports_the_stream_without_decoding_it() {
        let mut session = Session::new();
        assert_eq!(session.open(ORACLE_STREAM.to_vec()), Status::Ok);
        let info = session.info().unwrap();
        assert_eq!((info.width, info.height), (64, 64));
        assert_eq!(info.frame_count, 1);
        assert_eq!(session.keyframes(), &[0]);
        assert!(session.output().is_empty());
    }

    #[test]
    fn decoding_a_frame_fills_the_output_with_all_three_planes() {
        let mut session = Session::new();
        session.open(ORACLE_STREAM.to_vec());
        assert_eq!(session.decode_frame(0), Status::Ok);
        assert_eq!(session.output().len(), 64 * 64 + 2 * 32 * 32);
        assert!(session.output()[..64 * 64].iter().all(|&s| s == 128));
        assert_eq!(session.last_entry(), (0, 1));
    }

    #[test]
    fn a_frame_past_the_end_is_refused_and_named() {
        let mut session = Session::new();
        session.open(ORACLE_STREAM.to_vec());
        assert_eq!(session.decode_frame(1), Status::BadFrameIndex);
        assert!(session.message().contains("past the last frame"));
    }

    #[test]
    fn decoding_without_a_stream_is_refused() {
        let mut session = Session::new();
        assert_eq!(session.decode_frame(0), Status::NoStream);
        assert_eq!(session.decode_all(), Status::NoStream);
    }

    #[test]
    fn a_damaged_stream_is_refused_at_open_time() {
        let mut corrupt = ORACLE_STREAM;
        corrupt[53] ^= 1;
        let mut session = Session::new();
        assert_eq!(session.open(corrupt.to_vec()), Status::BadStream);
        assert!(session.info().is_none());
        assert!(!session.message().is_empty());
    }

    #[test]
    fn opening_a_second_stream_forgets_the_first() {
        let mut session = Session::new();
        session.open(ORACLE_STREAM.to_vec());
        session.decode_frame(0);
        assert!(!session.output().is_empty());
        assert_eq!(session.open(vec![0; 8]), Status::BadStream);
        assert!(session.info().is_none());
        assert!(session.output().is_empty());
        assert!(session.keyframes().is_empty());
    }

    #[test]
    fn decoding_everything_matches_decoding_the_single_frame() {
        let mut session = Session::new();
        session.open(ORACLE_STREAM.to_vec());
        session.decode_frame(0);
        let single = session.output().to_vec();
        session.decode_all();
        assert_eq!(session.output(), single.as_slice());
    }

    #[test]
    fn status_codes_are_distinct_and_stable() {
        assert_eq!(Status::Ok.code(), 0);
        assert_eq!(Status::NoStream.code(), 1);
        assert_eq!(Status::BadStream.code(), 2);
        assert_eq!(Status::BadFrameIndex.code(), 3);
        assert_eq!(Status::DecodeFailed.code(), 4);
        assert_eq!(Status::ProbeFailed.code(), 5);
    }

    #[test]
    fn probing_produces_the_same_json_the_command_line_tool_writes() {
        let mut session = Session::new();
        session.open(ORACLE_STREAM.to_vec());
        assert_eq!(session.probe(0), Status::Ok);
        let json = core::str::from_utf8(session.output()).expect("the probe emits UTF-8");
        assert_eq!(
            json,
            kf_probe::probe_frame(&ORACLE_STREAM, 0)
                .expect("the oracle stream probes")
                .to_json()
        );
        assert!(json.starts_with('{') && json.trim_end().ends_with('}'));
    }

    #[test]
    fn probing_without_a_stream_is_refused() {
        assert_eq!(Session::new().probe(0), Status::NoStream);
    }

    #[test]
    fn probing_a_frame_past_the_end_is_refused() {
        let mut session = Session::new();
        session.open(ORACLE_STREAM.to_vec());
        assert_eq!(session.probe(1), Status::BadFrameIndex);
    }
}

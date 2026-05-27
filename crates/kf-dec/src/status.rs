//! Per-frame outcomes of a recovering decode.
//!
//! A stream that has been damaged does not stop being a stream. The normative
//! rules say what a decoder owes its caller for every packet it walks past:
//! which image is safe to show, which reference state must be thrown away, and
//! when decoding may legitimately resume. These types are that report.

/// Why a keyframe counted as a recovery point rather than an ordinary frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Recovery {
    /// Frame indices skipped forward: packets were lost mid-stream.
    Gap,
    /// The stream's first accepted packet was not frame zero.
    LeadingLoss,
}

/// What happened to one structurally accepted packet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameStatus {
    /// Decoded and installed. Its image is the frame at the matching position.
    Shown,
    /// Decode failed. Both references and the context bank were discarded and
    /// the decoder now requires a keyframe. No image of its own.
    Corrupt,
    /// Structurally valid, but the state it predicts from is gone, so it was
    /// never entropy-decoded.
    DependencyLost,
    /// A keyframe that resumed decoding after loss.
    RecoveredKeyframe(Recovery),
}

impl FrameStatus {
    /// True when this packet produced a new image of its own.
    #[must_use]
    pub const fn produced_image(self) -> bool {
        matches!(self, Self::Shown | Self::RecoveredKeyframe(_))
    }
}

/// The result of walking one damaged stream to its end.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StreamReport {
    /// One entry per structurally accepted packet, in scan order.
    pub statuses: Vec<FrameStatus>,
    /// Images, in display order. Shorter than `statuses` whenever a packet was
    /// corrupt or dependency-lost: those contribute no image.
    pub frames: Vec<kf_frame::Frame>,
}

impl StreamReport {
    /// Count of packets that ended in the given status.
    #[must_use]
    pub fn count(&self, status: FrameStatus) -> usize {
        self.statuses.iter().filter(|&&s| s == status).count()
    }
}

//! Per-frame outcomes of a recovering decode, written independently of the
//! fast decoder's equivalent.
//!
//! These types deliberately duplicate `kf-dec`'s vocabulary rather than share
//! it. The recovery rules are normative decoder behavior, so two
//! implementations agreeing on them is evidence; one implementation imported
//! twice would be none.

/// Why a keyframe counted as a recovery point rather than an ordinary frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefRecovery {
    /// Frame indices skipped forward mid-stream.
    Gap,
    /// The first accepted packet of the stream was not frame zero.
    LeadingLoss,
}

/// What happened to one structurally accepted packet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefFrameStatus {
    /// Decoded and installed.
    Shown,
    /// Decode failed; references and contexts were discarded.
    Corrupt,
    /// Structurally valid but unpredictable from surviving state; never
    /// entropy-decoded.
    DependencyLost,
    /// A keyframe that resumed decoding after loss.
    RecoveredKeyframe(RefRecovery),
}

impl RefFrameStatus {
    /// True when this packet produced an image of its own.
    #[must_use]
    pub const fn produced_image(self) -> bool {
        matches!(self, Self::Shown | Self::RecoveredKeyframe(_))
    }
}

/// The result of walking one damaged stream to its end.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RefStreamReport {
    /// One entry per structurally accepted packet, in scan order.
    pub statuses: Vec<RefFrameStatus>,
    /// Images in display order; shorter than `statuses` when packets failed.
    pub frames: Vec<kf_frame::Frame>,
}

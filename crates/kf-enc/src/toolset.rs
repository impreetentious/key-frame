//! Which encoder tools are allowed to be used.
//!
//! An ablation answers "what does this tool buy?" by turning it off and
//! measuring the same clip again. That question is only worth asking if the
//! answer stays inside the format: every switch here narrows what the *encoder*
//! is willing to choose, and never what the bitstream means. A stream produced
//! with any combination below is an ordinary `.kfv` file that both decoders
//! reconstruct bit-exactly, which is what makes the resulting curve a fact
//! about a decision rather than about a fork.
//!
//! That constraint is also why the loop filter is not here. Deblocking in
//! version one is unconditional — there is no frame-header flag that turns it
//! off, because a decoder that could skip it would reconstruct a different
//! picture from the same bytes. Measuring "filter off" would mean shipping a
//! decoder that disagrees with the specification, so the honest thing is to
//! say plainly that this particular ablation is not available in version one
//! rather than to publish a number no released decoder can reproduce.

/// The tools an encoder may use while searching for a coding decision.
///
/// `full` is the shipping configuration and the one every conformance stream,
/// gate, and published headline number uses. The rest exist to be measured
/// against it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Toolset {
    /// Consider the GOLDEN reference as well as LAST.
    ///
    /// With this off, the golden slot is still maintained exactly as the format
    /// requires — a key frame still refreshes it — but no block ever selects
    /// it, so the curve shows what a second, older reference is worth.
    pub golden: bool,
    /// Consider `SKIP`, which codes a block as its predictor with no residual
    /// and no motion vector difference.
    pub skip: bool,
    /// Consider inter prediction at all. With this off, a P frame is still a P
    /// frame — it carries the same header and the same context state — but
    /// every block in it is coded with an intra mode.
    pub inter: bool,
    /// Refine motion vectors below full-pixel precision. With this off the
    /// search stops at integer positions, which is what isolates the cost and
    /// the benefit of the quarter-pixel interpolator.
    pub subpel: bool,
    /// Split superblocks below 64×64. With this off every superblock is one
    /// leaf, which is the cheapest possible partition syntax and the worst
    /// possible fit to real content.
    pub split: bool,
}

impl Toolset {
    /// Everything on: the shipping encoder.
    #[must_use]
    pub const fn full() -> Self {
        Self {
            golden: true,
            skip: true,
            inter: true,
            subpel: true,
            split: true,
        }
    }

    /// A named ablation, or `None` if the name is not one.
    ///
    /// Names are matched exactly and are part of the receipt format: a chart
    /// that says `no-subpel` has to mean the same thing next year as it does
    /// today, so this is a closed list rather than a parser.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        let full = Self::full();
        match name {
            "full" => Some(full),
            "no-golden" => Some(Self {
                golden: false,
                ..full
            }),
            "no-skip" => Some(Self {
                skip: false,
                ..full
            }),
            "no-inter" => Some(Self {
                inter: false,
                ..full
            }),
            "no-subpel" => Some(Self {
                subpel: false,
                ..full
            }),
            "no-split" => Some(Self {
                split: false,
                ..full
            }),
            _ => None,
        }
    }

    /// Every name `named` accepts, in reporting order.
    #[must_use]
    pub const fn names() -> [&'static str; 6] {
        [
            "full",
            "no-golden",
            "no-skip",
            "no-inter",
            "no-subpel",
            "no-split",
        ]
    }

    /// The name this toolset was built from, or `custom`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        for name in Self::names() {
            if Self::named(name) == Some(*self) {
                return name;
            }
        }
        "custom"
    }
}

impl Default for Toolset {
    fn default() -> Self {
        Self::full()
    }
}

#[cfg(test)]
mod tests {
    use super::Toolset;

    #[test]
    fn the_default_is_the_shipping_encoder() {
        // Every gate and every conformance stream goes through the default. If
        // it ever stopped being the full toolset, the suite would quietly start
        // proving something about an ablation instead.
        let full = Toolset::full();
        assert_eq!(Toolset::default(), full);
        assert!(full.golden && full.skip && full.inter && full.subpel && full.split);
        assert_eq!(full.name(), "full");
    }

    #[test]
    fn every_name_round_trips_and_turns_off_exactly_one_tool() {
        for name in Toolset::names() {
            let toolset = Toolset::named(name).expect("a named toolset");
            assert_eq!(toolset.name(), name);
            let full = Toolset::full();
            let disabled = [
                toolset.golden != full.golden,
                toolset.skip != full.skip,
                toolset.inter != full.inter,
                toolset.subpel != full.subpel,
                toolset.split != full.split,
            ]
            .iter()
            .filter(|changed| **changed)
            .count();
            assert_eq!(
                disabled,
                usize::from(name != "full"),
                "{name} changes {disabled} tools; an ablation must isolate one"
            );
        }
    }

    #[test]
    fn an_unknown_name_is_refused_rather_than_defaulted() {
        // Silently falling back to the full toolset would label an ablation
        // curve with a name it does not have.
        assert_eq!(Toolset::named("no-loopfilter"), None);
        assert_eq!(Toolset::named(""), None);
        assert_eq!(Toolset::named("FULL"), None);
    }

    #[test]
    fn a_combination_with_no_name_says_so() {
        let odd = Toolset {
            golden: false,
            subpel: false,
            ..Toolset::full()
        };
        assert_eq!(odd.name(), "custom");
    }
}

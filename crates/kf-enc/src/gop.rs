use std::collections::VecDeque;

use crate::EncodeError;

const SCENE_HISTORY_LIMIT: usize = 16;
const SCENE_HISTORY_MINIMUM: usize = 4;

/// Authoritative frame-class and reference-refresh decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameDecision {
    pub key: bool,
    pub golden_refresh: bool,
}

/// Deterministic canonical IPPP keyframe and golden-refresh policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GopPlanner {
    key_interval: u16,
    golden_interval: u8,
    last_frame_index: Option<u32>,
    last_key_frame_index: u32,
    golden_p_count: u8,
    transition_history: VecDeque<u64>,
}

impl GopPlanner {
    /// Creates a policy state with nonzero header-representable intervals.
    pub fn new(key_interval: u16, golden_interval: u8) -> Result<Self, EncodeError> {
        if key_interval == 0 {
            return Err(EncodeError::InvalidInput {
                element: "gop.key_interval",
            });
        }
        if golden_interval == 0 {
            return Err(EncodeError::InvalidInput {
                element: "gop.golden_interval",
            });
        }
        Ok(Self {
            key_interval,
            golden_interval,
            last_frame_index: None,
            last_key_frame_index: 0,
            golden_p_count: 0,
            transition_history: VecDeque::with_capacity(SCENE_HISTORY_LIMIT),
        })
    }

    /// Plans the next sequential frame. Frame zero has no transition SAD;
    /// every later frame supplies full-frame luma SAD against its predecessor.
    pub fn next(
        &mut self,
        frame_index: u32,
        transition_sad: Option<u64>,
    ) -> Result<FrameDecision, EncodeError> {
        let Some(last_frame_index) = self.last_frame_index else {
            if frame_index != 0 || transition_sad.is_some() {
                return Err(policy("gop.initial_frame"));
            }
            self.last_frame_index = Some(0);
            self.last_key_frame_index = 0;
            return Ok(FrameDecision {
                key: true,
                golden_refresh: true,
            });
        };
        let expected_index = last_frame_index
            .checked_add(1)
            .ok_or_else(|| policy("gop.frame_index_overflow"))?;
        if frame_index != expected_index {
            return Err(policy("gop.frame_index_sequence"));
        }
        let transition_sad = transition_sad.ok_or_else(|| policy("gop.transition_sad"))?;
        let key_distance = frame_index
            .checked_sub(self.last_key_frame_index)
            .ok_or_else(|| policy("gop.key_distance"))?;
        let periodic = key_distance == u32::from(self.key_interval);
        let scene_cut = self.is_scene_cut(transition_sad)?;
        let key = periodic || scene_cut;
        let golden_refresh = if key {
            self.last_key_frame_index = frame_index;
            self.golden_p_count = 0;
            self.transition_history.clear();
            true
        } else {
            self.transition_history.push_back(transition_sad);
            if self.transition_history.len() > SCENE_HISTORY_LIMIT {
                self.transition_history.pop_front();
            }
            self.golden_p_count = self
                .golden_p_count
                .checked_add(1)
                .ok_or_else(|| policy("gop.golden_count"))?;
            if self.golden_p_count == self.golden_interval {
                self.golden_p_count = 0;
                true
            } else {
                false
            }
        };
        self.last_frame_index = Some(frame_index);
        Ok(FrameDecision {
            key,
            golden_refresh,
        })
    }

    fn is_scene_cut(&self, transition_sad: u64) -> Result<bool, EncodeError> {
        if transition_sad == 0 || self.transition_history.len() < SCENE_HISTORY_MINIMUM {
            return Ok(false);
        }
        let history_sum = self
            .transition_history
            .iter()
            .try_fold(0_u128, |sum, &value| {
                sum.checked_add(u128::from(value))
                    .ok_or_else(|| policy("gop.scene_history_sum"))
            })?;
        let weighted_current = u128::from(transition_sad)
            .checked_mul(
                u128::try_from(self.transition_history.len())
                    .map_err(|_| policy("gop.scene_history_count"))?,
            )
            .ok_or_else(|| policy("gop.scene_current"))?;
        let threshold = history_sum
            .checked_mul(3)
            .ok_or_else(|| policy("gop.scene_threshold"))?;
        Ok(weighted_current >= threshold)
    }
}

const fn policy(element: &'static str) -> EncodeError {
    EncodeError::Policy { element }
}

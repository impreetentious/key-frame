use std::sync::OnceLock;

use kf_spec::V1_ASSETS;

use crate::{EncodeError, declared_qp_max};

const Q16: i64 = 1 << 16;

struct Tables {
    first_qp: [u8; 32],
    window_frames: u8,
    bucket_multiple: u8,
    maximum_qp_step: u8,
    complexity_ewma_shift: u8,
}

/// Encoder rate policy. Constant QP is the correctness baseline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateControl {
    /// One QP for every frame.
    ConstantQp(u8),
    /// Single-pass ABR targeting `bitrate_bps` bits per second.
    Abr { bitrate_bps: u32 },
}

impl RateControl {
    /// Accepts QP in `0..=63`.
    pub fn constant_qp(qp: u8) -> Result<Self, EncodeError> {
        // The declared ceiling, not a copy of it. An encoder that accepted a
        // quantizer the format does not carry would produce a stream both
        // decoders refuse, and the first sign of it would be a refusal rather
        // than a message about a quantizer.
        if qp > declared_qp_max() {
            return Err(EncodeError::InvalidInput {
                element: "frame.qp",
            });
        }
        Ok(Self::ConstantQp(qp))
    }

    /// Validates that a controller can be constructed for this rate and fps.
    pub fn abr(bitrate_bps: u32, fps_num: u16, fps_den: u16) -> Result<Self, EncodeError> {
        let _controller = RateController::new(bitrate_bps, fps_num, fps_den)?;
        Ok(Self::Abr { bitrate_bps })
    }
}

/// Q16.16 leaky-bucket ABR controller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateController {
    budget_q16: i64,
    capacity_q16: i64,
    fill_q16: i64,
    qp: u8,
    complexity: u64,
    /// The most recent frame's transition cost, kept so the controller can ask
    /// how this frame compares with the recent past rather than only how full
    /// the bucket is.
    last_sad: u64,
    step: u8,
    ewma_shift: u8,
}

impl RateController {
    /// Builds a controller for a strictly positive bitrate and frame rate.
    /// Fill starts at half capacity so the first frame's QP sits in the
    /// middle third rather than immediately starving the bucket.
    pub fn new(bitrate_bps: u32, fps_num: u16, fps_den: u16) -> Result<Self, EncodeError> {
        if bitrate_bps == 0 || fps_num == 0 || fps_den == 0 {
            return Err(EncodeError::InvalidInput {
                element: "rate.bitrate",
            });
        }
        let tables = tables();
        let budget_bits =
            u64::from(bitrate_bps).saturating_mul(u64::from(fps_den)) / u64::from(fps_num);
        if budget_bits == 0 {
            return Err(EncodeError::InvalidInput {
                element: "rate.budget",
            });
        }
        let capacity_bits = u64::from(tables.bucket_multiple)
            .saturating_mul(u64::from(tables.window_frames))
            .saturating_mul(budget_bits);
        let budget_q16 = q16_from_bits(budget_bits);
        let capacity_q16 = q16_from_bits(capacity_bits);
        let qp = initial_qp(budget_bits, &tables.first_qp);
        Ok(Self {
            budget_q16,
            capacity_q16,
            fill_q16: capacity_q16 / 2,
            qp,
            complexity: 0,
            last_sad: 0,
            step: tables.maximum_qp_step,
            ewma_shift: tables.complexity_ewma_shift,
        })
    }

    /// QP that the next frame should be encoded at.
    #[must_use]
    pub const fn qp(&self) -> u8 {
        self.qp
    }

    /// Bucket fill in Q16.16 bits.
    #[must_use]
    pub const fn fill_q16(&self) -> i64 {
        self.fill_q16
    }

    /// Bucket capacity in Q16.16 bits.
    #[must_use]
    pub const fn capacity_q16(&self) -> i64 {
        self.capacity_q16
    }

    /// Bias the bucket thresholds are shifted by, in Q16.16 bits.
    ///
    /// Positive means "act as though the bucket were fuller than it is".
    #[must_use]
    pub const fn complexity_bias_q16(&self) -> i64 {
        complexity_bias(self.last_sad, self.complexity, self.capacity_q16)
    }

    /// Updates the complexity EWMA with saturating arithmetic.
    pub fn observe_complexity(&mut self, sad: u64) {
        self.last_sad = sad;
        if sad >= self.complexity {
            self.complexity = self
                .complexity
                .saturating_add((sad - self.complexity) >> self.ewma_shift);
        } else {
            self.complexity = self
                .complexity
                .saturating_sub((self.complexity - sad) >> self.ewma_shift);
        }
    }

    /// Applies one frame's emitted bit count and steps QP for the next frame.
    pub fn commit_frame_bits(&mut self, bits: u64) {
        let delta = q16_from_bits(bits).saturating_sub(self.budget_q16);
        self.fill_q16 = self
            .fill_q16
            .saturating_add(delta)
            .clamp(0, self.capacity_q16);
        self.qp = step_qp(
            self.fill_q16,
            self.capacity_q16,
            self.qp,
            self.step,
            self.complexity_bias_q16(),
        );
    }
}

/// How far the thresholds move for a frame that is not of average difficulty.
///
/// Bucket fullness is a report on what already happened. On its own it makes the
/// controller strictly reactive: it raises QP only after the overspend has been
/// paid for, which on content that changes difficulty costs a frame or two of
/// wrong quantizer every time the scene turns. The complexity average is what
/// lets it see the turn coming, and it was being computed and discarded.
///
/// The rule is deliberately coarse. A frame more than a quarter harder than the
/// running average shifts the thresholds as if the bucket were a sixteenth
/// fuller, so QP rises sooner; a frame more than a quarter easier shifts them
/// the other way. Anything in between changes nothing. A finer response would be
/// a prediction of the next frame's cost, which single-pass control cannot make
/// honestly, and an aggressive one would chase noise — a sixteenth of capacity
/// against bands at thirds can move the decision one band early but never
/// override it.
///
/// The sixteenth is measured rather than chosen. Across twelve operating points
/// on the pinned corpus, mean absolute rate error was 3.28% with no complexity
/// term, 2.81% at a twelfth, 2.21% at a sixteenth, and 3.14% at a
/// twenty-fourth; worst-case error fell from 6.74% to 5.31%. That sweep
/// compared three encoder variants two of which no longer exist, so it stays in
/// ADR-0016 as the record of the decision rather than as a figure anything can
/// recheck. What the build does still check is the shipped configuration: the
/// campaign receipt carries six average-bitrate operating points, `rd_verify`
/// re-encodes them, and `docs/LIMITATIONS.md` publishes the envelope they
/// describe.
const fn complexity_bias(last_sad: u64, complexity: u64, capacity: i64) -> i64 {
    // No history yet: the first frames have nothing to be harder or easier than.
    if complexity == 0 {
        return 0;
    }
    let shift = capacity / 16;
    let quarter = complexity / 4;
    if last_sad > complexity.saturating_add(quarter) {
        shift
    } else if last_sad < complexity.saturating_sub(quarter) {
        -shift
    } else {
        0
    }
}

fn step_qp(fill: i64, capacity: i64, qp: u8, step: u8, bias: i64) -> u8 {
    let low = capacity / 3;
    let high = (2 * capacity) / 3;
    // Clamped back inside the bucket so a bias can never push the decision
    // outside the range fullness alone could have reached.
    let effective = fill.saturating_add(bias).clamp(0, capacity);
    if effective < low {
        qp.saturating_sub(step)
    } else if effective > high {
        qp.saturating_add(step).min(63)
    } else {
        qp
    }
}

fn initial_qp(budget_bits: u64, table: &[u8; 32]) -> u8 {
    let index = (u64::BITS - 1 - budget_bits.leading_zeros()).min(31);
    table[usize::try_from(index).expect("invariant: log2 index fits usize")]
}

fn q16_from_bits(bits: u64) -> i64 {
    i64::try_from(bits).unwrap_or(i64::MAX).saturating_mul(Q16)
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let asset = V1_ASSETS
            .iter()
            .find(|asset| asset.name == "quant.toml")
            .expect("invariant: kf-spec exposes quant.toml");
        Tables {
            first_qp: parse_u8_array(asset.contents, "first_qp = ["),
            window_frames: parse_u8_scalar(asset.contents, "window_frames = "),
            bucket_multiple: parse_u8_scalar(asset.contents, "bucket_multiple = "),
            maximum_qp_step: parse_u8_scalar(asset.contents, "maximum_qp_step = "),
            complexity_ewma_shift: parse_u8_scalar(asset.contents, "complexity_ewma_shift = "),
        }
    })
}

fn parse_u8_scalar(contents: &str, prefix: &str) -> u8 {
    contents
        .lines()
        .find_map(|line| line.strip_prefix(prefix)?.parse().ok())
        .expect("invariant: checked rate-control scalar exists")
}

fn parse_u8_array(contents: &str, prefix: &str) -> [u8; 32] {
    let line = contents
        .lines()
        .find(|line| line.starts_with(prefix))
        .expect("invariant: checked first_qp array exists");
    let body = line
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(']'))
        .expect("invariant: checked first_qp array has brackets");
    let values: Vec<u8> = body
        .split(',')
        .map(|value| {
            value
                .trim()
                .parse::<u8>()
                .expect("invariant: first_qp entry is u8")
        })
        .collect();
    values
        .try_into()
        .expect("invariant: first_qp has 32 entries")
}

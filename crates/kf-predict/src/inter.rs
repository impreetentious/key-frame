use std::sync::OnceLock;

use kf_frame::Plane;
use kf_spec::V1_ASSETS;

use crate::PredictError;

/// The motion-compensation constants, taken from the frozen asset rather than
/// restated here.
///
/// Every field below decides reconstructed pixels, so a copy of it in this file
/// would be a second definition of the format competing with the normative one.
/// The asset is the definition; this struct is a parse of it.
struct McConstants {
    taps: Vec<i32>,
    scale: i32,
    two_stage_rounding: i32,
    two_stage_shift: u32,
    edge_extension: i64,
    fullpel_limit: i32,
    luma_phase_denominator: i32,
    chroma_phase_denominator: i32,
}

fn mc() -> &'static McConstants {
    static CONSTANTS: OnceLock<McConstants> = OnceLock::new();
    CONSTANTS.get_or_init(|| {
        let contents = V1_ASSETS
            .iter()
            .find(|asset| asset.name == "mc.toml")
            .expect("invariant: kf-spec exposes mc.toml")
            .contents;
        McConstants {
            taps: parse_i32_array(contents, "filter_taps = ["),
            scale: parse_i32(contents, "filter_denominator"),
            two_stage_rounding: parse_i32(contents, "two_stage_rounding"),
            two_stage_shift: u32::try_from(parse_i32(contents, "two_stage_shift"))
                .expect("invariant: checked stage shift is not negative"),
            edge_extension: i64::from(parse_i32(contents, "edge_extension_pixels")),
            fullpel_limit: parse_i32(contents, "fullpel_search_max"),
            luma_phase_denominator: parse_i32(contents, "phase_denominator"),
            chroma_phase_denominator: parse_i32(contents, "chroma_phase_denominator"),
        }
    })
}

fn parse_i32(contents: &str, key: &str) -> i32 {
    let prefix = format!("{key} = ");
    contents
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .expect("invariant: checked motion asset declares every scalar")
        .trim()
        .parse::<i32>()
        .expect("invariant: checked motion scalar is an integer")
}

fn parse_i32_array(contents: &str, prefix: &str) -> Vec<i32> {
    contents
        .lines()
        .find(|line| line.starts_with(prefix))
        .and_then(|line| line.strip_prefix(prefix))
        .and_then(|body| body.strip_suffix(']'))
        .expect("invariant: checked motion asset has a bracketed filter")
        .split(',')
        .map(|value| {
            value
                .trim()
                .parse::<i32>()
                .expect("invariant: checked filter tap is an integer")
        })
        .collect()
}

/// A motion vector measured in quarter-luma-pixel units on every plane.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MotionVector {
    pub x_q4: i32,
    pub y_q4: i32,
}

/// The sampling scale of the reference plane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaneScale {
    /// One plane sample per luma sample; an MV unit is one quarter sample.
    Luma,
    /// 4:2:0 chroma; an MV unit is one eighth sample.
    Chroma420,
}

impl PlaneScale {
    fn phase_denominator(self) -> i32 {
        match self {
            Self::Luma => mc().luma_phase_denominator,
            Self::Chroma420 => mc().chroma_phase_denominator,
        }
    }
}

/// Clamps a motion vector so its complete interpolation support remains in the
/// 64-pixel virtual edge extension.
pub fn clamp_motion_vector(
    plane: &Plane,
    x: u32,
    y: u32,
    size: u32,
    motion_vector: MotionVector,
    scale: PlaneScale,
) -> Result<MotionVector, PredictError> {
    validate_block(plane, x, y, size)?;
    let denominator = scale.phase_denominator();
    Ok(MotionVector {
        x_q4: clamp_component(
            motion_vector.x_q4,
            i64::from(x),
            i64::from(size),
            i64::from(plane.width()),
            denominator,
        ),
        y_q4: clamp_component(
            motion_vector.y_q4,
            i64::from(y),
            i64::from(size),
            i64::from(plane.height()),
            denominator,
        ),
    })
}

/// Produces a square inter predictor using the normative horizontal-then-
/// vertical six-tap path. Out-of-frame samples use the virtual replicated edge.
pub fn predict_inter(
    plane: &Plane,
    x: u32,
    y: u32,
    size: u32,
    motion_vector: MotionVector,
    scale: PlaneScale,
) -> Result<Vec<u8>, PredictError> {
    let motion_vector = clamp_motion_vector(plane, x, y, size, motion_vector, scale)?;
    let denominator = scale.phase_denominator();
    let x_base = i64::from(x) + i64::from(motion_vector.x_q4.div_euclid(denominator));
    let y_base = i64::from(y) + i64::from(motion_vector.y_q4.div_euclid(denominator));
    let x_phase = motion_vector.x_q4.rem_euclid(denominator);
    let y_phase = motion_vector.y_q4.rem_euclid(denominator);
    let output_len = usize::try_from(u64::from(size) * u64::from(size))
        .map_err(|_| PredictError::InvalidSize { size })?;
    let mut output = Vec::with_capacity(output_len);

    for block_y in 0..size {
        let source_y = y_base + i64::from(block_y);
        for block_x in 0..size {
            let source_x = x_base + i64::from(block_x);
            let scaled = if y_phase == 0 {
                horizontal_scaled(plane, source_x, source_y, x_phase, denominator) * mc().scale
            } else {
                vertical_scaled(plane, source_x, source_y, x_phase, y_phase, denominator)
            };
            output.push(clip_sample(
                (scaled + mc().two_stage_rounding) >> mc().two_stage_shift,
            ));
        }
    }
    Ok(output)
}

fn validate_block(plane: &Plane, x: u32, y: u32, size: u32) -> Result<(), PredictError> {
    if !matches!(size, 4 | 8 | 16 | 32 | 64) {
        return Err(PredictError::InvalidSize { size });
    }
    let Some(end_x) = x.checked_add(size) else {
        return Err(PredictError::BlockOutOfBounds { x, y, size });
    };
    let Some(end_y) = y.checked_add(size) else {
        return Err(PredictError::BlockOutOfBounds { x, y, size });
    };
    if end_x > plane.width() || end_y > plane.height() {
        return Err(PredictError::BlockOutOfBounds { x, y, size });
    }
    Ok(())
}

fn clamp_component(requested: i32, block: i64, size: i64, extent: i64, denominator: i32) -> i32 {
    // A motion vector is in quarter-luma units on every plane, so the declared
    // full-pixel search bound converts through the luma denominator even when
    // this call is interpolating chroma. Using the plane's own denominator here
    // would quietly double the legal range on chroma.
    let limit = mc().fullpel_limit * mc().luma_phase_denominator;
    let mut value = requested.clamp(-limit, limit);
    while !component_is_legal(value, block, size, extent, denominator) {
        value -= value.signum();
    }
    value
}

fn component_is_legal(value: i32, block: i64, size: i64, extent: i64, denominator: i32) -> bool {
    let integer = i64::from(value.div_euclid(denominator));
    let fractional = value.rem_euclid(denominator) != 0;
    let filter_before = i64::from(fractional) * 2;
    let filter_after = i64::from(fractional) * 3;
    let first = block + integer - filter_before;
    let last = block + size - 1 + integer + filter_after;
    first >= -mc().edge_extension && last <= extent - 1 + mc().edge_extension
}

fn vertical_scaled(
    plane: &Plane,
    x: i64,
    y: i64,
    x_phase: i32,
    y_phase: i32,
    denominator: i32,
) -> i32 {
    let half = mc()
        .taps
        .iter()
        .enumerate()
        .map(|(tap_index, &tap)| {
            let tap_y = y + i64::try_from(tap_index).expect("six taps fit i64") - 2;
            tap * horizontal_scaled(plane, x, tap_y, x_phase, denominator)
        })
        .sum();
    let integer = horizontal_scaled(plane, x, y, x_phase, denominator) * mc().scale;
    let next = horizontal_scaled(plane, x, y + 1, x_phase, denominator) * mc().scale;
    blend_phase(integer, half, next, y_phase, denominator)
}

fn horizontal_scaled(plane: &Plane, x: i64, y: i64, phase: i32, denominator: i32) -> i32 {
    let integer = i32::from(extended_sample(plane, x, y)) * mc().scale;
    if phase == 0 {
        return integer;
    }
    let half = mc()
        .taps
        .iter()
        .enumerate()
        .map(|(tap_index, &tap)| {
            let tap_x = x + i64::try_from(tap_index).expect("six taps fit i64") - 2;
            tap * i32::from(extended_sample(plane, tap_x, y))
        })
        .sum();
    let next = i32::from(extended_sample(plane, x + 1, y)) * mc().scale;
    blend_phase(integer, half, next, phase, denominator)
}

fn blend_phase(integer: i32, half: i32, next: i32, phase: i32, denominator: i32) -> i32 {
    let half_phase = denominator / 2;
    if phase == half_phase {
        half
    } else if phase < half_phase {
        rounded_blend(integer, half, phase, half_phase)
    } else {
        rounded_blend(half, next, phase - half_phase, half_phase)
    }
}

fn rounded_blend(left: i32, right: i32, right_weight: i32, denominator: i32) -> i32 {
    let numerator = left * (denominator - right_weight) + right * right_weight;
    (numerator + denominator / 2).div_euclid(denominator)
}

fn extended_sample(plane: &Plane, x: i64, y: i64) -> u8 {
    let clamped_x = x.clamp(0, i64::from(plane.width()) - 1);
    let clamped_y = y.clamp(0, i64::from(plane.height()) - 1);
    plane
        .get(
            u32::try_from(clamped_x).expect("clamped x fits u32"),
            u32::try_from(clamped_y).expect("clamped y fits u32"),
        )
        .expect("clamped coordinate is within the plane")
}

fn clip_sample(value: i32) -> u8 {
    u8::try_from(value.clamp(0, 255)).expect("clamped sample fits u8")
}

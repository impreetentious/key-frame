use std::sync::OnceLock;

use kf_frame::Plane;
use kf_predict::{MotionVector, PlaneScale, clamp_motion_vector, predict_inter};
use kf_spec::V1_ASSETS;

use crate::EncodeError;

/// The sub-pixel refinement step sizes, in quarter-pixel units.
///
/// The declared list is both the step sizes and, by its length, the declared
/// round count. Reading it here keeps the search this encoder performs the
/// search the asset describes.
fn subpel_steps() -> &'static [i32] {
    static STEPS: OnceLock<Vec<i32>> = OnceLock::new();
    STEPS.get_or_init(|| {
        let contents = V1_ASSETS
            .iter()
            .find(|asset| asset.name == "search.toml")
            .expect("invariant: kf-spec exposes search.toml")
            .contents;
        let steps: Vec<i32> = contents
            .lines()
            .find_map(|line| line.strip_prefix("subpel_steps_q4 = ["))
            .and_then(|body| body.strip_suffix(']'))
            .expect("invariant: checked search asset lists the sub-pixel steps")
            .split(',')
            .map(|value| {
                value
                    .trim()
                    .parse::<i32>()
                    .expect("invariant: checked sub-pixel step is an integer")
            })
            .collect();
        let declared_rounds: usize = contents
            .lines()
            .find_map(|line| line.strip_prefix("subpel_rounds = "))
            .expect("invariant: checked search asset declares the round count")
            .trim()
            .parse()
            .expect("invariant: checked round count is a count");
        assert_eq!(
            steps.len(),
            declared_rounds,
            "invariant: checked sub-pixel step list matches the declared round count"
        );
        steps
    })
}

/// The full-pixel and sub-pixel candidate offsets the search visits, in the
/// declared order.
///
/// Order is not cosmetic here: the declared tie-break is "zero then search
/// order", so two candidates of equal cost are separated by which one this list
/// reaches first. A reordered copy would silently pick different blocks.
fn pattern(key: &str) -> &'static [(i32, i32)] {
    fn parse(key: &str) -> Vec<(i32, i32)> {
        let contents = V1_ASSETS
            .iter()
            .find(|asset| asset.name == "search.toml")
            .expect("invariant: kf-spec exposes search.toml")
            .contents;
        let prefix = format!("{key} = [");
        let body = contents
            .lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .and_then(|line| line.strip_suffix("]"))
            .expect("invariant: checked search asset lists every candidate pattern");
        let mut offsets = Vec::new();
        for pair in body.split("],") {
            let pair = pair.trim().trim_start_matches('[').trim_end_matches(']');
            if pair.is_empty() {
                continue;
            }
            let (x, y) = pair
                .split_once(',')
                .expect("invariant: checked candidate offset is a pair");
            offsets.push((
                x.trim()
                    .parse()
                    .expect("invariant: checked offset component is an integer"),
                y.trim()
                    .parse()
                    .expect("invariant: checked offset component is an integer"),
            ));
        }
        offsets
    }
    static FULLPEL: OnceLock<Vec<(i32, i32)>> = OnceLock::new();
    static SUBPEL_PATTERN: OnceLock<Vec<(i32, i32)>> = OnceLock::new();
    match key {
        "fullpel_pattern" => FULLPEL.get_or_init(|| parse("fullpel_pattern")).as_slice(),
        "subpel_order" => SUBPEL_PATTERN
            .get_or_init(|| parse("subpel_order"))
            .as_slice(),
        other => panic!("invariant: unknown declared search pattern {other}"),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MotionSearchResult {
    pub(crate) motion_vector: MotionVector,
    pub(crate) sad: u64,
}

pub(crate) fn estimate_motion(
    source: &Plane,
    reference: &Plane,
    x: u32,
    y: u32,
    size: u32,
    predictor: MotionVector,
    subpel: bool,
) -> Result<MotionSearchResult, EncodeError> {
    let mut best = evaluate(source, reference, x, y, size, MotionVector::default())?;
    let center = MotionVector {
        x_q4: round_to_fullpel(predictor.x_q4),
        y_q4: round_to_fullpel(predictor.y_q4),
    };
    let mut diamond_center = evaluate(source, reference, x, y, size, center)?;
    consider(&mut best, diamond_center);

    loop {
        let ring_center = diamond_center.motion_vector;
        let mut ring_best = diamond_center;
        for &(delta_x, delta_y) in pattern("fullpel_pattern") {
            let candidate = MotionVector {
                x_q4: ring_center.x_q4.saturating_add(delta_x * 4),
                y_q4: ring_center.y_q4.saturating_add(delta_y * 4),
            };
            let evaluated = evaluate(source, reference, x, y, size, candidate)?;
            consider(&mut ring_best, evaluated);
            consider(&mut best, evaluated);
        }
        if ring_best.motion_vector == ring_center {
            break;
        }
        diamond_center = ring_best;
    }

    // The refinement rings are halves of a pixel and then quarters. Skipping
    // them leaves the result on an integer position, which is exactly the
    // comparison the subpel ablation wants; it is not a cheaper search for the
    // same answer.
    for &step in subpel_steps() {
        if !subpel {
            break;
        }
        let ring_center = best.motion_vector;
        for &(delta_x, delta_y) in pattern("subpel_order") {
            let candidate = MotionVector {
                x_q4: ring_center.x_q4.saturating_add(delta_x * step),
                y_q4: ring_center.y_q4.saturating_add(delta_y * step),
            };
            consider(
                &mut best,
                evaluate(source, reference, x, y, size, candidate)?,
            );
        }
    }
    Ok(best)
}

fn evaluate(
    source: &Plane,
    reference: &Plane,
    x: u32,
    y: u32,
    size: u32,
    requested: MotionVector,
) -> Result<MotionSearchResult, EncodeError> {
    let motion_vector = clamp_motion_vector(reference, x, y, size, requested, PlaneScale::Luma)
        .map_err(|_| reconstruction("motion.clamp"))?;
    let prediction = predict_inter(reference, x, y, size, motion_vector, PlaneScale::Luma)
        .map_err(|_| reconstruction("motion.predict"))?;
    let mut sad = 0_u64;
    let side = usize::try_from(size).map_err(|_| reconstruction("motion.size"))?;
    for row in 0..side {
        for column in 0..side {
            let source_sample = source
                .get(
                    x + u32::try_from(column).map_err(|_| reconstruction("motion.x"))?,
                    y + u32::try_from(row).map_err(|_| reconstruction("motion.y"))?,
                )
                .map_err(|_| reconstruction("motion.source"))?;
            sad = sad.saturating_add(u64::from(
                source_sample.abs_diff(prediction[row * side + column]),
            ));
        }
    }
    Ok(MotionSearchResult { motion_vector, sad })
}

fn consider(best: &mut MotionSearchResult, candidate: MotionSearchResult) {
    if candidate.sad < best.sad {
        *best = candidate;
    }
}

fn round_to_fullpel(value_q4: i32) -> i32 {
    if value_q4 >= 0 {
        value_q4.saturating_add(2).div_euclid(4) * 4
    } else {
        -value_q4.saturating_abs().saturating_add(2).div_euclid(4) * 4
    }
}

const fn reconstruction(element: &'static str) -> EncodeError {
    EncodeError::Reconstruction { element }
}

#[cfg(test)]
mod tests {
    use kf_frame::Plane;
    use kf_predict::{MotionVector, PlaneScale, predict_inter};

    use super::{estimate_motion, round_to_fullpel};

    fn noise_plane() -> Plane {
        let mut plane = Plane::filled(64, 64, 0).unwrap();
        let mut state = 0x1234_5678_u32;
        for sample in plane.data_mut() {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *sample = state.to_le_bytes()[0];
        }
        plane
    }

    #[test]
    fn fullpel_diamond_finds_an_exact_shift() {
        let reference = noise_plane();
        let mut source = reference.clone();
        let shifted = predict_inter(
            &reference,
            16,
            16,
            8,
            MotionVector { x_q4: 12, y_q4: -8 },
            PlaneScale::Luma,
        )
        .unwrap();
        for row in 0..8 {
            for column in 0..8 {
                source
                    .set(16 + column, 16 + row, shifted[(row * 8 + column) as usize])
                    .unwrap();
            }
        }
        let result = estimate_motion(
            &source,
            &reference,
            16,
            16,
            8,
            MotionVector { x_q4: 8, y_q4: -8 },
            true,
        )
        .unwrap();
        assert_eq!(result.motion_vector, MotionVector { x_q4: 12, y_q4: -8 });
        assert_eq!(result.sad, 0);
    }

    #[test]
    fn half_and_quarter_rounds_find_an_exact_fractional_shift() {
        let reference = noise_plane();
        let mut source = reference.clone();
        let shifted = predict_inter(
            &reference,
            24,
            24,
            8,
            MotionVector { x_q4: 5, y_q4: -3 },
            PlaneScale::Luma,
        )
        .unwrap();
        for row in 0..8 {
            for column in 0..8 {
                source
                    .set(24 + column, 24 + row, shifted[(row * 8 + column) as usize])
                    .unwrap();
            }
        }
        let result = estimate_motion(
            &source,
            &reference,
            24,
            24,
            8,
            MotionVector { x_q4: 7, y_q4: -5 },
            true,
        )
        .unwrap();
        assert_eq!(result.motion_vector, MotionVector { x_q4: 5, y_q4: -3 });
        assert_eq!(result.sad, 0);
    }

    #[test]
    fn zero_motion_wins_a_flat_tie() {
        let plane = Plane::filled(64, 64, 90).unwrap();
        let result = estimate_motion(
            &plane,
            &plane,
            8,
            8,
            8,
            MotionVector {
                x_q4: 40,
                y_q4: -28,
            },
            true,
        )
        .unwrap();
        assert_eq!(result.motion_vector, MotionVector::default());
    }

    #[test]
    fn predictor_center_rounds_nearest_with_away_ties() {
        assert_eq!(round_to_fullpel(1), 0);
        assert_eq!(round_to_fullpel(2), 4);
        assert_eq!(round_to_fullpel(6), 8);
        assert_eq!(round_to_fullpel(-1), 0);
        assert_eq!(round_to_fullpel(-2), -4);
        assert_eq!(round_to_fullpel(-6), -8);
    }
}

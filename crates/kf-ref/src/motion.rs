use std::sync::OnceLock;

use kf_frame::Plane;
use kf_spec::V1_ASSETS;

use crate::ReferenceError;

/// Motion-compensation constants read from the frozen asset.
///
/// This decoder agrees with the production one only by way of the
/// specification, so it reads the same declarations rather than repeating their
/// values. The reader below is this crate's own: independence means not sharing
/// the implementation, not refusing to read the same normative bytes.
struct RefMc {
    taps: Vec<i32>,
    scale: i32,
    rounding: i32,
    shift: u32,
    edge: i64,
    fullpel_limit: i32,
    luma_denominator: i32,
    chroma_denominator: i32,
}

fn mc() -> &'static RefMc {
    static MC: OnceLock<RefMc> = OnceLock::new();
    MC.get_or_init(|| {
        let contents = V1_ASSETS
            .iter()
            .find(|asset| asset.name == "mc.toml")
            .expect("invariant: kf-spec exposes mc.toml")
            .contents;
        let number = |key: &str| -> i32 {
            let prefix = format!("{key} = ");
            contents
                .lines()
                .find_map(|line| line.strip_prefix(&prefix))
                .expect("invariant: checked motion asset declares every scalar")
                .trim()
                .parse::<i32>()
                .expect("invariant: checked motion scalar is an integer")
        };
        let taps = contents
            .lines()
            .find_map(|line| line.strip_prefix("filter_taps = ["))
            .and_then(|body| body.strip_suffix(']'))
            .expect("invariant: checked motion asset lists the filter taps")
            .split(',')
            .map(|value| {
                value
                    .trim()
                    .parse::<i32>()
                    .expect("invariant: checked filter tap is an integer")
            })
            .collect();
        RefMc {
            taps,
            scale: number("filter_denominator"),
            rounding: number("two_stage_rounding"),
            shift: u32::try_from(number("two_stage_shift"))
                .expect("invariant: checked stage shift is not negative"),
            edge: i64::from(number("edge_extension_pixels")),
            fullpel_limit: number("fullpel_search_max"),
            luma_denominator: number("phase_denominator"),
            chroma_denominator: number("chroma_phase_denominator"),
        }
    })
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RefMotionVector {
    pub(crate) x_q4: i32,
    pub(crate) y_q4: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RefReference {
    Last,
    Golden,
}

#[derive(Clone)]
pub(crate) struct RefMotionField {
    width: u32,
    height: u32,
    cells_per_row: u32,
    cells: Vec<Option<(RefReference, RefMotionVector)>>,
}

impl RefMotionField {
    pub(crate) fn new(width: u32, height: u32) -> Result<Self, ReferenceError> {
        let cells_per_row = width / 8;
        let count = usize::try_from(u64::from(cells_per_row) * u64::from(height / 8))
            .map_err(|_| ReferenceError::new(0, "motion_field.size"))?;
        Ok(Self {
            width,
            height,
            cells_per_row,
            cells: vec![None; count],
        })
    }

    pub(crate) fn record_intra(&mut self, x: u32, y: u32, size: u32) {
        self.fill(x, y, size, None);
    }

    pub(crate) fn record_inter(
        &mut self,
        x: u32,
        y: u32,
        size: u32,
        reference: RefReference,
        motion_vector: RefMotionVector,
    ) {
        self.fill(x, y, size, Some((reference, motion_vector)));
    }

    pub(crate) fn predictor(
        &self,
        x: u32,
        y: u32,
        size: u32,
        reference: RefReference,
    ) -> RefMotionVector {
        let left = self.matching(i64::from(x) - 1, i64::from(y), reference);
        let above = self.matching(i64::from(x), i64::from(y) - 1, reference);
        let above_right =
            self.matching(i64::from(x) + i64::from(size), i64::from(y) - 1, reference);
        let above_left = self.matching(i64::from(x) - 1, i64::from(y) - 1, reference);
        let third = above_right.or(above_left).unwrap_or_default();
        let left = left.unwrap_or_default();
        let above = above.unwrap_or_default();
        RefMotionVector {
            x_q4: median(left.x_q4, above.x_q4, third.x_q4),
            y_q4: median(left.y_q4, above.y_q4, third.y_q4),
        }
    }

    fn fill(&mut self, x: u32, y: u32, size: u32, value: Option<(RefReference, RefMotionVector)>) {
        for cell_y in y / 8..(y + size) / 8 {
            for cell_x in x / 8..(x + size) / 8 {
                let index = self.index(cell_x, cell_y);
                self.cells[index] = value;
            }
        }
    }

    fn matching(&self, x: i64, y: i64, reference: RefReference) -> Option<RefMotionVector> {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return None;
        }
        let cell_x = u32::try_from(x).ok()? / 8;
        let cell_y = u32::try_from(y).ok()? / 8;
        match self.cells[self.index(cell_x, cell_y)] {
            Some((candidate_reference, motion_vector)) if candidate_reference == reference => {
                Some(motion_vector)
            }
            _ => None,
        }
    }

    fn index(&self, cell_x: u32, cell_y: u32) -> usize {
        usize::try_from(u64::from(cell_y) * u64::from(self.cells_per_row) + u64::from(cell_x))
            .expect("validated reference motion-field index")
    }
}

pub(crate) fn clamp_motion(
    plane: &Plane,
    x: u32,
    y: u32,
    size: u32,
    motion: RefMotionVector,
    chroma: bool,
) -> RefMotionVector {
    let denominator = if chroma {
        mc().chroma_denominator
    } else {
        mc().luma_denominator
    };
    RefMotionVector {
        x_q4: clamp_component(
            motion.x_q4,
            i64::from(x),
            i64::from(size),
            i64::from(plane.width()),
            denominator,
        ),
        y_q4: clamp_component(
            motion.y_q4,
            i64::from(y),
            i64::from(size),
            i64::from(plane.height()),
            denominator,
        ),
    }
}

pub(crate) fn predict_inter(
    plane: &Plane,
    x: u32,
    y: u32,
    size: u32,
    motion: RefMotionVector,
    chroma: bool,
) -> Vec<u8> {
    let denominator = if chroma {
        mc().chroma_denominator
    } else {
        mc().luma_denominator
    };
    let x_base = i64::from(x) + i64::from(motion.x_q4.div_euclid(denominator));
    let y_base = i64::from(y) + i64::from(motion.y_q4.div_euclid(denominator));
    let x_phase = motion.x_q4.rem_euclid(denominator);
    let y_phase = motion.y_q4.rem_euclid(denominator);
    let mut output = Vec::with_capacity(usize::try_from(size * size).unwrap());
    for row in 0..size {
        for column in 0..size {
            let source_x = x_base + i64::from(column);
            let source_y = y_base + i64::from(row);
            let scaled = if y_phase == 0 {
                horizontal(plane, source_x, source_y, x_phase, denominator) * mc().scale
            } else {
                let half = mc()
                    .taps
                    .iter()
                    .enumerate()
                    .map(|(index, &tap)| {
                        tap * horizontal(
                            plane,
                            source_x,
                            source_y + i64::try_from(index).unwrap() - 2,
                            x_phase,
                            denominator,
                        )
                    })
                    .sum();
                phase(
                    horizontal(plane, source_x, source_y, x_phase, denominator) * mc().scale,
                    half,
                    horizontal(plane, source_x, source_y + 1, x_phase, denominator) * mc().scale,
                    y_phase,
                    denominator,
                )
            };
            output.push(
                u8::try_from(((scaled + mc().rounding) >> mc().shift).clamp(0, 255)).unwrap(),
            );
        }
    }
    output
}

fn horizontal(plane: &Plane, x: i64, y: i64, phase_index: i32, denominator: i32) -> i32 {
    let integer = i32::from(sample(plane, x, y)) * mc().scale;
    if phase_index == 0 {
        return integer;
    }
    let half = mc()
        .taps
        .iter()
        .enumerate()
        .map(|(index, &tap)| {
            tap * i32::from(sample(plane, x + i64::try_from(index).unwrap() - 2, y))
        })
        .sum();
    phase(
        integer,
        half,
        i32::from(sample(plane, x + 1, y)) * mc().scale,
        phase_index,
        denominator,
    )
}

fn phase(integer: i32, half: i32, next: i32, index: i32, denominator: i32) -> i32 {
    let midpoint = denominator / 2;
    if index == midpoint {
        half
    } else if index < midpoint {
        blend(integer, half, index, midpoint)
    } else {
        blend(half, next, index - midpoint, midpoint)
    }
}

fn blend(left: i32, right: i32, right_weight: i32, denominator: i32) -> i32 {
    (left * (denominator - right_weight) + right * right_weight + denominator / 2)
        .div_euclid(denominator)
}

fn sample(plane: &Plane, x: i64, y: i64) -> u8 {
    let x = u32::try_from(x.clamp(0, i64::from(plane.width()) - 1)).unwrap();
    let y = u32::try_from(y.clamp(0, i64::from(plane.height()) - 1)).unwrap();
    plane.get(x, y).expect("clamped reference coordinate")
}

fn clamp_component(requested: i32, block: i64, size: i64, extent: i64, denominator: i32) -> i32 {
    // Quarter-luma units on every plane, so the bound always converts
    // through the luma denominator.
    let limit = mc().fullpel_limit * mc().luma_denominator;
    let mut value = requested.clamp(-limit, limit);
    while !legal(value, block, size, extent, denominator) {
        value -= value.signum();
    }
    value
}

fn legal(value: i32, block: i64, size: i64, extent: i64, denominator: i32) -> bool {
    let integer = i64::from(value.div_euclid(denominator));
    let fractional = value.rem_euclid(denominator) != 0;
    let first = block + integer - i64::from(fractional) * 2;
    let last = block + size - 1 + integer + i64::from(fractional) * 3;
    first >= -mc().edge && last <= extent - 1 + mc().edge
}

fn median(first: i32, second: i32, third: i32) -> i32 {
    let low = first.min(second);
    let high = first.max(second);
    third.clamp(low, high)
}

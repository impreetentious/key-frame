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

#[cfg(test)]
mod declared_predictor {
    //! The same declaration, read again by this decoder's own parser.
    //!
    //! `spec/v1/mc.toml` names the predictor's candidates and the rules that go
    //! with them. The production crate has a test that holds its implementation
    //! to that declaration; this one holds this implementation to it. Two
    //! decoders that both read the specification cannot agree except by
    //! agreeing with the specification, which is the whole point of there being
    //! two.
    //!
    //! Isolating one candidate under a median takes a bare majority carrying
    //! the same marker: a median ignores its extremes, so a field that gives
    //! every candidate a distinct vector still passes when one of them is read
    //! from the wrong cell. Every cell outside that majority holds an intra
    //! block, so a misread candidate finds a neighbour that cannot contribute
    //! rather than an accident that agrees.

    use kf_spec::V1_ASSETS;

    use super::{RefMotionField, RefMotionVector, RefReference};

    const X: u32 = 16;
    const Y: u32 = 16;
    const SIZE: u32 = 8;
    const NEIGHBOURHOOD: [&str; 4] = ["left", "above", "above_right", "above_left"];
    const MARKER: RefMotionVector = RefMotionVector {
        x_q4: 44,
        y_q4: -36,
    };
    const DECOY: RefMotionVector = RefMotionVector {
        x_q4: -100,
        y_q4: 100,
    };

    fn section() -> &'static str {
        V1_ASSETS
            .iter()
            .find(|asset| asset.name == "mc.toml")
            .expect("kf-spec exposes mc.toml")
            .contents
            .split_once("[mv_predictor]")
            .expect("mc.toml declares an [mv_predictor] section")
            .1
    }

    fn declaration(key: &str) -> String {
        let prefix = format!("{key} = ");
        section()
            .lines()
            .take_while(|line| !line.trim_start().starts_with('['))
            .find_map(|line| line.trim().strip_prefix(&prefix))
            .unwrap_or_else(|| panic!("[mv_predictor] declares no `{key}`"))
            .trim()
            .to_owned()
    }

    fn declared_list(key: &str) -> Vec<String> {
        declaration(key)
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .map(|entry| entry.trim().trim_matches('"').to_owned())
            .filter(|entry| !entry.is_empty())
            .collect()
    }

    fn cell(name: &str) -> (u32, u32) {
        let (x, y): (i64, i64) = match name {
            "left" => (i64::from(X) - 1, i64::from(Y)),
            "above" => (i64::from(X), i64::from(Y) - 1),
            "above_right" => (i64::from(X) + i64::from(SIZE), i64::from(Y) - 1),
            "above_left" => (i64::from(X) - 1, i64::from(Y) - 1),
            other => panic!(
                "mc.toml names a predictor candidate this decoder's test cannot place: \
                 `{other}`"
            ),
        };
        assert!(x >= 0 && y >= 0, "a candidate position left the frame");
        (
            u32::try_from(x).unwrap() / SIZE * SIZE,
            u32::try_from(y).unwrap() / SIZE * SIZE,
        )
    }

    fn candidate_cells(name: &str) -> ((u32, u32), Option<(u32, u32)>) {
        match name.split_once("_else_") {
            Some((first, second)) => (cell(first), Some(cell(second))),
            None => (cell(name), None),
        }
    }

    fn declared_unavailable() -> RefMotionVector {
        let components: Vec<i32> = declared_list("unavailable")
            .iter()
            .map(|entry| entry.parse::<i32>().expect("declared as integers"))
            .collect();
        assert_eq!(components.len(), 2, "a motion vector has two components");
        RefMotionVector {
            x_q4: components[0],
            y_q4: components[1],
        }
    }

    /// `moving` cells carry their vector; every other neighbour is intra.
    fn neighbourhood(moving: &[((u32, u32), RefMotionVector, RefReference)]) -> RefMotionField {
        let mut field = RefMotionField::new(64, 64).unwrap();
        let mut placed: Vec<(u32, u32)> = Vec::new();
        for (origin, vector, reference) in moving {
            if placed.contains(origin) {
                continue;
            }
            field.record_inter(origin.0, origin.1, SIZE, *reference, *vector);
            placed.push(*origin);
        }
        for name in NEIGHBOURHOOD {
            let origin = cell(name);
            if placed.contains(&origin) {
                continue;
            }
            field.record_intra(origin.0, origin.1, SIZE);
            placed.push(origin);
        }
        field
    }

    fn carrying(
        live: &[(u32, u32)],
        vector: RefMotionVector,
        reference: RefReference,
    ) -> RefMotionField {
        let moving: Vec<((u32, u32), RefMotionVector, RefReference)> = live
            .iter()
            .map(|origin| (*origin, vector, reference))
            .collect();
        neighbourhood(&moving)
    }

    fn majority(candidates: &[String], index: usize) -> Vec<(u32, u32)> {
        let needed = candidates.len() / 2 + 1;
        let mut chosen = vec![index];
        for other in 0..candidates.len() {
            if chosen.len() == needed {
                break;
            }
            if other != index {
                chosen.push(other);
            }
        }
        chosen
            .into_iter()
            .map(|slot| candidate_cells(&candidates[slot]).0)
            .collect()
    }

    #[test]
    fn the_combine_rule_is_the_one_this_test_derives_from() {
        assert_eq!(declaration("combine"), "\"componentwise_median\"");
        let candidates = declared_list("candidates");
        assert_eq!(candidates.len() % 2, 1);
        assert!(candidates.len() >= 3);
    }

    #[test]
    fn each_declared_candidate_is_read_from_the_cell_its_name_names() {
        let candidates = declared_list("candidates");
        let unavailable = declared_unavailable();
        for (index, name) in candidates.iter().enumerate() {
            let live = majority(&candidates, index);
            let field = carrying(&live, MARKER, RefReference::Last);
            assert_eq!(
                field.predictor(X, Y, SIZE, RefReference::Last),
                MARKER,
                "the `{name}` candidate was not read from the cell mc.toml names"
            );

            let primary = candidate_cells(name).0;
            let without: Vec<(u32, u32)> = live
                .iter()
                .copied()
                .filter(|origin| *origin != primary)
                .collect();
            let field = carrying(&without, MARKER, RefReference::Last);
            assert_eq!(
                field.predictor(X, Y, SIZE, RefReference::Last),
                unavailable,
                "dropping the `{name}` candidate left the prediction unchanged"
            );
        }
    }

    #[test]
    fn a_declared_fallback_candidate_falls_back_where_the_name_says() {
        let candidates = declared_list("candidates");
        let mut checked = 0;
        for (index, name) in candidates.iter().enumerate() {
            let (primary, fallback) = candidate_cells(name);
            let Some(fallback) = fallback else { continue };
            checked += 1;
            assert_ne!(primary, fallback);

            let live = majority(&candidates, index);
            let mut fallen_back: Vec<(u32, u32)> =
                live.iter().copied().filter(|o| *o != primary).collect();
            fallen_back.push(fallback);
            let field = carrying(&fallen_back, MARKER, RefReference::Last);
            assert_eq!(
                field.predictor(X, Y, SIZE, RefReference::Last),
                MARKER,
                "the `{name}` candidate did not fall back where its name says"
            );

            // With both cells available and disagreeing, the primary wins.
            // Without this the halves of an `a_else_b` name are interchangeable.
            let mut moving: Vec<((u32, u32), RefMotionVector, RefReference)> = live
                .iter()
                .map(|origin| (*origin, MARKER, RefReference::Last))
                .collect();
            moving.push((fallback, DECOY, RefReference::Last));
            let field = neighbourhood(&moving);
            assert_eq!(
                field.predictor(X, Y, SIZE, RefReference::Last),
                MARKER,
                "the `{name}` candidate preferred its fallback cell over its primary one"
            );
        }
        assert_eq!(checked, 1, "this decoder implements one fallback candidate");
    }

    #[test]
    fn an_unavailable_candidate_contributes_the_declared_vector() {
        let field = RefMotionField::new(64, 64).unwrap();
        assert_eq!(
            field.predictor(X, Y, SIZE, RefReference::Last),
            declared_unavailable()
        );
        let field = neighbourhood(&[]);
        assert_eq!(
            field.predictor(X, Y, SIZE, RefReference::Last),
            declared_unavailable()
        );
    }

    #[test]
    fn a_neighbour_on_the_other_reference_is_not_a_candidate() {
        assert_eq!(declaration("same_reference_required"), "true");
        let candidates = declared_list("candidates");
        let live: Vec<(u32, u32)> = candidates
            .iter()
            .map(|name| candidate_cells(name).0)
            .collect();
        let field = carrying(&live, MARKER, RefReference::Golden);
        assert_eq!(
            field.predictor(X, Y, SIZE, RefReference::Last),
            declared_unavailable(),
            "GOLDEN neighbours were counted while predicting a LAST block"
        );
        assert_eq!(
            field.predictor(X, Y, SIZE, RefReference::Golden),
            MARKER,
            "and the same neighbours were not counted while predicting a GOLDEN block"
        );
    }
}

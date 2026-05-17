//! The declared motion-vector predictor, compared against the implemented one.
//!
//! `spec/v1/mc.toml` names the predictor's candidates, says how they combine,
//! says what an unavailable one contributes, and says a neighbour coded against
//! a different reference does not count. Every one of those is decoder-normative:
//! two decoders that disagree about which neighbour is the third candidate
//! reconstruct different pictures from the same bytes.
//!
//! Until this test existed the declaration was the only copy of that rule
//! nothing compared against anything. Both decoders spelled the candidates out
//! in Rust, agreed with each other, and would have gone on agreeing while the
//! published specification named a different set.
//!
//! So the asset supplies the vocabulary and this file supplies only the
//! neighbourhood. A declared name the test cannot place is a failure rather
//! than a skip.
//!
//! Isolating one candidate under a median needs care. A median ignores its
//! extremes, so a field that simply gives each candidate a distinct vector
//! passes even when a candidate is read from the wrong cell — the wrong cell
//! reads as unavailable, the value that moves is an extreme, and the middle
//! one does not budge. Every test below therefore gives exactly a bare
//! majority of candidates the same marker: the median is the marker when they
//! are all read from the declared cells and the unavailable vector when any one
//! of them is not. Every cell the neighbourhood contains that is not part of
//! that majority holds an intra block, so a candidate read from somewhere the
//! declaration did not name finds a neighbour that cannot contribute rather
//! than an accident that agrees.

use kf_predict::{BlockMotion, MotionField, MotionVector, ReferenceSlot};
use kf_spec::V1_ASSETS;

/// The block under prediction. Every neighbour position is derived from it.
const X: u32 = 16;
const Y: u32 = 16;
const SIZE: u32 = 8;

/// Every cell this test controls. Anything outside it is unavailable by being
/// outside a freshly allocated field, so a predictor reading beyond this set
/// reads the unavailable vector and fails the majority.
const NEIGHBOURHOOD: [&str; 4] = ["left", "above", "above_right", "above_left"];

const MARKER: MotionVector = MotionVector {
    x_q4: 44,
    y_q4: -36,
};

/// Placed where a fallback candidate must *not* look while its primary is
/// available. It sits on the far side of the unavailable vector from the
/// marker, so a candidate that picks it up drags the median away from the
/// marker rather than landing beside it.
const DECOY: MotionVector = MotionVector {
    x_q4: -100,
    y_q4: 100,
};

fn mc_asset() -> &'static str {
    V1_ASSETS
        .iter()
        .find(|asset| asset.name == "mc.toml")
        .expect("kf-spec exposes mc.toml")
        .contents
}

/// The raw right-hand side of a `key = value` line inside `[mv_predictor]`.
fn declaration(key: &str) -> String {
    let section = mc_asset()
        .split_once("[mv_predictor]")
        .expect("mc.toml declares an [mv_predictor] section")
        .1;
    let prefix = format!("{key} = ");
    section
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

/// The 8×8 cell origin a named neighbour of the block under test lives in.
fn cell(name: &str) -> (u32, u32) {
    let position: (i64, i64) = match name {
        "left" => (i64::from(X) - 1, i64::from(Y)),
        "above" => (i64::from(X), i64::from(Y) - 1),
        "above_right" => (i64::from(X) + i64::from(SIZE), i64::from(Y) - 1),
        "above_left" => (i64::from(X) - 1, i64::from(Y) - 1),
        other => panic!(
            "mc.toml names a predictor candidate this test cannot place: `{other}`. \
             Teach it the position, or the declaration and the decoders have diverged."
        ),
    };
    assert!(
        position.0 >= 0 && position.1 >= 0,
        "a candidate position left the frame"
    );
    (
        u32::try_from(position.0).unwrap() / SIZE * SIZE,
        u32::try_from(position.1).unwrap() / SIZE * SIZE,
    )
}

/// A candidate's primary neighbour, and the one its name says to fall back to.
///
/// The fallback arm exists because one declared name carries one: the third
/// candidate is written `above_right_else_above_left`, and the `_else_` is the
/// rule rather than decoration.
fn candidate_cells(name: &str) -> ((u32, u32), Option<(u32, u32)>) {
    match name.split_once("_else_") {
        Some((first, second)) => (cell(first), Some(cell(second))),
        None => (cell(name), None),
    }
}

/// The whole neighbourhood, with `entries` as given and every other cell intra.
fn neighbourhood(entries: &[((u32, u32), BlockMotion)]) -> MotionField {
    let mut field = MotionField::new(64, 64).unwrap();
    let mut placed: Vec<(u32, u32)> = Vec::new();
    for (origin, motion) in entries {
        if placed.contains(origin) {
            continue;
        }
        field.record(origin.0, origin.1, SIZE, *motion).unwrap();
        placed.push(*origin);
    }
    for name in NEIGHBOURHOOD {
        let origin = cell(name);
        if placed.contains(&origin) {
            continue;
        }
        field
            .record(origin.0, origin.1, SIZE, BlockMotion::Intra)
            .unwrap();
        placed.push(origin);
    }
    field
}

/// The same neighbourhood written the common way: these cells carry `motion`.
fn all_carrying(live: &[(u32, u32)], motion: BlockMotion) -> MotionField {
    let entries: Vec<((u32, u32), BlockMotion)> =
        live.iter().map(|origin| (*origin, motion)).collect();
    neighbourhood(&entries)
}

fn moving(reference: ReferenceSlot, motion_vector: MotionVector) -> BlockMotion {
    BlockMotion::Inter {
        reference,
        motion_vector,
    }
}

fn declared_unavailable() -> MotionVector {
    let components: Vec<i32> = declared_list("unavailable")
        .iter()
        .map(|entry| entry.parse::<i32>().expect("declared as integers"))
        .collect();
    assert_eq!(components.len(), 2, "a motion vector has two components");
    MotionVector {
        x_q4: components[0],
        y_q4: components[1],
    }
}

/// The candidates that must carry the marker for the median to be the marker.
fn majority(candidates: &[String], index: usize) -> Vec<usize> {
    let mut chosen = vec![index];
    let needed = candidates.len() / 2 + 1;
    for other in 0..candidates.len() {
        if chosen.len() == needed {
            break;
        }
        if other != index {
            chosen.push(other);
        }
    }
    chosen
}

#[test]
fn the_combine_rule_is_the_one_these_tests_derive_from() {
    assert_eq!(
        declaration("combine"),
        "\"componentwise_median\"",
        "every expectation below is a componentwise median; a different combine \
         rule needs a different derivation, not a passing test"
    );
    let candidates = declared_list("candidates");
    assert_eq!(
        candidates.len() % 2,
        1,
        "a median needs an odd number of candidates"
    );
    assert!(
        candidates.len() >= 3,
        "a median over fewer than three candidates cannot isolate one"
    );
}

#[test]
fn each_declared_candidate_is_read_from_the_cell_its_name_names() {
    let candidates = declared_list("candidates");
    let unavailable = declared_unavailable();
    for (index, name) in candidates.iter().enumerate() {
        let live: Vec<(u32, u32)> = majority(&candidates, index)
            .into_iter()
            .map(|slot| candidate_cells(&candidates[slot]).0)
            .collect();
        let field = all_carrying(&live, moving(ReferenceSlot::Last, MARKER));
        assert_eq!(
            field.predictor(X, Y, SIZE, ReferenceSlot::Last).unwrap(),
            MARKER,
            "the `{name}` candidate was not read from the cell mc.toml names"
        );

        // And the same majority minus this candidate is not a majority, so the
        // assertion above is about this candidate rather than about the others.
        let without: Vec<(u32, u32)> = live
            .iter()
            .copied()
            .filter(|origin| *origin != candidate_cells(name).0)
            .collect();
        let field = all_carrying(&without, moving(ReferenceSlot::Last, MARKER));
        assert_eq!(
            field.predictor(X, Y, SIZE, ReferenceSlot::Last).unwrap(),
            unavailable,
            "dropping the `{name}` candidate left the prediction unchanged, so the \
             test above would pass without it"
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
        assert_ne!(
            primary, fallback,
            "a fallback that reads the same cell as the primary is not a fallback"
        );

        // The primary cell holds an intra block, so the only way this candidate
        // reaches the majority is by falling back exactly where its name says.
        let live: Vec<(u32, u32)> = majority(&candidates, index)
            .into_iter()
            .map(|slot| candidate_cells(&candidates[slot]).0)
            .collect();
        let mut fallen_back = live.clone();
        fallen_back.retain(|origin| *origin != primary);
        fallen_back.push(fallback);
        let field = all_carrying(&fallen_back, moving(ReferenceSlot::Last, MARKER));
        assert_eq!(
            field.predictor(X, Y, SIZE, ReferenceSlot::Last).unwrap(),
            MARKER,
            "the `{name}` candidate did not fall back to the cell its name names"
        );

        // Falling back where the name says is only half the rule. The other
        // half is the order: with both cells available and disagreeing, the
        // primary wins. Without this the two halves of an `a_else_b` name are
        // interchangeable, because a chain that visits both reaches a vector
        // either way and the tests above cannot tell which one it visited first.
        let mut entries: Vec<((u32, u32), BlockMotion)> = live
            .iter()
            .map(|origin| (*origin, moving(ReferenceSlot::Last, MARKER)))
            .collect();
        entries.push((fallback, moving(ReferenceSlot::Last, DECOY)));
        let field = neighbourhood(&entries);
        assert_eq!(
            field.predictor(X, Y, SIZE, ReferenceSlot::Last).unwrap(),
            MARKER,
            "the `{name}` candidate preferred its fallback cell over its primary one"
        );
    }
    assert_eq!(
        checked, 1,
        "mc.toml declares {checked} fallback candidate(s); the decoders implement one"
    );
}

#[test]
fn an_unavailable_candidate_contributes_the_declared_vector() {
    // An empty field makes every candidate unavailable at once, so the
    // predictor can only return the declared substitute.
    let field = MotionField::new(64, 64).unwrap();
    assert_eq!(
        field.predictor(X, Y, SIZE, ReferenceSlot::Last).unwrap(),
        declared_unavailable()
    );

    // An intra neighbourhood is the same answer by a different route: a block
    // that was coded without motion has no vector to lend.
    let field = all_carrying(&[], BlockMotion::Intra);
    assert_eq!(
        field.predictor(X, Y, SIZE, ReferenceSlot::Last).unwrap(),
        declared_unavailable()
    );
}

#[test]
fn a_neighbour_on_the_other_reference_is_not_a_candidate() {
    assert_eq!(
        declaration("same_reference_required"),
        "true",
        "this test asserts the declared rule; a declaration of false needs the \
         opposite test, not this one"
    );

    let candidates = declared_list("candidates");
    let live: Vec<(u32, u32)> = candidates
        .iter()
        .map(|name| candidate_cells(name).0)
        .collect();
    let field = all_carrying(&live, moving(ReferenceSlot::Golden, MARKER));
    assert_eq!(
        field.predictor(X, Y, SIZE, ReferenceSlot::Last).unwrap(),
        declared_unavailable(),
        "GOLDEN neighbours were counted while predicting a LAST block"
    );
    assert_eq!(
        field.predictor(X, Y, SIZE, ReferenceSlot::Golden).unwrap(),
        MARKER,
        "and the same neighbours were not counted while predicting a GOLDEN block"
    );
}

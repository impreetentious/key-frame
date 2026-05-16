//! The declared stage shifts and the implemented ones, compared.
//!
//! `spec/v1/transforms.toml` states the four stage shifts and the final clamp
//! window as normative text. Until this test existed, that text was the only
//! copy nothing compared against anything: the Rust carried the numbers as
//! literals, the Python oracle carried the same numbers as a second set of
//! literals, and the two agreed with each other rather than with the asset.
//! Editing `inverse_shift2` in the frozen specification would have left every
//! gate green while the normative document described a codec the build does
//! not implement.
//!
//! The asset writes three of the four as formulas over the block side, so this
//! test evaluates the declared formula rather than restating its result — a
//! table of expected shifts here would be a third copy of the literals and
//! would prove nothing.

use kf_spec::V1_ASSETS;
use kf_transform::TransformSize;

const SIZES: [TransformSize; 4] = [
    TransformSize::N4,
    TransformSize::N8,
    TransformSize::N16,
    TransformSize::N32,
];

fn transforms_asset() -> &'static str {
    V1_ASSETS
        .iter()
        .find(|asset| asset.name == "transforms.toml")
        .expect("invariant: kf-spec exposes transforms.toml")
        .contents
}

/// Returns the raw right-hand side of a top-level `key = value` declaration.
fn declaration(key: &str) -> String {
    let prefix = format!("{key} = ");
    transforms_asset()
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("transforms.toml declares no `{key}`"))
        .trim()
        .trim_matches('"')
        .to_string()
}

/// Evaluates the closed formula grammar the transform asset uses for shifts.
///
/// Exactly three shapes appear, and anything else is rejected rather than
/// guessed at: a bare integer, `log2(N)+K`, and `K-log2(N)`.
fn evaluate(formula: &str, log2_side: u8) -> u8 {
    let formula = formula.replace(' ', "");
    if let Ok(literal) = formula.parse::<u8>() {
        return literal;
    }
    if let Some(addend) = formula.strip_prefix("log2(N)+") {
        let addend: u8 = addend.parse().expect("declared addend is a small integer");
        return log2_side + addend;
    }
    if let Some((minuend, subtrahend)) = formula.split_once('-')
        && subtrahend == "log2(N)"
    {
        let minuend: u8 = minuend
            .parse()
            .expect("declared minuend is a small integer");
        return minuend - log2_side;
    }
    panic!("transforms.toml uses a shift formula this test cannot evaluate: {formula}");
}

#[test]
fn implemented_shifts_match_the_declared_formulas() {
    let forward1 = declaration("forward_shift1");
    let forward2 = declaration("forward_shift2");
    let inverse1 = declaration("inverse_shift1");
    let inverse2 = declaration("inverse_shift2");

    for size in SIZES {
        let side = size.side();
        let log2_side = size.log2_side();
        assert_eq!(
            1_usize << log2_side,
            side,
            "log2_side disagrees with side for the {side}-point transform"
        );
        assert_eq!(
            size.forward_first_shift(),
            evaluate(&forward1, log2_side),
            "forward_shift1 differs from `{forward1}` at size {side}"
        );
        assert_eq!(
            size.forward_second_shift(),
            evaluate(&forward2, log2_side),
            "forward_shift2 differs from `{forward2}` at size {side}"
        );
        assert_eq!(
            size.inverse_first_shift(),
            evaluate(&inverse1, log2_side),
            "inverse_shift1 differs from `{inverse1}` at size {side}"
        );
        assert_eq!(
            size.inverse_second_shift(),
            evaluate(&inverse2, log2_side),
            "inverse_shift2 differs from `{inverse2}` at size {side}"
        );
    }
}

/// The first inverse stage undoes the matrix scale, so the two declarations
/// that express that single fact have to stay equal to each other.
#[test]
fn first_inverse_shift_is_the_declared_coefficient_scale() {
    let scale_bits: u8 = declaration("coefficient_scale_bits")
        .parse()
        .expect("coefficient_scale_bits is an integer");
    for size in SIZES {
        assert_eq!(
            size.inverse_first_shift(),
            scale_bits,
            "the first inverse shift must undo exactly coefficient_scale_bits"
        );
    }
}

/// The final inverse stage clamps to the declared window, not to a window the
/// implementation chose for itself.
#[test]
fn final_clamp_window_matches_the_declaration() {
    let min: i32 = declaration("post_inverse_min")
        .parse()
        .expect("post_inverse_min is an integer");
    let max: i32 = declaration("post_inverse_max")
        .parse()
        .expect("post_inverse_max is an integer");
    assert_eq!(
        min,
        i32::from(i16::MIN),
        "declared post-inverse floor moved"
    );
    assert_eq!(
        max,
        i32::from(i16::MAX),
        "declared post-inverse ceiling moved"
    );

    // Drive the real transform hard enough that the clamp is what decides the
    // result, and require every output to land inside the declared window.
    for size in SIZES {
        let side = size.side();
        let saturated = vec![i32::MAX; side * side];
        let output =
            kf_transform::inverse_transform(&saturated, size).expect("square input is accepted");
        for sample in output {
            assert!(
                (min..=max).contains(&sample),
                "inverse output {sample} escaped the declared window at size {side}"
            );
        }
    }
}

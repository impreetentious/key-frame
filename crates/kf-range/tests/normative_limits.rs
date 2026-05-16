//! The entropy layer's declared constants, checked against what it enforces.
//!
//! The same hole the picture bounds had, in the place it would matter most.
//! `spec/v1/constants.toml` declares the probability scale, its clamps, the
//! adaptation shift, the context count, and the range coder's initial state,
//! and the generated normative document publishes all of them. The
//! implementation holds them as literals because they sit inside the coder's
//! inner loop.
//!
//! Editing the asset is caught, because the generated document drifts. Editing
//! the implementation was not caught by anything: a coder that clamped
//! probabilities differently, or adapted at a different rate, would still round
//! trip perfectly against itself and produce streams no conformant decoder
//! could read. It is the worst shape of entropy-coder bug — no crash, no
//! disagreement between encoder and decoder, just a format that quietly stopped
//! being the documented one.
//!
//! Everything below drives the public surface with values the asset declares
//! rather than comparing literals to literals.

use kf_range::{ContextBank, Probability, RangeDecoder, RangeEncoder};
use kf_spec::V1_ASSETS;

fn constant(name: &str) -> i64 {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "constants.toml")
        .expect("the specification exposes constants.toml");
    asset
        .contents
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            if key.trim() != name {
                return None;
            }
            value.trim().parse::<i64>().ok()
        })
        .unwrap_or_else(|| panic!("constants.toml declares no scalar {name}"))
}

fn scale(name: &str) -> u16 {
    u16::try_from(constant(name)).unwrap_or_else(|_| panic!("{name} does not fit a probability"))
}

#[test]
fn the_probability_clamps_are_the_declared_ones() {
    let minimum = scale("probability_min");
    let maximum = scale("probability_max");
    let total = scale("probability_total");

    assert!(
        Probability::new(minimum).is_ok(),
        "the document allows a probability of {minimum}"
    );
    assert!(
        Probability::new(maximum).is_ok(),
        "the document allows a probability of {maximum}"
    );
    assert!(
        Probability::new(minimum - 1).is_err(),
        "a probability below the declared minimum was accepted"
    );
    assert!(
        Probability::new(total).is_err(),
        "a probability at the full scale leaves no interval for the other symbol"
    );
    assert_eq!(
        maximum + 1,
        total,
        "the clamp and the scale have drifted apart, so one symbol can be priced at zero"
    );
}

#[test]
fn adaptation_never_leaves_the_declared_clamps() {
    // The clamps keep a symbol from ever becoming free. Driven to both extremes
    // rather than asserted from the shift, so a change to the update rule is
    // caught as well as a change to the bounds.
    //
    // The two ends are not symmetric, and the asymmetry is a real property of
    // the rule rather than an accident. Going up, the step is
    // `(total - p) >> shift`, which reaches zero while `total - p` is still
    // under `2^shift` — so adaptation stalls short of the clamp and the upper
    // clamp never engages. Going down, the step is `(0 - p) >> shift`, and an
    // arithmetic shift of a negative number rounds away from zero, so it keeps
    // stepping by one until the clamp catches it. The lower clamp is load
    // bearing; the upper one is a guard rail.
    let minimum = scale("probability_min");
    let maximum = scale("probability_max");
    let total = i32::from(scale("probability_total"));
    let shift = u32::try_from(constant("adaptation_shift")).expect("a small shift");

    let mut towards_one = Probability::new(2048).expect("mid scale");
    let mut towards_zero = Probability::new(2048).expect("mid scale");
    for _ in 0..4096 {
        towards_one.update(true);
        towards_zero.update(false);
        assert!(
            (minimum..=maximum).contains(&towards_one.p1())
                && (minimum..=maximum).contains(&towards_zero.p1()),
            "adaptation left the declared clamps"
        );
    }

    // Where the upward stall lands: the largest p for which the step is still
    // nonzero, plus that step.
    let stall = total - (1 << shift) + 1;
    assert_eq!(
        i32::from(towards_one.p1()),
        stall,
        "adaptation towards one does not stall where the shift says it should"
    );
    assert_eq!(
        towards_zero.p1(),
        minimum,
        "adaptation towards zero does not settle at the declared minimum"
    );
}

#[test]
fn the_adaptation_shift_is_the_declared_rate() {
    // One update from mid scale moves by exactly (target - current) >> shift.
    // Checking the first step rather than the settling point is what pins the
    // rate: a coder that adapted twice as fast would still reach the same
    // clamps, and would price every symbol differently on the way.
    let shift = u32::try_from(constant("adaptation_shift")).expect("a small shift");
    let total = i32::from(scale("probability_total"));
    let start = 2048_i32;

    let mut probability = Probability::new(u16::try_from(start).expect("mid scale")).unwrap();
    probability.update(true);
    let expected_up = start + ((total - start) >> shift);
    assert_eq!(
        i32::from(probability.p1()),
        expected_up,
        "adapting towards one does not move at the declared shift"
    );

    let mut probability = Probability::new(u16::try_from(start).expect("mid scale")).unwrap();
    probability.update(false);
    let expected_down = start + ((0 - start) >> shift);
    assert_eq!(
        i32::from(probability.p1()),
        expected_down,
        "adapting towards zero does not move at the declared shift"
    );
}

#[test]
fn the_context_bank_holds_the_declared_number_of_slots() {
    // Declared in contexts.toml rather than constants.toml, because the count
    // is a fact about that table rather than a limit of the format.
    let declared = usize::try_from(context_count()).expect("a context count");
    let bank = ContextBank::initial();
    assert_eq!(
        bank.as_slice().len(),
        declared,
        "the context bank and the declared count have drifted apart"
    );
    assert!(
        bank.get(u16::try_from(declared).expect("a context id") - 1)
            .is_ok(),
        "the last declared context is unreachable"
    );
    assert!(
        bank.get(u16::try_from(declared).expect("a context id"))
            .is_err(),
        "a context past the declared bank was reachable"
    );
}

/// The number of contexts the frozen bank declares.
fn context_count() -> i64 {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "contexts.toml")
        .expect("the specification exposes contexts.toml");
    asset
        .contents
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "count").then(|| value.trim().parse::<i64>().ok())?
        })
        .expect("contexts.toml declares a count")
}

#[test]
fn the_decoder_needs_exactly_the_declared_initial_bytes() {
    // Five bytes, declared, and the reason the packet layer refuses a shorter
    // payload. A decoder that started from four would read one byte of whatever
    // followed the payload into its initial range.
    let initial = usize::try_from(constant("decoder_initial_bytes")).expect("a small count");
    assert!(
        RangeDecoder::new(&vec![0_u8; initial]).is_ok(),
        "the decoder refused exactly the declared initial read"
    );
    assert!(
        RangeDecoder::new(&vec![0_u8; initial - 1]).is_err(),
        "the decoder started from fewer bytes than the document declares"
    );
}

#[test]
fn finalization_flushes_the_declared_number_of_bytes() {
    // The tail is what lets a decoder finish the last symbol. Measured through
    // the shortest possible stream — one bin — where the output is the
    // finalization and nothing else.
    let calls = usize::try_from(constant("encoder_finish_calls")).expect("a small count");
    let mut encoder = RangeEncoder::new();
    let mut probability = Probability::new(2048).expect("mid scale");
    encoder
        .encode_context(false, &mut probability)
        .expect("one bin encodes");
    let encoded = encoder.finish();
    assert_eq!(
        encoded.bytes.len(),
        calls,
        "a one-bin stream is exactly the declared finalization tail"
    );
}

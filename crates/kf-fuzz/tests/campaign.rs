use kf_fuzz::{FuzzTarget, SMOKE_ITERATIONS, run_campaign};

#[test]
fn smoke_campaign_does_not_panic() {
    for target in FuzzTarget::ALL {
        run_campaign(target, SMOKE_ITERATIONS).unwrap();
    }
}

/// The value `constants.toml` declares for one key.
fn declared(key: &str) -> u32 {
    kf_spec::V1_ASSETS
        .iter()
        .find(|asset| asset.name == "constants.toml")
        .expect("the specification exposes constants.toml")
        .contents
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == key).then(|| value.trim().parse::<u32>().ok())?
        })
        .unwrap_or_else(|| panic!("constants.toml declares no {key}"))
}

#[test]
fn the_campaign_budgets_are_the_ones_the_specification_declares() {
    // Both counts are published claims: the frozen constants declare them and
    // the generated documentation prints them. A budget quietly reduced here
    // would leave the repository claiming a campaign it no longer runs, and
    // nothing else in the build would notice — a smaller campaign passes.
    //
    // The preflight count was not declared at all until it was found written
    // as a literal in the gate script, a third number beside the nightly one
    // and the unit tests' smoke run, with the crate's own documentation
    // calling the smoke run the preflight one.
    assert_eq!(
        kf_fuzz::NIGHTLY_ITERATIONS,
        declared("fuzz_iterations_per_target"),
        "the compiled nightly budget and the declared one have drifted apart"
    );
    assert_eq!(
        kf_fuzz::PREFLIGHT_ITERATIONS,
        declared("fuzz_iterations_preflight"),
        "the compiled preflight budget and the declared one have drifted apart"
    );
    // These operands are compile-time constants, so the ordering is settled
    // when the test crate is built rather than when it runs: an edit that
    // inverts it fails to compile instead of failing a campaign later.
    const {
        assert!(
            kf_fuzz::SMOKE_ITERATIONS < kf_fuzz::PREFLIGHT_ITERATIONS,
            "the unit-test smoke run must be cheaper than the preflight campaign"
        );
        assert!(
            kf_fuzz::PREFLIGHT_ITERATIONS < kf_fuzz::NIGHTLY_ITERATIONS,
            "the preflight campaign must be cheaper than the nightly one"
        );
    }
}

#[test]
fn this_profile_stops_on_integer_overflow() {
    // Cheap here, because the test profile checks arithmetic by default. The
    // load-bearing run is the campaign binary, which asks the same question of
    // itself before it starts and refuses to report a clean campaign from a
    // build that would wrap: `cargo test --release` and `cargo run --release`
    // are different profiles, so the only trustworthy place to ask is inside
    // the artifact that does the work.
    assert!(
        kf_fuzz::overflow_is_checked(),
        "this build wraps on integer overflow, so a wrapping decoder bug would decode quietly"
    );
}

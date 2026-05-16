use kf_fuzz::{FuzzTarget, SMOKE_ITERATIONS, run_campaign};

#[test]
fn smoke_campaign_does_not_panic() {
    for target in FuzzTarget::ALL {
        run_campaign(target, SMOKE_ITERATIONS).unwrap();
    }
}

#[test]
fn the_nightly_budget_is_the_one_the_specification_declares() {
    // The iteration count is a published claim: the frozen constants declare it
    // and the generated documentation prints it. A budget quietly reduced here
    // would leave the repository claiming a campaign it no longer runs, and
    // nothing else in the build would notice — a smaller campaign passes.
    let declared = kf_spec::V1_ASSETS
        .iter()
        .find(|asset| asset.name == "constants.toml")
        .expect("the specification exposes constants.toml")
        .contents
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "fuzz_iterations_per_target")
                .then(|| value.trim().parse::<u32>().ok())?
        })
        .expect("constants.toml declares the campaign budget");

    assert_eq!(
        kf_fuzz::NIGHTLY_ITERATIONS,
        declared,
        "the compiled campaign budget and the declared one have drifted apart"
    );
    // Both operands are compile-time constants, so this is settled when the
    // test crate is built rather than when it runs: a budget edit that inverts
    // the relationship fails to compile instead of failing a campaign later.
    const {
        assert!(
            kf_fuzz::SMOKE_ITERATIONS < kf_fuzz::NIGHTLY_ITERATIONS,
            "the preflight smoke run must be cheaper than the nightly campaign"
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

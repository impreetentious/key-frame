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

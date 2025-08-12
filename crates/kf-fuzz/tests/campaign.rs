use kf_fuzz::{FuzzTarget, SMOKE_ITERATIONS, run_campaign};

#[test]
fn smoke_campaign_does_not_panic() {
    for target in FuzzTarget::ALL {
        run_campaign(target, SMOKE_ITERATIONS).unwrap();
    }
}

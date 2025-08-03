use kf_enc::{EncodeError, FrameDecision, GopPlanner};

fn decision(key: bool, golden_refresh: bool) -> FrameDecision {
    FrameDecision {
        key,
        golden_refresh,
    }
}

#[test]
fn trap_scene_cut_history() {
    let mut all_zero = GopPlanner::new(120, 16).unwrap();
    assert_eq!(all_zero.next(0, None).unwrap(), decision(true, true));
    for frame_index in 1..=4 {
        assert!(!all_zero.next(frame_index, Some(0)).unwrap().key);
    }
    assert_eq!(all_zero.next(5, Some(1)).unwrap(), decision(true, true));
    assert!(!all_zero.next(6, Some(1)).unwrap().key);

    let mut exact = GopPlanner::new(120, 16).unwrap();
    exact.next(0, None).unwrap();
    for frame_index in 1..=4 {
        exact.next(frame_index, Some(1)).unwrap();
    }
    assert!(exact.next(5, Some(3)).unwrap().key);

    let mut below = GopPlanner::new(120, 16).unwrap();
    below.next(0, None).unwrap();
    for frame_index in 1..=4 {
        below.next(frame_index, Some(1)).unwrap();
    }
    assert!(!below.next(5, Some(2)).unwrap().key);

    let mut eviction = GopPlanner::new(120, 32).unwrap();
    eviction.next(0, None).unwrap();
    eviction.next(1, Some(100)).unwrap();
    for frame_index in 2..=17 {
        assert!(!eviction.next(frame_index, Some(1)).unwrap().key);
    }
    assert!(eviction.next(18, Some(3)).unwrap().key);

    let mut extremes = GopPlanner::new(120, 32).unwrap();
    extremes.next(0, None).unwrap();
    for frame_index in 1..=17 {
        assert!(!extremes.next(frame_index, Some(u64::MAX)).unwrap().key);
    }

    assert_eq!(
        GopPlanner::new(0, 1),
        Err(EncodeError::InvalidInput {
            element: "gop.key_interval"
        })
    );
    assert_eq!(
        GopPlanner::new(1, 0),
        Err(EncodeError::InvalidInput {
            element: "gop.golden_interval"
        })
    );
}

#[test]
fn periodic_keys_reset_the_exact_distance() {
    let mut planner = GopPlanner::new(3, 16).unwrap();
    assert!(planner.next(0, None).unwrap().key);
    assert!(!planner.next(1, Some(0)).unwrap().key);
    assert!(!planner.next(2, Some(0)).unwrap().key);
    assert!(planner.next(3, Some(0)).unwrap().key);
    assert!(!planner.next(4, Some(0)).unwrap().key);
    assert!(!planner.next(5, Some(0)).unwrap().key);
    assert!(planner.next(6, Some(0)).unwrap().key);
}

#[test]
fn trap_golden_refresh_count() {
    let mut planner = GopPlanner::new(5, 2).unwrap();
    assert_eq!(planner.next(0, None).unwrap(), decision(true, true));
    assert_eq!(planner.next(1, Some(0)).unwrap(), decision(false, false));
    assert_eq!(planner.next(2, Some(0)).unwrap(), decision(false, true));
    assert_eq!(planner.next(3, Some(0)).unwrap(), decision(false, false));
    assert_eq!(planner.next(4, Some(0)).unwrap(), decision(false, true));
    assert_eq!(planner.next(5, Some(0)).unwrap(), decision(true, true));
    assert_eq!(planner.next(6, Some(0)).unwrap(), decision(false, false));
    assert_eq!(planner.next(7, Some(0)).unwrap(), decision(false, true));
}

#[test]
fn nonsequential_inputs_are_rejected() {
    let mut planner = GopPlanner::new(120, 16).unwrap();
    assert!(planner.next(0, None).is_ok());
    assert_eq!(
        planner.next(2, Some(0)),
        Err(EncodeError::Policy {
            element: "gop.frame_index_sequence"
        })
    );
}

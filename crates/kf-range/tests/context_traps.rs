use kf_range::{ContextBank, Probability, modeled_cost_q16};

#[test]
fn trap_probability_semantics() {
    let probability = Probability::new(3584).unwrap();
    assert!(
        modeled_cost_q16(true, probability).unwrap()
            < modeled_cost_q16(false, probability).unwrap()
    );
}

#[test]
fn context_snapshot_is_transactional_by_clone() {
    let original = ContextBank::initial();
    let mut transaction = original.clone();
    transaction.get_mut(0).unwrap().update(true);
    assert_ne!(transaction, original);
    assert_eq!(original.get(0).unwrap().p1(), 2048);
}

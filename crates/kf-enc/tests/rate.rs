use kf_enc::RateController;
use kf_spec::V1_ASSETS;

struct RateVector {
    name: String,
    fill_bits: i64,
    capacity_bits: i64,
    qp_in: u8,
    qp_out: u8,
}

fn parse_vectors() -> Vec<RateVector> {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "quant.toml")
        .unwrap();
    let mut vectors = Vec::new();
    let mut name = None;
    let mut fill_bits = None;
    let mut capacity_bits = None;
    let mut qp_in = None;
    for line in asset.contents.lines() {
        if let Some(value) = line.strip_prefix("name = \"") {
            name = Some(value.trim_end_matches('"').to_owned());
        } else if let Some(value) = line.strip_prefix("fill_bits = ") {
            fill_bits = Some(value.parse().unwrap());
        } else if let Some(value) = line.strip_prefix("capacity_bits = ") {
            capacity_bits = Some(value.parse().unwrap());
        } else if let Some(value) = line.strip_prefix("qp_in = ") {
            qp_in = Some(value.parse().unwrap());
        } else if let Some(value) = line.strip_prefix("qp_out = ") {
            vectors.push(RateVector {
                name: name.clone().unwrap(),
                fill_bits: fill_bits.unwrap(),
                capacity_bits: capacity_bits.unwrap(),
                qp_in: qp_in.unwrap(),
                qp_out: value.parse().unwrap(),
            });
        }
    }
    vectors
}

fn step_qp(fill: i64, capacity: i64, qp: u8) -> u8 {
    let low = capacity / 3;
    let high = (2 * capacity) / 3;
    if fill < low {
        qp.saturating_sub(2)
    } else if fill > high {
        qp.saturating_add(2).min(63)
    } else {
        qp
    }
}

#[test]
fn frozen_rate_control_vectors_replay() {
    let vectors = parse_vectors();
    assert_eq!(vectors.len(), 5);
    for vector in vectors {
        assert_eq!(
            step_qp(vector.fill_bits, vector.capacity_bits, vector.qp_in),
            vector.qp_out,
            "{}",
            vector.name
        );
    }
}

#[test]
fn trap_rc_bucket_bounds() {
    let mut starved = RateController::new(10_000, 30, 1).unwrap();
    for _ in 0..64 {
        starved.observe_complexity(u64::MAX);
        starved.commit_frame_bits(u64::MAX);
    }
    assert_eq!(starved.qp(), 63);
    assert_eq!(starved.fill_q16(), starved.capacity_q16());

    let mut flooded = RateController::new(100_000_000, 30, 1).unwrap();
    for _ in 0..64 {
        flooded.observe_complexity(0);
        flooded.commit_frame_bits(1);
    }
    assert_eq!(flooded.qp(), 0);
    assert_eq!(flooded.fill_q16(), 0);
}

#[test]
fn bucket_starts_in_the_middle_third() {
    let controller = RateController::new(120_000, 30, 1).unwrap();
    let low = controller.capacity_q16() / 3;
    let high = (2 * controller.capacity_q16()) / 3;
    assert!(controller.fill_q16() > low);
    assert!(controller.fill_q16() < high);
}

#[test]
fn rate_controller_is_deterministic() {
    let mut first = RateController::new(500_000, 24, 1).unwrap();
    let mut second = RateController::new(500_000, 24, 1).unwrap();
    for bits in [8000_u64, 12000, 9000, 11000] {
        first.observe_complexity(bits);
        second.observe_complexity(bits);
        first.commit_frame_bits(bits);
        second.commit_frame_bits(bits);
        assert_eq!(first, second);
    }
}

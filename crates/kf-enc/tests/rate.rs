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

#[test]
fn the_complexity_average_reaches_the_quantizer() {
    // It did not, for a while. The exponentially weighted average was computed
    // on every frame and then never read, so the controller was reacting to
    // bucket fullness alone — a report on what already happened — and the
    // "bucket fullness plus complexity" model existed only on paper.
    //
    // This checks the wiring rather than the tuning: a controller fed steadily
    // hard frames and one fed steadily easy frames must reach a different bias,
    // or the average is still decorative.
    let mut controller = RateController::new(200_000, 30, 1).unwrap();
    assert_eq!(
        controller.complexity_bias_q16(),
        0,
        "with no history there is nothing to be harder or easier than"
    );

    // Settle the average at a moderate level, then hand it a much harder frame.
    for _ in 0..32 {
        controller.observe_complexity(1_000_000);
    }
    let settled = controller.complexity_bias_q16();
    controller.observe_complexity(4_000_000);
    let harder = controller.complexity_bias_q16();
    controller.observe_complexity(0);
    let easier = controller.complexity_bias_q16();

    assert_eq!(
        settled, 0,
        "a frame of average difficulty must not bias anything"
    );
    assert!(
        harder > 0,
        "a harder frame must push towards a higher quantizer"
    );
    assert!(easier < 0, "an easier frame must push the other way");

    // And the bias stays small against the bands it shifts. A bias that could
    // cross a third would let complexity override fullness rather than inform
    // it, which is a different controller from the one that was designed.
    let capacity = controller.capacity_q16();
    assert!(
        harder.abs() < capacity / 3 && easier.abs() < capacity / 3,
        "the complexity bias can override bucket fullness outright"
    );
}

#[test]
fn a_biased_controller_is_still_deterministic() {
    // The complexity term reads only its own history, so two controllers given
    // the same sequence have to agree exactly. If it ever depended on anything
    // ambient, average bitrate would stop being reproducible and every receipt
    // measured through it would become unrepeatable.
    let run = || {
        let mut controller = RateController::new(150_000, 30, 1).unwrap();
        let mut trace = Vec::new();
        for index in 0..64_u64 {
            // A deliberately uneven sequence, so the bias changes sign often.
            controller.observe_complexity((index * 977) % 5_000_000);
            controller.commit_frame_bits((index * 7919) % 90_000);
            trace.push((
                controller.qp(),
                controller.fill_q16(),
                controller.complexity_bias_q16(),
            ));
        }
        trace
    };
    assert_eq!(run(), run());
}

/// Reads a declared scalar out of the frozen constants.
fn declared(name: &str) -> u8 {
    kf_spec::V1_ASSETS
        .iter()
        .find(|asset| asset.name == "constants.toml")
        .expect("the specification exposes constants.toml")
        .contents
        .lines()
        .find_map(|line| {
            line.strip_prefix(&format!("{name} = "))?
                .trim()
                .parse()
                .ok()
        })
        .unwrap_or_else(|| panic!("constants.toml declares no {name}"))
}

#[test]
fn the_encoder_accepts_exactly_the_declared_quantizer_range() {
    // The encoder validated `qp > 63` in two places. A narrowed declaration
    // would have left it emitting streams both decoders refuse.
    let (min, max) = (declared("qp_min"), declared("qp_max"));
    let sequence =
        kf_bitstream::SequenceHeader::new(64, 64, 24, 1, 120, 16).expect("a legal sequence");

    assert!(kf_enc::Encoder::new(sequence, min).is_ok());
    assert!(kf_enc::Encoder::new(sequence, max).is_ok());
    assert!(kf_enc::Encoder::new(sequence, max + 1).is_err());
    assert!(kf_enc::RateControl::constant_qp(max).is_ok());
    assert!(kf_enc::RateControl::constant_qp(max + 1).is_err());
}

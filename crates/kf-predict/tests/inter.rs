use kf_frame::Plane;
use kf_predict::{MotionVector, PlaneScale, clamp_motion_vector, predict_inter};

const VECTORS: &str = include_str!("../../../spec/v1/mc-vectors.toml");

#[derive(Debug)]
struct VectorCase {
    name: String,
    scale: PlaneScale,
    motion_vector: MotionVector,
    expected: Vec<u8>,
}

fn values(line: &str) -> Vec<i32> {
    line.split_once('=')
        .expect("vector field has an equals sign")
        .1
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|value| value.trim().parse().expect("vector value is an integer"))
        .collect()
}

fn string_value(line: &str) -> String {
    line.split_once('=')
        .expect("string field has an equals sign")
        .1
        .trim()
        .trim_matches('"')
        .to_owned()
}

fn parsed_vectors() -> (Plane, Vec<VectorCase>) {
    let source_line = VECTORS
        .lines()
        .find(|line| line.starts_with("source ="))
        .expect("vector asset has source samples");
    let source = values(source_line)
        .into_iter()
        .map(|value| u8::try_from(value).expect("source sample fits u8"))
        .collect();
    let plane = Plane::from_vec(8, 8, 8, source).unwrap();
    let mut cases = Vec::new();
    let mut name = None;
    let mut scale = None;
    let mut motion_vector = None;
    let mut expected = None;

    for line in VECTORS.lines() {
        if line == "[[cases]]" {
            if let (Some(name), Some(scale), Some(motion_vector), Some(expected)) = (
                name.take(),
                scale.take(),
                motion_vector.take(),
                expected.take(),
            ) {
                cases.push(VectorCase {
                    name,
                    scale,
                    motion_vector,
                    expected,
                });
            }
        } else if line.starts_with("name =") {
            name = Some(string_value(line));
        } else if line.starts_with("scale =") {
            scale = Some(match string_value(line).as_str() {
                "luma" => PlaneScale::Luma,
                "chroma420" => PlaneScale::Chroma420,
                other => panic!("unknown vector scale {other}"),
            });
        } else if line.starts_with("mv_q4 =") {
            let value = values(line);
            motion_vector = Some(MotionVector {
                x_q4: value[0],
                y_q4: value[1],
            });
        } else if line.starts_with("expected =") {
            expected = Some(
                values(line)
                    .into_iter()
                    .map(|value| u8::try_from(value).expect("expected sample fits u8"))
                    .collect(),
            );
        }
    }
    cases.push(VectorCase {
        name: name.expect("final vector has a name"),
        scale: scale.expect("final vector has a scale"),
        motion_vector: motion_vector.expect("final vector has an MV"),
        expected: expected.expect("final vector has expected samples"),
    });
    (plane, cases)
}

fn transpose(plane: &Plane) -> Plane {
    let mut transposed = Plane::filled(plane.height(), plane.width(), 0).unwrap();
    for y in 0..plane.height() {
        for x in 0..plane.width() {
            transposed.set(y, x, plane.get(x, y).unwrap()).unwrap();
        }
    }
    transposed
}

fn transpose_block(block: &[u8], size: usize) -> Vec<u8> {
    let mut transposed = vec![0; block.len()];
    for y in 0..size {
        for x in 0..size {
            transposed[y * size + x] = block[x * size + y];
        }
    }
    transposed
}

#[test]
fn golden_phase_vectors_match_the_independent_asset() {
    let (plane, cases) = parsed_vectors();
    assert_eq!(cases.len(), 24);
    for case in cases {
        let actual = predict_inter(&plane, 2, 2, 4, case.motion_vector, case.scale).unwrap();
        assert_eq!(actual, case.expected, "{}", case.name);
    }
}

#[test]
fn trap_mc_stage_order() {
    let field = |name: &str| {
        VECTORS
            .lines()
            .find(|line| line.starts_with(name))
            .map(values)
            .unwrap_or_else(|| panic!("vector asset has {name}"))
    };
    let source = field("stage_order_source =")
        .into_iter()
        .map(|value| u8::try_from(value).unwrap())
        .collect();
    let plane = Plane::from_vec(8, 8, 8, source).unwrap();
    let motion = field("stage_order_mv_q4 =");
    let motion_vector = MotionVector {
        x_q4: motion[0],
        y_q4: motion[1],
    };
    let expected: Vec<u8> = field("stage_order_expected =")
        .into_iter()
        .map(|value| u8::try_from(value).unwrap())
        .collect();
    let expected_vertical_first: Vec<u8> = field("stage_order_vertical_first =")
        .into_iter()
        .map(|value| u8::try_from(value).unwrap())
        .collect();
    let horizontal_then_vertical =
        predict_inter(&plane, 2, 2, 4, motion_vector, PlaneScale::Luma).unwrap();
    assert_eq!(horizontal_then_vertical, expected);

    let vertical_then_horizontal_transposed = predict_inter(
        &transpose(&plane),
        2,
        2,
        4,
        MotionVector {
            x_q4: motion_vector.y_q4,
            y_q4: motion_vector.x_q4,
        },
        PlaneScale::Luma,
    )
    .unwrap();
    let vertical_then_horizontal = transpose_block(&vertical_then_horizontal_transposed, 4);
    assert_eq!(vertical_then_horizontal, expected_vertical_first);
    assert_ne!(horizontal_then_vertical, vertical_then_horizontal);
}

#[test]
fn trap_mv_out_of_bounds() {
    let mut plane = Plane::filled(8, 8, 0).unwrap();
    for y in 0..8 {
        for x in 0..8 {
            plane.set(x, y, u8::try_from(x * 31).unwrap()).unwrap();
        }
    }
    let positive = MotionVector {
        x_q4: 10_000,
        y_q4: 10_000,
    };
    let negative = MotionVector {
        x_q4: -10_000,
        y_q4: -10_000,
    };
    assert_eq!(
        clamp_motion_vector(&plane, 0, 0, 8, positive, PlaneScale::Luma).unwrap(),
        MotionVector {
            x_q4: 256,
            y_q4: 256
        }
    );
    assert_eq!(
        clamp_motion_vector(&plane, 0, 0, 8, negative, PlaneScale::Luma).unwrap(),
        MotionVector {
            x_q4: -256,
            y_q4: -256
        }
    );
    assert_eq!(
        predict_inter(&plane, 0, 0, 8, positive, PlaneScale::Luma).unwrap(),
        vec![217; 64]
    );
    assert_eq!(
        predict_inter(&plane, 0, 0, 8, negative, PlaneScale::Luma).unwrap(),
        vec![0; 64]
    );

    assert_eq!(
        clamp_motion_vector(
            &plane,
            0,
            0,
            8,
            MotionVector {
                x_q4: 255,
                y_q4: -255
            },
            PlaneScale::Luma
        )
        .unwrap(),
        MotionVector {
            x_q4: 252,
            y_q4: -252
        }
    );
}

/// The full-pixel search bound is stated once, in quarter-luma units, and it
/// does not widen on chroma.
///
/// A motion vector is in quarter-luma units on every plane, so the declared
/// ±64-pixel bound is ±256 everywhere. Converting it through the plane's own
/// phase denominator instead would double the legal range on chroma — a change
/// no committed vector catches, because every one of them uses a small motion
/// vector well inside either bound.
#[test]
fn the_full_pixel_bound_does_not_widen_on_chroma() {
    let plane = Plane::filled(320, 320, 0).unwrap();
    let far = MotionVector {
        x_q4: 4000,
        y_q4: 4000,
    };
    // Placed centrally so the edge extension, not the frame border, is what
    // the clamp has to answer for.
    let luma = clamp_motion_vector(&plane, 128, 128, 8, far, PlaneScale::Luma).unwrap();
    let chroma = clamp_motion_vector(&plane, 128, 128, 8, far, PlaneScale::Chroma420).unwrap();
    assert!(
        luma.x_q4 <= 256 && luma.y_q4 <= 256,
        "luma clamp exceeded the declared bound: {luma:?}"
    );
    assert!(
        chroma.x_q4 <= 256 && chroma.y_q4 <= 256,
        "chroma clamp exceeded the declared bound: {chroma:?}"
    );
}

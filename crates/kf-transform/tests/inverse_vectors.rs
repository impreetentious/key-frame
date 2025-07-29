use kf_spec::V1_ASSETS;
use kf_transform::{TransformSize, inverse_transform};

#[test]
fn dc_only_vectors_are_spatially_constant() {
    for (size, expected) in [
        (TransformSize::N4, 16),
        (TransformSize::N8, 32),
        (TransformSize::N16, 64),
        (TransformSize::N32, 128),
    ] {
        let mut coefficients = vec![0; size.side() * size.side()];
        coefficients[0] = 1024;
        let output = inverse_transform(&coefficients, size).unwrap();
        assert!(output.iter().all(|&sample| sample == expected));
    }
}

#[test]
fn trap_idct_extremes() {
    for size in [
        TransformSize::N4,
        TransformSize::N8,
        TransformSize::N16,
        TransformSize::N32,
    ] {
        for level in [-32_767, 32_767] {
            let coefficients = vec![level; size.side() * size.side()];
            let output = inverse_transform(&coefficients, size).unwrap();
            assert!(
                output
                    .iter()
                    .all(|&sample| (-32_768..=32_767).contains(&sample))
            );
        }
    }
}

#[test]
fn literal_golden_asset_replays_all_cases() {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "transform-vectors.toml")
        .expect("invariant: kf-spec embeds transform vectors");
    let mut size = None;
    let mut input = None;
    let mut expected = None;
    let mut count = 0;
    for line in asset.contents.lines().chain([""]) {
        if let Some(value) = line.strip_prefix("size = ") {
            size = Some(value.parse::<u8>().unwrap());
        } else if let Some(value) = line.strip_prefix("input = ") {
            input = Some(parse_array(value));
        } else if let Some(value) = line.strip_prefix("expected = ") {
            expected = Some(parse_array(value));
        } else if line.is_empty() {
            if let (Some(size), Some(input), Some(expected)) =
                (size.take(), input.take(), expected.take())
            {
                let transform_size = TransformSize::try_from(size).unwrap();
                assert_eq!(inverse_transform(&input, transform_size).unwrap(), expected);
                count += 1;
            }
        }
    }
    assert_eq!(count, 24);
}

fn parse_array(value: &str) -> Vec<i32> {
    value
        .strip_prefix('[')
        .and_then(|body| body.strip_suffix(']'))
        .expect("invariant: checked vector array has balanced brackets")
        .split(',')
        .map(|number| number.trim().parse::<i32>().unwrap())
        .collect()
}

use std::sync::OnceLock;

use kf_spec::V1_ASSETS;

use crate::TransformBlockSize;

pub(crate) fn diagonal_scan(size: TransformBlockSize) -> &'static [usize] {
    static N4: OnceLock<Vec<usize>> = OnceLock::new();
    static N8: OnceLock<Vec<usize>> = OnceLock::new();
    static N16: OnceLock<Vec<usize>> = OnceLock::new();
    static N32: OnceLock<Vec<usize>> = OnceLock::new();
    match size {
        TransformBlockSize::N4 => N4.get_or_init(|| parse_scan(4)).as_slice(),
        TransformBlockSize::N8 => N8.get_or_init(|| parse_scan(8)).as_slice(),
        TransformBlockSize::N16 => N16.get_or_init(|| parse_scan(16)).as_slice(),
        TransformBlockSize::N32 => N32.get_or_init(|| parse_scan(32)).as_slice(),
    }
}

fn parse_scan(side: usize) -> Vec<usize> {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "scans.toml")
        .expect("invariant: kf-spec exposes scans.toml");
    let prefix = format!("n{side} = [");
    let line = asset
        .contents
        .lines()
        .find(|line| line.starts_with(&prefix))
        .expect("invariant: checked scan asset has every transform size");
    let body = line
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(']'))
        .expect("invariant: checked scan array has balanced brackets");
    let coordinates: Vec<usize> = body
        .split(',')
        .map(|value| {
            value
                .trim()
                .parse::<usize>()
                .expect("invariant: checked scan coordinate is usize")
        })
        .collect();
    assert_eq!(
        coordinates.len(),
        side * side * 2,
        "invariant: checked scan has one x/y pair per coefficient"
    );
    coordinates
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| pair[1] * side + pair[0])
        .collect()
}

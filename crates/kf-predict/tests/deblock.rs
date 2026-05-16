use kf_frame::Frame;
use kf_predict::{CodedBlock, PredictError, deblock_frame, filter_samples};
use kf_spec::V1_ASSETS;

struct AssetVector {
    name: String,
    kind: String,
    qp: u8,
    samples: [u8; 8],
    expected: [u8; 8],
}

fn parse_vectors() -> Vec<AssetVector> {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "deblock.toml")
        .unwrap();
    let mut vectors = Vec::new();
    let mut name = None;
    let mut kind = None;
    let mut qp = None;
    let mut samples = None;
    for line in asset.contents.lines() {
        if let Some(value) = line.strip_prefix("name = \"") {
            name = Some(value.trim_end_matches('"').to_owned());
        } else if let Some(value) = line.strip_prefix("kind = \"") {
            kind = Some(value.trim_end_matches('"').to_owned());
        } else if let Some(value) = line.strip_prefix("qp = ") {
            qp = Some(value.parse::<u8>().unwrap());
        } else if let Some(value) = line.strip_prefix("samples = [") {
            samples = Some(parse_u8s(value));
        } else if let Some(value) = line.strip_prefix("expected = [") {
            vectors.push(AssetVector {
                name: name.clone().unwrap(),
                kind: kind.clone().unwrap(),
                qp: qp.unwrap(),
                samples: samples.unwrap(),
                expected: parse_u8s(value),
            });
        }
    }
    vectors
}

fn parse_u8s(body: &str) -> [u8; 8] {
    let body = body.trim_end_matches(']');
    let values: Vec<u8> = body
        .split(',')
        .map(|value| value.trim().parse::<u8>().unwrap())
        .collect();
    values.try_into().unwrap()
}

#[test]
fn frozen_deblock_vectors_replay() {
    let vectors = parse_vectors();
    assert_eq!(vectors.len(), 7);
    for vector in vectors {
        let strength = if vector.kind == "strong" { 2 } else { 1 };
        let luma = filter_samples(vector.samples, vector.qp, strength, false);
        assert_eq!(luma, vector.expected, "{} luma", vector.name);
        if vector.kind == "strong" {
            let chroma = filter_samples(vector.samples, vector.qp, 2, true);
            let weak = filter_samples(vector.samples, vector.qp, 1, false);
            assert_eq!(chroma, weak, "{} chroma uses weak", vector.name);
        }
    }
}

#[test]
fn deblock_installs_filtered_samples_on_a_coding_block_edge() {
    let mut frame = Frame::filled_420(64, 64, 100).unwrap();
    for y in 0..64 {
        for x in 32..64 {
            frame.y.set(x, y, 140).unwrap();
        }
    }
    let before = frame.y.get(31, 8).unwrap();
    deblock_frame(
        &mut frame,
        32,
        &[
            CodedBlock {
                x: 0,
                y: 0,
                size: 32,
                intra: true,
                coded: true,
            },
            CodedBlock {
                x: 32,
                y: 0,
                size: 32,
                intra: true,
                coded: true,
            },
            CodedBlock {
                x: 0,
                y: 32,
                size: 32,
                intra: true,
                coded: true,
            },
            CodedBlock {
                x: 32,
                y: 32,
                size: 32,
                intra: true,
                coded: true,
            },
        ],
    )
    .unwrap();
    assert_ne!(frame.y.get(31, 8).unwrap(), before);
    assert_ne!(frame.y.get(32, 8).unwrap(), 140);
}

#[test]
fn deblock_rejects_qp_outside_the_closed_range() {
    let mut frame = Frame::filled_420(64, 64, 128).unwrap();
    let error = deblock_frame(&mut frame, 64, &[]).unwrap_err();
    assert_eq!(error, PredictError::InvalidQp { qp: 64 });
}

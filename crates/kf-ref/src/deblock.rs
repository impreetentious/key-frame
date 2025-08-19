use std::collections::BTreeMap;

use kf_frame::{Frame, Plane};
use kf_spec::V1_ASSETS;

use crate::ReferenceError;

/// Coding-block flags the independent loop filter needs for each leaf.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RefCodedBlock {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) size: u32,
    pub(crate) intra: bool,
    pub(crate) coded: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    CodingBlock,
    DerivedTransform,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct EdgeKey {
    vertical: bool,
    line: u32,
    start: u32,
}

struct Edge {
    length: u32,
    kind: Kind,
    intra: bool,
    coded: bool,
}

struct Tables {
    alpha: [u8; 64],
    beta: [u8; 64],
    tc: [u8; 64],
    /// Indexed `[kind][either_intra][either_coded]` with coding-block fallback.
    strength: [[[u8; 2]; 2]; 2],
}

/// Independent in-place loop filter. The filtered samples are the reference.
pub(crate) fn loop_filter_frame(
    frame: &mut Frame,
    qp: u8,
    blocks: &[RefCodedBlock],
) -> Result<(), ReferenceError> {
    if qp > 63 {
        return Err(ReferenceError::new(0, "deblock.qp"));
    }
    let tables = load_tables();
    let width = frame.width();
    let height = frame.height();
    let edges = gather_edges(blocks, width, height);
    filter_plane(&mut frame.y, qp, &edges, false, &tables)?;
    let chroma = chroma_scale(&edges);
    filter_plane(&mut frame.cb, qp, &chroma, true, &tables)?;
    filter_plane(&mut frame.cr, qp, &chroma, true, &tables)?;
    Ok(())
}

fn gather_edges(blocks: &[RefCodedBlock], width: u32, height: u32) -> Vec<(EdgeKey, Edge)> {
    let mut edges = BTreeMap::new();
    for block in blocks {
        if block.size < 8 {
            continue;
        }
        let right = block.x + block.size;
        let bottom = block.y + block.size;
        if right < width {
            let neighbor = covering(blocks, right, block.y);
            insert_edge(
                &mut edges,
                EdgeKey {
                    vertical: true,
                    line: right,
                    start: block.y,
                },
                Edge {
                    length: block.size,
                    kind: Kind::CodingBlock,
                    intra: block.intra || neighbor.is_some_and(|other| other.intra),
                    coded: block.coded || neighbor.is_some_and(|other| other.coded),
                },
            );
        }
        if bottom < height {
            let neighbor = covering(blocks, block.x, bottom);
            insert_edge(
                &mut edges,
                EdgeKey {
                    vertical: false,
                    line: bottom,
                    start: block.x,
                },
                Edge {
                    length: block.size,
                    kind: Kind::CodingBlock,
                    intra: block.intra || neighbor.is_some_and(|other| other.intra),
                    coded: block.coded || neighbor.is_some_and(|other| other.coded),
                },
            );
        }
        if block.size == 64 {
            let mid_x = block.x + 32;
            let mid_y = block.y + 32;
            if mid_x < width {
                insert_edge(
                    &mut edges,
                    EdgeKey {
                        vertical: true,
                        line: mid_x,
                        start: block.y,
                    },
                    Edge {
                        length: 64,
                        kind: Kind::DerivedTransform,
                        intra: block.intra,
                        coded: block.coded,
                    },
                );
            }
            if mid_y < height {
                insert_edge(
                    &mut edges,
                    EdgeKey {
                        vertical: false,
                        line: mid_y,
                        start: block.x,
                    },
                    Edge {
                        length: 64,
                        kind: Kind::DerivedTransform,
                        intra: block.intra,
                        coded: block.coded,
                    },
                );
            }
        }
    }
    edges.into_iter().collect()
}

fn insert_edge(edges: &mut BTreeMap<EdgeKey, Edge>, key: EdgeKey, edge: Edge) {
    edges.entry(key).or_insert(edge);
}

fn covering(blocks: &[RefCodedBlock], x: u32, y: u32) -> Option<RefCodedBlock> {
    blocks.iter().copied().find(|block| {
        x >= block.x && x < block.x + block.size && y >= block.y && y < block.y + block.size
    })
}

fn chroma_scale(edges: &[(EdgeKey, Edge)]) -> Vec<(EdgeKey, Edge)> {
    edges
        .iter()
        .filter(|(key, edge)| {
            edge.length >= 16 && key.line.is_multiple_of(2) && key.start.is_multiple_of(2)
        })
        .map(|(key, edge)| {
            (
                EdgeKey {
                    vertical: key.vertical,
                    line: key.line / 2,
                    start: key.start / 2,
                },
                Edge {
                    length: edge.length / 2,
                    kind: edge.kind,
                    intra: edge.intra,
                    coded: edge.coded,
                },
            )
        })
        .filter(|(_, edge)| edge.length >= 8)
        .collect()
}

fn filter_plane(
    plane: &mut Plane,
    qp: u8,
    edges: &[(EdgeKey, Edge)],
    chroma: bool,
    tables: &Tables,
) -> Result<(), ReferenceError> {
    let mut vertical: Vec<&(EdgeKey, Edge)> =
        edges.iter().filter(|(key, _)| key.vertical).collect();
    let mut horizontal: Vec<&(EdgeKey, Edge)> =
        edges.iter().filter(|(key, _)| !key.vertical).collect();
    vertical.sort_by_key(|(key, _)| (key.line, key.start));
    horizontal.sort_by_key(|(key, _)| (key.line, key.start));
    for edge in vertical.into_iter().chain(horizontal) {
        apply_edge(plane, qp, edge, chroma, tables)?;
    }
    Ok(())
}

fn apply_edge(
    plane: &mut Plane,
    qp: u8,
    edge: &(EdgeKey, Edge),
    chroma: bool,
    tables: &Tables,
) -> Result<(), ReferenceError> {
    let (key, meta) = edge;
    let kind_index = usize::from(meta.kind == Kind::DerivedTransform);
    let strength = tables.strength[kind_index][usize::from(meta.intra)][usize::from(meta.coded)];
    if strength == 0 {
        return Ok(());
    }
    let mut offset = 0_u32;
    while offset + 8 <= meta.length {
        for step in 0..8 {
            let along = key.start + offset + step;
            let window = if key.vertical {
                load_vertical(plane, key.line, along)?
            } else {
                load_horizontal(plane, key.line, along)?
            };
            let filtered = filter_window(window, qp, strength, chroma, tables);
            if key.vertical {
                store_vertical(plane, key.line, along, filtered)?;
            } else {
                store_horizontal(plane, key.line, along, filtered)?;
            }
        }
        offset += 8;
    }
    Ok(())
}

fn filter_window(window: [u8; 8], qp: u8, strength: u8, chroma: bool, tables: &Tables) -> [u8; 8] {
    let strength = if chroma && strength == 2 { 1 } else { strength };
    if strength == 0 {
        return window;
    }
    let qp = usize::from(qp);
    let p0 = i32::from(window[3]);
    let p1 = i32::from(window[2]);
    let q0 = i32::from(window[4]);
    let q1 = i32::from(window[5]);
    if p0.abs_diff(q0) >= u32::from(tables.alpha[qp])
        || p1.abs_diff(p0) >= u32::from(tables.beta[qp])
        || q1.abs_diff(q0) >= u32::from(tables.beta[qp])
    {
        return window;
    }
    if strength == 1 {
        weak_four_tap(window, i32::from(tables.tc[qp]))
    } else {
        strong_six_tap(window)
    }
}

fn weak_four_tap(window: [u8; 8], tc: i32) -> [u8; 8] {
    let p0 = i32::from(window[3]);
    let p1 = i32::from(window[2]);
    let q0 = i32::from(window[4]);
    let q1 = i32::from(window[5]);
    let delta = (((q0 - p0) * 4 + (p1 - q1) + 4) >> 3).clamp(-tc, tc);
    let mut out = window;
    out[3] = clip8(p0 + delta);
    out[4] = clip8(q0 - delta);
    out
}

fn strong_six_tap(window: [u8; 8]) -> [u8; 8] {
    let p3 = i32::from(window[0]);
    let p2 = i32::from(window[1]);
    let p1 = i32::from(window[2]);
    let p0 = i32::from(window[3]);
    let q0 = i32::from(window[4]);
    let q1 = i32::from(window[5]);
    let q2 = i32::from(window[6]);
    let q3 = i32::from(window[7]);
    let mut out = window;
    out[1] = clip8((2 * p3 + 3 * p2 + p1 + p0 + q0 + 4) >> 3);
    out[2] = clip8((p2 + p1 + p0 + q0 + 2) >> 2);
    out[3] = clip8((p2 + 2 * p1 + 2 * p0 + 2 * q0 + q1 + 4) >> 3);
    out[4] = clip8((p1 + 2 * p0 + 2 * q0 + 2 * q1 + q2 + 4) >> 3);
    out[5] = clip8((p0 + q0 + q1 + q2 + 2) >> 2);
    out[6] = clip8((p0 + q0 + q1 + 3 * q2 + 2 * q3 + 4) >> 3);
    out
}

fn clip8(value: i32) -> u8 {
    u8::try_from(value.clamp(0, 255)).expect("invariant: 0..=255 fits u8")
}

fn load_vertical(plane: &Plane, x: u32, y: u32) -> Result<[u8; 8], ReferenceError> {
    Ok([
        get(plane, x.saturating_sub(4), y)?,
        get(plane, x.saturating_sub(3), y)?,
        get(plane, x.saturating_sub(2), y)?,
        get(plane, x.saturating_sub(1), y)?,
        get(plane, x, y)?,
        get(plane, x.saturating_add(1), y)?,
        get(plane, x.saturating_add(2), y)?,
        get(plane, x.saturating_add(3), y)?,
    ])
}

fn store_vertical(
    plane: &mut Plane,
    x: u32,
    y: u32,
    window: [u8; 8],
) -> Result<(), ReferenceError> {
    set(plane, x.saturating_sub(3), y, window[1])?;
    set(plane, x.saturating_sub(2), y, window[2])?;
    set(plane, x.saturating_sub(1), y, window[3])?;
    set(plane, x, y, window[4])?;
    set(plane, x.saturating_add(1), y, window[5])?;
    set(plane, x.saturating_add(2), y, window[6])?;
    Ok(())
}

fn load_horizontal(plane: &Plane, y: u32, x: u32) -> Result<[u8; 8], ReferenceError> {
    Ok([
        get(plane, x, y.saturating_sub(4))?,
        get(plane, x, y.saturating_sub(3))?,
        get(plane, x, y.saturating_sub(2))?,
        get(plane, x, y.saturating_sub(1))?,
        get(plane, x, y)?,
        get(plane, x, y.saturating_add(1))?,
        get(plane, x, y.saturating_add(2))?,
        get(plane, x, y.saturating_add(3))?,
    ])
}

fn store_horizontal(
    plane: &mut Plane,
    y: u32,
    x: u32,
    window: [u8; 8],
) -> Result<(), ReferenceError> {
    set(plane, x, y.saturating_sub(3), window[1])?;
    set(plane, x, y.saturating_sub(2), window[2])?;
    set(plane, x, y.saturating_sub(1), window[3])?;
    set(plane, x, y, window[4])?;
    set(plane, x, y.saturating_add(1), window[5])?;
    set(plane, x, y.saturating_add(2), window[6])?;
    Ok(())
}

fn get(plane: &Plane, x: u32, y: u32) -> Result<u8, ReferenceError> {
    plane
        .get(x, y)
        .map_err(|_| ReferenceError::new(0, "deblock.read"))
}

fn set(plane: &mut Plane, x: u32, y: u32, value: u8) -> Result<(), ReferenceError> {
    plane
        .set(x, y, value)
        .map_err(|_| ReferenceError::new(0, "deblock.write"))
}

fn load_tables() -> Tables {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "deblock.toml")
        .expect("invariant: kf-spec exposes deblock.toml");
    let mut tables = Tables {
        alpha: parse_thresholds(asset.contents, "alpha = ["),
        beta: parse_thresholds(asset.contents, "beta = ["),
        tc: parse_thresholds(asset.contents, "tc = ["),
        strength: [[[0; 2]; 2]; 2],
    };
    let mut filled = [[[false; 2]; 2]; 2];
    let mut kind = None;
    let mut intra = None;
    let mut coded = None;
    for line in asset.contents.lines() {
        if line == "[[decision]]" {
            kind = None;
            intra = None;
            coded = None;
        } else if let Some(value) = line.strip_prefix("edge_kind = \"") {
            kind = Some(match value.trim_end_matches('"') {
                "coding_block" => Kind::CodingBlock,
                _ => Kind::DerivedTransform,
            });
        } else if let Some(value) = line.strip_prefix("either_intra = ") {
            intra = Some(value == "true");
        } else if let Some(value) = line.strip_prefix("either_coded = ") {
            coded = Some(value == "true");
        } else if let Some(value) = line.strip_prefix("strength = ")
            && let (Some(kind), Some(intra), Some(coded)) = (kind, intra, coded)
        {
            let kind_index = usize::from(kind == Kind::DerivedTransform);
            tables.strength[kind_index][usize::from(intra)][usize::from(coded)] =
                value.parse().expect("invariant: strength is u8");
            filled[kind_index][usize::from(intra)][usize::from(coded)] = true;
        }
    }
    for intra in [false, true] {
        for coded in [false, true] {
            if !filled[1][usize::from(intra)][usize::from(coded)] {
                tables.strength[1][usize::from(intra)][usize::from(coded)] =
                    tables.strength[0][usize::from(intra)][usize::from(coded)];
            }
        }
    }
    tables
}

fn parse_thresholds(contents: &str, prefix: &str) -> [u8; 64] {
    let line = contents
        .lines()
        .find(|line| line.starts_with(prefix))
        .expect("invariant: checked deblock asset has threshold arrays");
    let body = line
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(']'))
        .expect("invariant: checked threshold array has brackets");
    let values: Vec<u8> = body
        .split(',')
        .map(|value| {
            value
                .trim()
                .parse::<u8>()
                .expect("invariant: threshold entry is u8")
        })
        .collect();
    values
        .try_into()
        .expect("invariant: checked deblock thresholds have 64 entries")
}

#[cfg(test)]
mod tests {
    use super::{RefCodedBlock, filter_window, load_tables, loop_filter_frame};
    use kf_frame::Frame;
    use kf_spec::V1_ASSETS;

    #[test]
    fn independent_filter_replays_frozen_sample_rows() {
        let tables = load_tables();
        let mut seen = 0_usize;
        let mut name = None;
        let mut kind = None;
        let mut qp = None;
        let mut samples = None;
        let asset = V1_ASSETS
            .iter()
            .find(|asset| asset.name == "deblock.toml")
            .unwrap();
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
                let expected = parse_u8s(value);
                let strength = if kind.as_deref() == Some("strong") {
                    2
                } else {
                    1
                };
                let luma = filter_window(samples.unwrap(), qp.unwrap(), strength, false, &tables);
                assert_eq!(luma, expected, "{} luma", name.as_deref().unwrap());
                if strength == 2 {
                    let chroma = filter_window(samples.unwrap(), qp.unwrap(), 2, true, &tables);
                    let weak = filter_window(samples.unwrap(), qp.unwrap(), 1, false, &tables);
                    assert_eq!(
                        chroma,
                        weak,
                        "{} chroma uses weak",
                        name.as_deref().unwrap()
                    );
                }
                seen += 1;
            }
        }
        assert_eq!(seen, 5);
    }

    #[test]
    fn independent_filter_changes_a_coding_block_seam() {
        let mut frame = Frame::filled_420(64, 64, 100).unwrap();
        for y in 0..64 {
            for x in 32..64 {
                frame.y.set(x, y, 140).unwrap();
            }
        }
        let before = frame.y.get(31, 8).unwrap();
        loop_filter_frame(
            &mut frame,
            32,
            &[
                RefCodedBlock {
                    x: 0,
                    y: 0,
                    size: 32,
                    intra: true,
                    coded: true,
                },
                RefCodedBlock {
                    x: 32,
                    y: 0,
                    size: 32,
                    intra: true,
                    coded: true,
                },
                RefCodedBlock {
                    x: 0,
                    y: 32,
                    size: 32,
                    intra: true,
                    coded: true,
                },
                RefCodedBlock {
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
    }

    fn parse_u8s(body: &str) -> [u8; 8] {
        let body = body.trim_end_matches(']');
        let values: Vec<u8> = body
            .split(',')
            .map(|value| value.trim().parse::<u8>().unwrap())
            .collect();
        values.try_into().unwrap()
    }
}

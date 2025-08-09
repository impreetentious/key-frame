use std::sync::OnceLock;

use kf_frame::{Frame, Plane};
use kf_spec::V1_ASSETS;

use crate::PredictError;

/// One reconstructed coding block's contribution to deblock edge metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodedBlock {
    /// Luma origin x, in padded-frame samples.
    pub x: u32,
    /// Luma origin y, in padded-frame samples.
    pub y: u32,
    /// Coding-block side in luma samples (8, 16, 32, or 64).
    pub size: u32,
    /// True when this block used an intra prediction mode.
    pub intra: bool,
    /// True when any derived transform block of this coding block had coefficients.
    pub coded: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EdgeKind {
    CodingBlock,
    DerivedTransform,
}

struct Tables {
    alpha: [u8; 64],
    beta: [u8; 64],
    tc: [u8; 64],
    decisions: Vec<(EdgeKind, bool, bool, u8)>,
}

/// Deblocks a reconstructed 4:2:0 frame in place. The filtered result is the
/// reference image; callers must not keep an unfiltered copy for prediction.
pub fn deblock_frame(frame: &mut Frame, qp: u8, blocks: &[CodedBlock]) -> Result<(), PredictError> {
    if qp > 63 {
        return Err(PredictError::InvalidQp { qp });
    }
    let width = frame.width();
    let height = frame.height();
    let edges = collect_edges(blocks, width, height);
    deblock_plane(&mut frame.y, qp, &edges, false)?;
    let chroma_edges = chroma_edges(&edges);
    deblock_plane(&mut frame.cb, qp, &chroma_edges, true)?;
    deblock_plane(&mut frame.cr, qp, &chroma_edges, true)?;
    Ok(())
}

/// Filters one eight-sample edge according to the frozen weak/strong rules.
#[must_use]
pub fn filter_samples(samples: [u8; 8], qp: u8, strength: u8, chroma: bool) -> [u8; 8] {
    let strength = if chroma && strength == 2 { 1 } else { strength };
    if strength == 0 || qp > 63 {
        return samples;
    }
    let tables = tables();
    let qp = usize::from(qp);
    let p0 = i32::from(samples[3]);
    let p1 = i32::from(samples[2]);
    let q0 = i32::from(samples[4]);
    let q1 = i32::from(samples[5]);
    if p0.abs_diff(q0) >= u32::from(tables.alpha[qp])
        || p1.abs_diff(p0) >= u32::from(tables.beta[qp])
        || q1.abs_diff(q0) >= u32::from(tables.beta[qp])
    {
        return samples;
    }
    if strength == 1 {
        weak(samples, i32::from(tables.tc[qp]))
    } else {
        strong(samples)
    }
}

fn weak(samples: [u8; 8], tc: i32) -> [u8; 8] {
    let p0 = i32::from(samples[3]);
    let p1 = i32::from(samples[2]);
    let q0 = i32::from(samples[4]);
    let q1 = i32::from(samples[5]);
    let delta = ((q0 - p0) * 4 + (p1 - q1) + 4) >> 3;
    let delta = delta.clamp(-tc, tc);
    let mut out = samples;
    out[3] = clip8(p0 + delta);
    out[4] = clip8(q0 - delta);
    out
}

fn strong(samples: [u8; 8]) -> [u8; 8] {
    let p3 = i32::from(samples[0]);
    let p2 = i32::from(samples[1]);
    let p1 = i32::from(samples[2]);
    let p0 = i32::from(samples[3]);
    let q0 = i32::from(samples[4]);
    let q1 = i32::from(samples[5]);
    let q2 = i32::from(samples[6]);
    let q3 = i32::from(samples[7]);
    let mut out = samples;
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

struct Edge {
    vertical: bool,
    line: u32,
    start: u32,
    length: u32,
    kind: EdgeKind,
    intra: bool,
    coded: bool,
}

fn collect_edges(blocks: &[CodedBlock], width: u32, height: u32) -> Vec<Edge> {
    let mut edges = Vec::new();
    for block in blocks {
        if block.size < 8 {
            continue;
        }
        let right = block.x + block.size;
        let bottom = block.y + block.size;
        if right < width {
            let neighbor = block_at(blocks, right, block.y);
            edges.push(Edge {
                vertical: true,
                line: right,
                start: block.y,
                length: block.size,
                kind: EdgeKind::CodingBlock,
                intra: block.intra || neighbor.is_some_and(|other| other.intra),
                coded: block.coded || neighbor.is_some_and(|other| other.coded),
            });
        }
        if bottom < height {
            let neighbor = block_at(blocks, block.x, bottom);
            edges.push(Edge {
                vertical: false,
                line: bottom,
                start: block.x,
                length: block.size,
                kind: EdgeKind::CodingBlock,
                intra: block.intra || neighbor.is_some_and(|other| other.intra),
                coded: block.coded || neighbor.is_some_and(|other| other.coded),
            });
        }
        if block.size == 64 {
            let mid_x = block.x + 32;
            let mid_y = block.y + 32;
            if mid_x < width {
                edges.push(Edge {
                    vertical: true,
                    line: mid_x,
                    start: block.y,
                    length: 64,
                    kind: EdgeKind::DerivedTransform,
                    intra: block.intra,
                    coded: block.coded,
                });
            }
            if mid_y < height {
                edges.push(Edge {
                    vertical: false,
                    line: mid_y,
                    start: block.x,
                    length: 64,
                    kind: EdgeKind::DerivedTransform,
                    intra: block.intra,
                    coded: block.coded,
                });
            }
        }
    }
    edges
}

fn chroma_edges(edges: &[Edge]) -> Vec<Edge> {
    edges
        .iter()
        .filter(|edge| {
            edge.length >= 16 && edge.line.is_multiple_of(2) && edge.start.is_multiple_of(2)
        })
        .map(|edge| Edge {
            vertical: edge.vertical,
            line: edge.line / 2,
            start: edge.start / 2,
            length: edge.length / 2,
            kind: edge.kind,
            intra: edge.intra,
            coded: edge.coded,
        })
        .filter(|edge| edge.length >= 8)
        .collect()
}

fn block_at(blocks: &[CodedBlock], x: u32, y: u32) -> Option<CodedBlock> {
    blocks.iter().copied().find(|block| {
        x >= block.x && x < block.x + block.size && y >= block.y && y < block.y + block.size
    })
}

fn deblock_plane(
    plane: &mut Plane,
    qp: u8,
    edges: &[Edge],
    chroma: bool,
) -> Result<(), PredictError> {
    let mut vertical: Vec<&Edge> = edges.iter().filter(|edge| edge.vertical).collect();
    let mut horizontal: Vec<&Edge> = edges.iter().filter(|edge| !edge.vertical).collect();
    vertical.sort_by_key(|edge| (edge.line, edge.start));
    horizontal.sort_by_key(|edge| (edge.line, edge.start));
    for edge in vertical {
        apply_edge(plane, qp, edge, chroma)?;
    }
    for edge in horizontal {
        apply_edge(plane, qp, edge, chroma)?;
    }
    Ok(())
}

fn apply_edge(plane: &mut Plane, qp: u8, edge: &Edge, chroma: bool) -> Result<(), PredictError> {
    let strength = lookup_strength(edge.kind, edge.intra, edge.coded);
    if strength == 0 {
        return Ok(());
    }
    let mut offset = 0_u32;
    while offset < edge.length {
        let run = 8.min(edge.length - offset);
        if run < 8 {
            break;
        }
        for step in 0..8 {
            let samples = if edge.vertical {
                read_vertical(plane, edge.line, edge.start + offset + step)?
            } else {
                read_horizontal(plane, edge.line, edge.start + offset + step)?
            };
            let filtered = filter_samples(samples, qp, strength, chroma);
            if edge.vertical {
                write_vertical(plane, edge.line, edge.start + offset + step, filtered)?;
            } else {
                write_horizontal(plane, edge.line, edge.start + offset + step, filtered)?;
            }
        }
        offset += 8;
    }
    Ok(())
}

fn read_vertical(plane: &Plane, x: u32, y: u32) -> Result<[u8; 8], PredictError> {
    Ok([
        plane.get(x.saturating_sub(4), y)?,
        plane.get(x.saturating_sub(3), y)?,
        plane.get(x.saturating_sub(2), y)?,
        plane.get(x.saturating_sub(1), y)?,
        plane.get(x, y)?,
        plane.get(x.saturating_add(1), y)?,
        plane.get(x.saturating_add(2), y)?,
        plane.get(x.saturating_add(3), y)?,
    ])
}

fn write_vertical(plane: &mut Plane, x: u32, y: u32, samples: [u8; 8]) -> Result<(), PredictError> {
    plane.set(x.saturating_sub(3), y, samples[1])?;
    plane.set(x.saturating_sub(2), y, samples[2])?;
    plane.set(x.saturating_sub(1), y, samples[3])?;
    plane.set(x, y, samples[4])?;
    plane.set(x.saturating_add(1), y, samples[5])?;
    plane.set(x.saturating_add(2), y, samples[6])?;
    Ok(())
}

fn read_horizontal(plane: &Plane, y: u32, x: u32) -> Result<[u8; 8], PredictError> {
    Ok([
        plane.get(x, y.saturating_sub(4))?,
        plane.get(x, y.saturating_sub(3))?,
        plane.get(x, y.saturating_sub(2))?,
        plane.get(x, y.saturating_sub(1))?,
        plane.get(x, y)?,
        plane.get(x, y.saturating_add(1))?,
        plane.get(x, y.saturating_add(2))?,
        plane.get(x, y.saturating_add(3))?,
    ])
}

fn write_horizontal(
    plane: &mut Plane,
    y: u32,
    x: u32,
    samples: [u8; 8],
) -> Result<(), PredictError> {
    plane.set(x, y.saturating_sub(3), samples[1])?;
    plane.set(x, y.saturating_sub(2), samples[2])?;
    plane.set(x, y.saturating_sub(1), samples[3])?;
    plane.set(x, y, samples[4])?;
    plane.set(x, y.saturating_add(1), samples[5])?;
    plane.set(x, y.saturating_add(2), samples[6])?;
    Ok(())
}

fn lookup_strength(kind: EdgeKind, intra: bool, coded: bool) -> u8 {
    let tables = tables();
    tables
        .decisions
        .iter()
        .find(|(row_kind, row_intra, row_coded, _)| {
            *row_kind == kind && *row_intra == intra && *row_coded == coded
        })
        .or_else(|| {
            tables
                .decisions
                .iter()
                .find(|(row_kind, row_intra, row_coded, _)| {
                    *row_kind == EdgeKind::CodingBlock && *row_intra == intra && *row_coded == coded
                })
        })
        .map(|(_, _, _, strength)| *strength)
        .unwrap_or(0)
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let asset = V1_ASSETS
            .iter()
            .find(|asset| asset.name == "deblock.toml")
            .expect("invariant: kf-spec exposes deblock.toml");
        Tables {
            alpha: parse_u8_array(asset.contents, "alpha = ["),
            beta: parse_u8_array(asset.contents, "beta = ["),
            tc: parse_u8_array(asset.contents, "tc = ["),
            decisions: parse_decisions(asset.contents),
        }
    })
}

fn parse_u8_array(contents: &str, prefix: &str) -> [u8; 64] {
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

fn parse_decisions(contents: &str) -> Vec<(EdgeKind, bool, bool, u8)> {
    let mut rows = Vec::new();
    let mut kind = None;
    let mut intra = None;
    let mut coded = None;
    for line in contents.lines() {
        if line == "[[decision]]" {
            kind = None;
            intra = None;
            coded = None;
        } else if let Some(value) = line.strip_prefix("edge_kind = \"") {
            kind = Some(match value.trim_end_matches('"') {
                "coding_block" => EdgeKind::CodingBlock,
                _ => EdgeKind::DerivedTransform,
            });
        } else if let Some(value) = line.strip_prefix("either_intra = ") {
            intra = Some(value == "true");
        } else if let Some(value) = line.strip_prefix("either_coded = ") {
            coded = Some(value == "true");
        } else if let Some(value) = line.strip_prefix("strength = ") {
            if let (Some(kind), Some(intra), Some(coded)) = (kind, intra, coded) {
                rows.push((
                    kind,
                    intra,
                    coded,
                    value.parse::<u8>().expect("invariant: strength is u8"),
                ));
            }
        }
    }
    rows
}

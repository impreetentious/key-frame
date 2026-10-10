use kf_spec::V1_ASSETS;

use crate::{
    ReferenceError,
    coverage::ReferenceElement,
    motion::{RefMotionVector, RefReference},
    range::ReferenceRange,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RefIntraMode {
    Dc,
    Planar,
    Horizontal,
    Vertical,
    D45,
    D135,
    D117,
    D153,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RefBlock {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) size: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RefPrediction {
    Intra(RefIntraMode),
    Skip(RefReference),
    Inter {
        reference: RefReference,
        mvd: RefMotionVector,
    },
}

pub(crate) fn read_partition(
    range: &mut ReferenceRange<'_>,
    x: u32,
    y: u32,
) -> Result<Vec<RefBlock>, ReferenceError> {
    let mut blocks = Vec::new();
    read_node(range, x, y, 64, 0, &mut blocks)?;
    Ok(blocks)
}

fn read_node(
    range: &mut ReferenceRange<'_>,
    x: u32,
    y: u32,
    size: u32,
    depth: u16,
    blocks: &mut Vec<RefBlock>,
) -> Result<(), ReferenceError> {
    if size == 8 {
        blocks.push(RefBlock { x, y, size });
        return Ok(());
    }
    range.element(ReferenceElement::PartitionSplit);
    if !range.context(depth * 3)? {
        blocks.push(RefBlock { x, y, size });
        return Ok(());
    }
    let half = size / 2;
    for (dx, dy) in [(0, 0), (half, 0), (0, half), (half, half)] {
        read_node(range, x + dx, y + dy, half, depth + 1, blocks)?;
    }
    Ok(())
}

pub(crate) fn read_intra_mode(
    range: &mut ReferenceRange<'_>,
) -> Result<RefIntraMode, ReferenceError> {
    range.element(ReferenceElement::IntraModeTree);
    let mut index = 0_u8;
    for context in [18, 21, 22] {
        index = (index << 1) | u8::from(range.context(context)?);
    }
    match index {
        0 => Ok(RefIntraMode::Dc),
        1 => Ok(RefIntraMode::Planar),
        2 => Ok(RefIntraMode::Horizontal),
        3 => Ok(RefIntraMode::Vertical),
        4 => Ok(RefIntraMode::D45),
        5 => Ok(RefIntraMode::D135),
        6 => Ok(RefIntraMode::D117),
        7 => Ok(RefIntraMode::D153),
        _ => Err(ReferenceError::new(0, "intra.mode")),
    }
}

pub(crate) fn read_prediction(
    range: &mut ReferenceRange<'_>,
    key: bool,
) -> Result<RefPrediction, ReferenceError> {
    if key {
        return Ok(RefPrediction::Intra(read_intra_mode(range)?));
    }
    range.element(ReferenceElement::SkipFlag);
    if range.context(12)? {
        return Ok(RefPrediction::Skip(read_reference(range)?));
    }
    range.element(ReferenceElement::InterFlag);
    if !range.context(15)? {
        return Ok(RefPrediction::Intra(read_intra_mode(range)?));
    }
    let reference = read_reference(range)?;
    range.element(ReferenceElement::MotionDifference);
    Ok(RefPrediction::Inter {
        reference,
        mvd: RefMotionVector {
            x_q4: i32::from(read_mvd_component(range, 30)?),
            y_q4: i32::from(read_mvd_component(range, 33)?),
        },
    })
}

fn read_reference(range: &mut ReferenceRange<'_>) -> Result<RefReference, ReferenceError> {
    range.element(ReferenceElement::ReferenceSelect);
    Ok(if range.context(28)? {
        RefReference::Golden
    } else {
        RefReference::Last
    })
}

fn read_mvd_component(
    range: &mut ReferenceRange<'_>,
    context_base: u16,
) -> Result<i16, ReferenceError> {
    let mut prefix = 0_u32;
    loop {
        let terminal = if prefix < 3 {
            range.context(context_base + u16::try_from(prefix).unwrap())?
        } else {
            range.bypass()?
        };
        if terminal {
            break;
        }
        prefix += 1;
        if prefix > 10 {
            return Err(ReferenceError::new(0, "prediction.mvd_prefix"));
        }
    }
    let mut code_number = 1_u32 << prefix;
    for shift in (0..prefix).rev() {
        code_number |= u32::from(range.bypass()?) << shift;
    }
    let magnitude = code_number - 1;
    if magnitude > 512 {
        return Err(ReferenceError::new(0, "prediction.mvd_magnitude"));
    }
    if magnitude == 0 {
        return Ok(0);
    }
    let magnitude =
        i16::try_from(magnitude).map_err(|_| ReferenceError::new(0, "prediction.mvd_magnitude"))?;
    Ok(if range.bypass()? {
        -magnitude
    } else {
        magnitude
    })
}

pub(crate) fn read_coefficients(
    range: &mut ReferenceRange<'_>,
    chroma: bool,
    size: u32,
) -> Result<Vec<i32>, ReferenceError> {
    let group = size_group(size)?;
    let side = usize::try_from(size).map_err(|_| ReferenceError::new(0, "transform.size"))?;
    let mut levels = vec![0_i32; side * side];
    let presence_context = 36 + if chroma { 4 } else { 0 } + group;
    range.element(ReferenceElement::CoefficientPresence);
    if !range.context(presence_context)? {
        return Ok(levels);
    }
    let last_x = read_position(range, false, size, group)?;
    let last_y = read_position(range, true, size, group)?;
    if last_x >= side || last_y >= side {
        return Err(ReferenceError::new(0, "coefficient.last_position"));
    }
    let last_index = last_y * side + last_x;
    let scan = literal_scan(size)?;
    let last_scan = scan
        .iter()
        .position(|&index| index == last_index)
        .ok_or_else(|| ReferenceError::new(0, "coefficient.last_position"))?;
    let mut nonzero_count = 0_u16;
    for (position, &index) in scan.iter().enumerate().take(last_scan + 1) {
        let significant = if position == last_scan {
            true
        } else {
            range.element(ReferenceElement::Significance);
            range.context(76 + group * 11 + nonzero_count.min(10))?
        };
        if significant {
            levels[index] = read_level(range, group, nonzero_count)?;
            nonzero_count = nonzero_count.saturating_add(1);
        }
    }
    if nonzero_count == 0 {
        return Err(ReferenceError::new(0, "coefficient.empty_nonzero_block"));
    }
    Ok(levels)
}

fn read_position(
    range: &mut ReferenceRange<'_>,
    vertical: bool,
    size: u32,
    group: u16,
) -> Result<usize, ReferenceError> {
    range.element(if vertical {
        ReferenceElement::LastRow
    } else {
        ReferenceElement::LastColumn
    });
    let bits = size.ilog2();
    let base = (if vertical { 60 } else { 44 }) + group * 4;
    let mut value = 0_usize;
    for bit_index in 0..bits {
        let bit = if bit_index < 4 {
            range.context(base + u16::try_from(bit_index).unwrap())?
        } else {
            range.bypass()?
        };
        value = (value << 1) | usize::from(bit);
    }
    Ok(value)
}

fn read_level(
    range: &mut ReferenceRange<'_>,
    group: u16,
    nonzero_count: u16,
) -> Result<i32, ReferenceError> {
    range.element(ReferenceElement::GreaterThanOne);
    let gt1 = range.context(120 + group * 4 + nonzero_count.min(3))?;
    let magnitude = if !gt1 {
        1
    } else {
        range.element(ReferenceElement::GreaterThanTwo);
        if range.context(136 + group * 2 + nonzero_count.min(1))? {
            read_unsigned(range)?
                .checked_add(3)
                .ok_or_else(|| ReferenceError::new(0, "coefficient.level"))?
        } else {
            2
        }
    };
    if magnitude > crate::limits().coefficient_abs_max {
        return Err(ReferenceError::new(0, "coefficient.level_cap"));
    }
    let magnitude =
        i32::try_from(magnitude).map_err(|_| ReferenceError::new(0, "coefficient.level"))?;
    range.element(ReferenceElement::LevelSign);
    Ok(if range.bypass()? {
        -magnitude
    } else {
        magnitude
    })
}

fn read_unsigned(range: &mut ReferenceRange<'_>) -> Result<u32, ReferenceError> {
    range.element(ReferenceElement::MagnitudeRemainder);
    let mut prefix = 0_u32;
    while !range.bypass()? {
        prefix += 1;
        if prefix > 15 {
            return Err(ReferenceError::new(0, "coefficient.remainder_prefix"));
        }
    }
    let mut code_number = 1_u32 << prefix;
    for shift in (0..prefix).rev() {
        code_number |= u32::from(range.bypass()?) << shift;
    }
    Ok(code_number - 1)
}

fn size_group(size: u32) -> Result<u16, ReferenceError> {
    match size {
        4 => Ok(0),
        8 => Ok(1),
        16 => Ok(2),
        32 => Ok(3),
        _ => Err(ReferenceError::new(0, "transform.size")),
    }
}

fn literal_scan(size: u32) -> Result<Vec<usize>, ReferenceError> {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "scans.toml")
        .expect("invariant: kf-spec exposes scans.toml");
    let prefix = format!("n{size} = [");
    let line = asset
        .contents
        .lines()
        .find(|line| line.starts_with(&prefix))
        .ok_or_else(|| ReferenceError::new(0, "scan.size"))?;
    let coordinates: Vec<usize> = line
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| ReferenceError::new(0, "scan.asset"))?
        .split(',')
        .map(|value| {
            value
                .trim()
                .parse::<usize>()
                .map_err(|_| ReferenceError::new(0, "scan.asset"))
        })
        .collect::<Result<_, _>>()?;
    let side = usize::try_from(size).map_err(|_| ReferenceError::new(0, "scan.size"))?;
    Ok(coordinates
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| pair[1] * side + pair[0])
        .collect())
}

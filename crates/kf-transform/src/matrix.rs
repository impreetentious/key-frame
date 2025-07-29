use std::sync::OnceLock;

use kf_spec::V1_ASSETS;

use crate::TransformError;

/// Closed set of version-one square transform sizes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransformSize {
    N4,
    N8,
    N16,
    N32,
}

impl TransformSize {
    /// Side length in samples.
    #[must_use]
    pub const fn side(self) -> usize {
        match self {
            Self::N4 => 4,
            Self::N8 => 8,
            Self::N16 => 16,
            Self::N32 => 32,
        }
    }

    /// Decoder-normative second inverse-stage shift.
    #[must_use]
    pub const fn inverse_second_shift(self) -> u8 {
        match self {
            Self::N4 => 11,
            Self::N8 => 10,
            Self::N16 => 9,
            Self::N32 => 8,
        }
    }

    pub(crate) fn matrix(self) -> &'static [i16] {
        static N4: OnceLock<Vec<i16>> = OnceLock::new();
        static N8: OnceLock<Vec<i16>> = OnceLock::new();
        static N16: OnceLock<Vec<i16>> = OnceLock::new();
        static N32: OnceLock<Vec<i16>> = OnceLock::new();
        match self {
            Self::N4 => N4.get_or_init(|| parse_matrix(4)).as_slice(),
            Self::N8 => N8.get_or_init(|| parse_matrix(8)).as_slice(),
            Self::N16 => N16.get_or_init(|| parse_matrix(16)).as_slice(),
            Self::N32 => N32.get_or_init(|| parse_matrix(32)).as_slice(),
        }
    }
}

fn parse_matrix(size: u8) -> Vec<i16> {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "transforms.toml")
        .expect("invariant: kf-spec exposes transforms.toml");
    let prefix = format!("n{size} = [");
    let line = asset
        .contents
        .lines()
        .find(|line| line.starts_with(&prefix))
        .expect("invariant: checked transform asset has every v1 size");
    let body = line
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(']'))
        .expect("invariant: checked matrix array has balanced brackets");
    let values: Vec<i16> = body
        .split(',')
        .map(|value| {
            value
                .trim()
                .parse::<i16>()
                .expect("invariant: checked matrix coefficient is i16")
        })
        .collect();
    let expected = usize::from(size) * usize::from(size);
    assert_eq!(
        values.len(),
        expected,
        "invariant: checked transform matrix is square"
    );
    values
}

impl TryFrom<u8> for TransformSize {
    type Error = TransformError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            4 => Ok(Self::N4),
            8 => Ok(Self::N8),
            16 => Ok(Self::N16),
            32 => Ok(Self::N32),
            size => Err(TransformError::InvalidSpecMatrix { size }),
        }
    }
}

use std::sync::OnceLock;

use kf_spec::V1_ASSETS;

use crate::{Probability, RangeError};

/// Returns the frozen Q16 negative-log cost for one binary decision.
pub fn modeled_cost_q16(symbol: bool, probability: Probability) -> Result<u32, RangeError> {
    let (cost0, cost1) = cost_tables();
    let index = usize::from(probability.p1() - 1);
    Ok(if symbol { cost1[index] } else { cost0[index] })
}

fn cost_tables() -> &'static (Vec<u32>, Vec<u32>) {
    static TABLES: OnceLock<(Vec<u32>, Vec<u32>)> = OnceLock::new();
    TABLES.get_or_init(|| {
        let asset = V1_ASSETS
            .iter()
            .find(|asset| asset.name == "costs.toml")
            .expect("invariant: kf-spec exposes costs.toml");
        let cost0 = parse_array(asset.contents, "cost0_q16")
            .expect("invariant: checked cost0 asset has 4095 u32 rows");
        let cost1 = parse_array(asset.contents, "cost1_q16")
            .expect("invariant: checked cost1 asset has 4095 u32 rows");
        (cost0, cost1)
    })
}

fn parse_array(contents: &str, name: &'static str) -> Result<Vec<u32>, RangeError> {
    let prefix = format!("{name} = [");
    let line = contents
        .lines()
        .find(|line| line.starts_with(&prefix))
        .ok_or(RangeError::InvalidSpecAsset {
            asset: "costs.toml",
        })?;
    let body = line
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(']'))
        .ok_or(RangeError::InvalidSpecAsset {
            asset: "costs.toml",
        })?;
    let values: Result<Vec<_>, _> = body
        .split(',')
        .map(|value| value.trim().parse::<u32>())
        .collect();
    let values = values.map_err(|_| RangeError::InvalidSpecAsset {
        asset: "costs.toml",
    })?;
    if values.len() != 4095 {
        return Err(RangeError::InvalidSpecAsset {
            asset: "costs.toml",
        });
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::modeled_cost_q16;
    use crate::Probability;

    #[test]
    fn half_probability_costs_one_bit() {
        let half = Probability::new(2048).unwrap();
        assert_eq!(modeled_cost_q16(false, half).unwrap(), 65_536);
        assert_eq!(modeled_cost_q16(true, half).unwrap(), 65_536);
    }

    #[test]
    fn asymmetric_cost_favors_the_likely_symbol() {
        let likely_one = Probability::new(3072).unwrap();
        assert!(
            modeled_cost_q16(true, likely_one).unwrap()
                < modeled_cost_q16(false, likely_one).unwrap()
        );
    }
}

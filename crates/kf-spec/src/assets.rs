/// A named, inert specification asset embedded byte-for-byte at compile time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Asset {
    /// Repository-relative asset name within `spec/v1`.
    pub name: &'static str,
    /// Complete UTF-8 contents of the asset.
    pub contents: &'static str,
}

/// The closed Key Frame v1 asset set.
pub const V1_ASSETS: &[Asset] = &[
    Asset {
        name: "constants.toml",
        contents: include_str!("../../../spec/v1/constants.toml"),
    },
    Asset {
        name: "fields.toml",
        contents: include_str!("../../../spec/v1/fields.toml"),
    },
    Asset {
        name: "contexts.toml",
        contents: include_str!("../../../spec/v1/contexts.toml"),
    },
    Asset {
        name: "syntax.toml",
        contents: include_str!("../../../spec/v1/syntax.toml"),
    },
    Asset {
        name: "intra.toml",
        contents: include_str!("../../../spec/v1/intra.toml"),
    },
    Asset {
        name: "mc.toml",
        contents: include_str!("../../../spec/v1/mc.toml"),
    },
    Asset {
        name: "deblock.toml",
        contents: include_str!("../../../spec/v1/deblock.toml"),
    },
    Asset {
        name: "search.toml",
        contents: include_str!("../../../spec/v1/search.toml"),
    },
    Asset {
        name: "scans.toml",
        contents: include_str!("../../../spec/v1/scans.toml"),
    },
    Asset {
        name: "transforms.toml",
        contents: include_str!("../../../spec/v1/transforms.toml"),
    },
    Asset {
        name: "quant.toml",
        contents: include_str!("../../../spec/v1/quant.toml"),
    },
    Asset {
        name: "costs.toml",
        contents: include_str!("../../../spec/v1/costs.toml"),
    },
    Asset {
        name: "vectors.json",
        contents: include_str!("../../../spec/v1/vectors.json"),
    },
];

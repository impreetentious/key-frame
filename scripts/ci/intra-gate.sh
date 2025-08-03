#!/usr/bin/env bash

set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"

cargo test --locked -p kf-predict --test intra --test padding
cargo test --locked -p kf-dec --test oracle
cargo test --locked -p kf-enc --test intra_roundtrip
cargo test --locked -p kf-tools --test y4m --test cli_roundtrip

echo "intra-gate: OK — partition/mode RDO, dual decoders, five QPs, replay accounting, Y4M, and CLIs"

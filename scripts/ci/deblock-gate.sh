#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo test --locked -p kf-predict --test deblock
cargo test --locked -p kf-ref deblock
cargo test --locked -p kf-enc --test loopfilter
cargo run --locked --quiet -p kf-tools --example generate_staging -- --check

echo "deblock-gate: OK — asset replay, independent filter, reference-slot identity, and filtered staging hashes"

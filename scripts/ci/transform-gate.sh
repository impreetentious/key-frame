#!/usr/bin/env bash

set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"

python3 spec/check_assets.py
cargo test --locked -p kf-transform

echo "transform-gate: OK — 24 literal vectors, stage extremes, stage widths, quant caps, and QP bounds"

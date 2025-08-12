#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo test --locked -p kf-fuzz
cargo run --release --locked --quiet -p kf-fuzz --bin kf-fuzz-campaign -- --iterations 2048

echo "fuzz-gate: OK — four deterministic decoder campaigns, smoke plus release replay"

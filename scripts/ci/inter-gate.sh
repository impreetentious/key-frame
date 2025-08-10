#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo test -p kf-predict --test inter --test motion_field --test padding
cargo test -p kf-enc motion_search
cargo test -p kf-enc --test gop --test inter_roundtrip --test context_lockstep --test loopfilter
cargo run -q -p kf-tools --example generate_staging -- --check

echo "inter-gate: OK — MC/MVP, ME, GOP policy, dual decoders, refresh state, P staging, context lockstep, and loop-filter identity"

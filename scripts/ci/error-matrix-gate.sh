#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo test --locked -p kf-tools --test error_matrix
cargo test --locked -p kf-bitstream --test resync --test packet_vectors
cargo test --locked -p kf-enc --test context_traps

echo "error-matrix-gate: OK — truncation, header and payload damage, false sync, index gaps, leading loss, flag rules, and recovery in both decoders"

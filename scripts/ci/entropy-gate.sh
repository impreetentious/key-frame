#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo test --locked -p kf-range --test coverage --test context_lockstep
cargo test --locked -p kf-bitstream --test coverage_syntax
cargo test --locked -p kf-enc --test context_lockstep --test context_traps
python3 spec/generate_docs.py --check

echo "entropy-gate: OK — 144-id coverage, SB/frame lockstep, carry, transaction, and generated-document check"

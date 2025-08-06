#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo test --locked -p kf-range --test coverage --test context_lockstep
cargo test --locked -p kf-bitstream --test coverage_syntax --test syntax_coverage
cargo test --locked -p kf-enc --test context_lockstep --test context_traps
python3 spec/generate_docs.py --check
python3 "$repo_root/scripts/ci/check-syntax-coverage.py"

echo "entropy-gate: OK — 144-id coverage inventory, SB/frame lockstep, carry, transaction, and generated-document check"

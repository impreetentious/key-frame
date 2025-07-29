#!/usr/bin/env bash

set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"

cargo test --locked -p kf-range \
  --test oracle_vectors \
  --test exhaustive \
  --test entropy_sanity \
  --test context_lockstep

echo "range-gate: OK — oracle, 2^16 alphabets, entropy sanity, and context lockstep"

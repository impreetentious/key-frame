#!/usr/bin/env bash

set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"

cargo test --locked -p kf-bitstream \
  --test partition_syntax \
  --test prediction_syntax \
  --test coefficient_syntax \
  --test syntax_properties \
  --test rdo_snapshot \
  --test coverage_syntax \
  --test syntax_coverage

echo "syntax-gate: OK — partitions, key/P branches, MV bins, scans, levels, truncation, search snapshots, and context coverage"

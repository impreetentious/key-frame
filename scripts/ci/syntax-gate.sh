#!/usr/bin/env bash

set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"

cargo test --locked -p kf-bitstream \
  --test partition_syntax \
  --test prediction_syntax \
  --test coefficient_syntax \
  --test syntax_properties \
  --test rdo_snapshot

echo "syntax-gate: OK — partitions, key/P branches, MV bins, scans, levels, truncation, and search snapshots"

#!/usr/bin/env bash
#
# The measurement gate.
#
# Everything the repository publishes about compression efficiency is a number
# this tooling produced, so the tooling has to be held to the same standard as
# the codec: a second implementation that agrees, and a receipt that fails
# loudly when it stops reproducing.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# The committed vectors against a fresh derivation by the independent oracle.
python3 bench/metric_oracle.py --check

# The Rust against those same vectors: separable passes against direct 2D
# convolution, closed-form antiderivative against Gauss-Legendre quadrature.
cargo test --locked -p kf-tools --lib
cargo test --locked -p kf-tools --test metric_vectors --test metric_cli

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

# An end-to-end receipt over a real encode, not a synthetic pair: the point of
# `repro` is that it re-runs the measurement, so the gate has to give it
# something that took work to produce.
cargo run --release --locked --quiet -p kf-tools --example make_clip -- \
  --output "$work_dir/source.y4m" --width 64 --height 64 --frames 4 --pattern motion
cargo run --release --locked --quiet -p kf-tools --bin kfenc -- \
  --input "$work_dir/source.y4m" --qp 32 --output "$work_dir/coded.kfv"
cargo run --release --locked --quiet -p kf-tools --bin kfdec -- \
  "$work_dir/coded.kfv" --output "$work_dir/decoded.y4m"

for metric in psnr ssim; do
  cargo run --release --locked --quiet -p kf-tools --bin kfmetric -- \
    "$metric" "$work_dir/source.y4m" "$work_dir/decoded.y4m" > "$work_dir/$metric.json"
  cargo run --release --locked --quiet -p kf-tools --bin kfmetric -- \
    repro "$work_dir/$metric.json" > /dev/null
done

# A receipt that cannot fail is not a receipt. Move one figure and the same
# command has to reject it.
python3 - "$work_dir/psnr.json" "$work_dir/tampered.json" <<'PY'
import json
from pathlib import Path
import sys

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
report["global"] = (report["global"] or 0.0) + 1.0
Path(sys.argv[2]).write_text(json.dumps(report), encoding="utf-8")
PY

if cargo run --release --locked --quiet -p kf-tools --bin kfmetric -- \
     repro "$work_dir/tampered.json" > /dev/null 2>&1; then
  echo "metric-gate: FAILED — an edited figure reproduced"
  exit 1
fi

echo "metric-gate: OK — independent oracle, vector replay, and a receipt that rejects an edit"

#!/usr/bin/env bash

set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"

probe_tmp_dir="$(mktemp -d)"
trap 'rm -rf "$probe_tmp_dir"' EXIT

python3 - "$probe_tmp_dir/oracle.kfv" <<'PY'
import json
from pathlib import Path
import sys

vectors = json.loads(Path("spec/v1/vectors.json").read_text(encoding="utf-8"))
Path(sys.argv[1]).write_bytes(bytes.fromhex(vectors["syntax"]["complete_stream_hex"]))
PY

cargo run --locked --quiet -p kf-tools --bin kfprobe -- \
  "$probe_tmp_dir/oracle.kfv" > "$probe_tmp_dir/report.json"
python3 spec/validate_probe.py \
  spec/v1/probe.schema.json "$probe_tmp_dir/report.json"
cargo test --locked -p kf-tools --test probe

echo "probe-gate: OK — frozen schema, replay accounting, canonical match, and noncanonical tail"

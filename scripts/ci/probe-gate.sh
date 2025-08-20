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
cargo test --locked -p kf-probe --test frames

# A later frame must report itself, not the keyframe the probe re-entered at.
# The three-frame encoder vector is the only committed stream deep enough to
# tell the difference, and its last frame is a P frame.
cargo run --locked --quiet -p kf-tools --bin kfprobe -- \
  conformance/encoder/inter_motion64_qp32.kfv --frame 2 > "$probe_tmp_dir/frame2.json"
python3 spec/validate_probe.py \
  spec/v1/probe.schema.json "$probe_tmp_dir/frame2.json"
python3 - "$probe_tmp_dir/frame2.json" <<'CHECK'
import json
from pathlib import Path
import sys

frame = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))["frame"]
if frame["frame_index"] != 2:
    raise SystemExit("probe --frame reported frame %s" % frame["frame_index"])
if frame["flags"]["key"]:
    raise SystemExit("frame 2 of the inter vector should not be a keyframe")
if not frame["canonical_payload_match"]:
    raise SystemExit("the encoder's own payload must replay canonically")
CHECK

# Asking for a frame the stream does not have is an error, not the nearest one.
if cargo run --locked --quiet -p kf-tools --bin kfprobe -- \
     conformance/encoder/inter_motion64_qp32.kfv --frame 3 > /dev/null 2>&1; then
  echo "probe-gate: FAILED — probing past the last frame succeeded"
  exit 1
fi

echo "probe-gate: OK — frozen schema, replay accounting, canonical match, noncanonical tail, and per-frame re-entry"

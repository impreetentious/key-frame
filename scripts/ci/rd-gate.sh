#!/usr/bin/env bash
#
# The measurement-receipt gate.
#
# `bench/results/rd-campaign.json` holds every rate-distortion number this
# repository publishes, and the rate-distortion tab of the projection room
# draws exactly that file. This checks that the numbers still come out of the
# encoder that is checked in — a chart describing a build that no longer exists
# is worse than no chart, because it looks like evidence.
#
# It also checks the shipping curves against the previous release. An encoder
# change that costs more than the allowance has to be argued for in the change
# that makes it, rather than absorbed quietly across releases until nobody can
# say when the codec got worse.
#
# What is re-encoded here on every run is the ends of each quality ladder,
# across every clip and every toolset. That is a deliberate sample and it is
# named in the output rather than glossed: re-encoding all sixty points takes
# minutes, and a gate slow enough to be skipped protects nothing. The nightly
# job runs the whole receipt.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

receipts="bench/results/rd-campaign.json"
baseline="bench/results/rd-baseline.json"

if [[ ! -f "$receipts" ]]; then
  echo "rd-gate: FAILED — $receipts is missing; the site would have nothing to draw"
  exit 1
fi
if [[ ! -f "$baseline" ]]; then
  echo "rd-gate: FAILED — $baseline is missing; a regression would have nothing to be measured against"
  exit 1
fi

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

# Every ablation the encoder knows has to be a curve in the receipt, or the
# charts page would quietly show fewer tools than the encoder actually has.
cargo test --locked -p kf-enc --test toolset --release

# The receipt's shape: a full ladder on every curve, and average-bitrate targets
# that are structurally not rate-quality points.
cargo test --locked -p kf-tools --test receipt_shape --release

"$repo_root/scripts/fetch-corpus.sh"

python3 - "$receipts" <<'SHAPE'
import json
from pathlib import Path
import sys

receipt = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
if receipt.get("format") != "key-frame-rd-campaign-v1":
    raise SystemExit("rd-gate: the receipt is not a campaign receipt")

curves = receipt["curves"]
clips = {curve["clip"] for curve in curves}
toolsets = {curve["toolset"] for curve in curves}
ladder = receipt["settings"]["qp_ladder"]

if len(curves) != len(clips) * len(toolsets):
    raise SystemExit("rd-gate: the receipt is not a full clip-by-toolset grid")
if "full" not in toolsets:
    raise SystemExit("rd-gate: there is no baseline curve to compare anything against")

targets = sum(len(entry["targets"]) for entry in receipt["rate_control"]["clips"])
print(
    "rd-gate: receipt covers %d clip(s) x %d toolset(s) at %d QPs, plus %d average-bitrate targets"
    % (len(clips), len(toolsets), len(ladder), targets)
)
SHAPE

# Re-encode the ends of every ladder and compare every recorded figure. This
# also prints each ablation's bitrate difference — the number the charts page
# draws and the one a reader is most likely to quote — and checks the shipping
# curves against the last release.
cargo run --release --locked --quiet -p kf-tools --example rd_verify -- \
  --receipts "$receipts" --quick --baseline "$baseline"

# A regression check that cannot fail is decoration. This hands the verifier a
# baseline whose shipping rates are ten percent lower than the committed one —
# which is a ten percent regression by construction — and requires a refusal.
python3 - "$baseline" "$work_dir/worse.json" <<'WORSE'
import json
from pathlib import Path
import sys

receipt = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
for curve in receipt["curves"]:
    if curve["toolset"] == "full":
        for point in curve["points"]:
            point["rate"] *= 0.9
Path(sys.argv[2]).write_text(json.dumps(receipt), encoding="utf-8")
WORSE

if cargo run --release --locked --quiet -p kf-tools --example rd_verify -- \
     --receipts "$receipts" --quick --baseline "$work_dir/worse.json" > /dev/null 2>&1; then
  echo "rd-gate: FAILED — a ten percent regression against the baseline was accepted"
  exit 1
fi

echo "rd-gate: OK — receipts reproduce, the shipping curves hold against the baseline, and a regression is refused"

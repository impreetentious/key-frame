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
# What is checked here on every run is the ends of each quality ladder, across
# every clip and every toolset. That is a deliberate sample and it is named in
# the output rather than glossed: re-encoding all sixty points takes minutes,
# and a gate slow enough to be skipped protects nothing. The nightly job runs
# the whole receipt.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

receipts="bench/results/rd-campaign.json"

if [[ ! -f "$receipts" ]]; then
  echo "rd-gate: FAILED — $receipts is missing; the site would have nothing to draw"
  exit 1
fi

# Every ablation the encoder knows has to be a curve in the receipt, or the
# charts page would quietly show fewer tools than the encoder actually has.
cargo test --locked -p kf-enc --test toolset --release

"$repo_root/scripts/fetch-corpus.sh"

python3 - "$receipts" <<'PY'
import json
from pathlib import Path
import sys

receipt = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
if receipt.get("format") != "key-frame-rd-campaign-v1":
    raise SystemExit("rd-gate: the receipt is not a campaign receipt")

curves = receipt["curves"]
clips = {curve["clip"] for curve in curves}
toolsets = {curve["toolset"] for curve in curves}

# A curve per clip per toolset, and a full ladder on each. A receipt missing a
# point would make its BD-rate an extrapolation over a shorter interval without
# saying so.
ladder = receipt["settings"]["qp_ladder"]
for curve in curves:
    got = [point["qp"] for point in curve["points"]]
    if got != ladder:
        raise SystemExit(
            "rd-gate: %s/%s has QPs %s, not the ladder %s"
            % (curve["clip"], curve["toolset"], got, ladder)
        )
    for point in curve["points"]:
        if point["lossless"]:
            raise SystemExit(
                "rd-gate: %s/%s at qp %s is lossless, so its quality figure is unbounded "
                "and the curve through it is not a curve"
                % (curve["clip"], curve["toolset"], point["qp"])
            )

if len(curves) != len(clips) * len(toolsets):
    raise SystemExit("rd-gate: the receipt is not a full clip-by-toolset grid")
if "full" not in toolsets:
    raise SystemExit("rd-gate: there is no baseline curve to compare anything against")

print(
    "rd-gate: receipt covers %d clip(s) x %d toolset(s) at %d QPs"
    % (len(clips), len(toolsets), len(ladder))
)
PY

# Re-encode the ends of every ladder and compare every recorded figure. This
# also prints each ablation's bitrate difference, which is the number the charts
# page draws and the one a reader is most likely to quote.
cargo run --release --locked --quiet -p kf-tools --example rd_verify -- \
  --receipts "$receipts" --quick

echo "rd-gate: OK — receipts reproduce at both ends of every ladder, on every clip and toolset"

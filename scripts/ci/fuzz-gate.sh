#!/usr/bin/env bash
#
# The preflight decoder campaign.
#
# The iteration count comes from `spec/v1/constants.toml`, which is the one
# place any campaign budget is declared. It used to be a literal here — a third
# number beside the nightly budget and the unit tests' smoke run, and the only
# one nothing checked.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

iterations="$(sed -n 's/^fuzz_iterations_preflight = \([0-9][0-9]*\)$/\1/p' \
  spec/v1/constants.toml)"
if [[ -z "$iterations" ]]; then
  echo "fuzz-gate: FAILED — spec/v1/constants.toml declares no fuzz_iterations_preflight" >&2
  exit 1
fi

cargo test --locked -p kf-fuzz
cargo run --release --locked --quiet -p kf-fuzz --bin kf-fuzz-campaign -- \
  --iterations "$iterations"

echo "fuzz-gate: OK — four deterministic decoder campaigns, smoke plus release replay"

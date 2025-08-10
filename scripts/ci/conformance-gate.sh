#!/usr/bin/env bash

set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"

cargo run --locked --quiet -p kf-tools --example generate_conformance -- --check

echo "conformance-gate: OK — oracle, hand, and encoder streams and decoded hashes are byte-identical"

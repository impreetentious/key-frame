#!/usr/bin/env bash

set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"

cargo run --locked --quiet -p kf-tools --example generate_staging -- --check

echo "conformance-gate: OK — pre-freeze streams and decoded hashes are byte-identical"

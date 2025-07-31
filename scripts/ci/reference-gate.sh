#!/usr/bin/env bash

set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"

python3 spec/oracle.py --check
"$root_dir/scripts/ci/forbidden-grep.sh"
cargo test --locked -p kf-ref

echo "reference-gate: OK — independent reader, CRC, range, transform, prediction, and atomic commit"

#!/usr/bin/env bash

set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"

python3 spec/oracle.py --check
cargo test --locked -p kf-bitstream

echo "bitstream-gate: OK — independent B.1/B.2 bytes, CRC spans, flags, bounds, and resync"

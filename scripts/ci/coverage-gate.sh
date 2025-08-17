#!/usr/bin/env bash
#
# Conformance coverage, measured rather than declared. Decodes every committed
# vector in both decoders and checks the observed context ids and syntax
# elements against `conformance/syntax-coverage.toml`, in both directions: the
# inventory may not claim coverage the suite does not deliver, and the suite may
# not code an id the inventory holds in reserve.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

python3 "$repo_root/scripts/ci/check-syntax-coverage.py"
cargo run --locked --quiet -p kf-tools --example verify_coverage

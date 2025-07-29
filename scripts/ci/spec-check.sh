#!/usr/bin/env bash

set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"

python3 spec/check_assets.py
python3 spec/oracle.py --check
python3 spec/generate_docs.py --check
python3 spec/mutation_check.py

hits="$(grep -rniE --include='*.md' --include='*.toml' \
  '\b(TODO|TBD|FIXME|placeholder|to be decided)\b|exact table|per the table' \
  spec docs/bitstream.md 2>/dev/null || true)"
if [[ -n "$hits" ]]; then
  echo "spec-check: unresolved normative prose"
  echo "$hits" | sed 's/^/  /'
  exit 1
fi

if grep -rnE --include='*.rs' '\b(fn|impl)[[:space:]<(]' crates/kf-spec/src/ >/dev/null; then
  echo "spec-check: kf-spec must remain inert data with no executable functions"
  exit 1
fi

echo "spec-check: OK — frozen assets, oracle, generated document, mutation rejection, and inert boundary"

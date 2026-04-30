#!/usr/bin/env bash

set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"

status=0
if ! grep -q '^license = "MIT"$' Cargo.toml; then
  echo 'license-coherence: Cargo.toml must declare MIT'
  status=1
fi
if ! grep -q '^\[MIT\](LICENSE) © 2024-2026 Sidakpreet Singh$' README.md; then
  echo 'license-coherence: README license line is missing or inconsistent'
  status=1
fi
if ! grep -q 'Permission is hereby granted, free of charge' LICENSE; then
  echo 'license-coherence: LICENSE is not the MIT license text'
  status=1
fi

if [[ $status -ne 0 ]]; then
  echo 'license-coherence: FAILED'
  exit 1
fi

echo 'license-coherence: OK — MIT'

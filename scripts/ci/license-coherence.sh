#!/usr/bin/env bash

set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"

status=0
if ! grep -q '^license = "Apache-2.0"$' Cargo.toml; then
  echo 'license-coherence: Cargo.toml must declare Apache-2.0'
  status=1
fi
if ! grep -q '^Apache-2.0 © 2024-2026 Sidakpreet Singh — see \[LICENSE\](LICENSE)\.$' README.md; then
  echo 'license-coherence: README license line is missing or inconsistent'
  status=1
fi
if ! grep -q 'Licensed under the Apache License, Version 2.0' LICENSE; then
  echo 'license-coherence: LICENSE is not the Apache License 2.0 text'
  status=1
fi

if [[ $status -ne 0 ]]; then
  echo 'license-coherence: FAILED'
  exit 1
fi

echo 'license-coherence: OK — Apache-2.0'

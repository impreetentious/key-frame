#!/usr/bin/env bash
#
# The local mirror of CI. Run this before opening a change; it is the same set
# of checks the `check` workflow runs, in the same order, so a red pipeline is
# something you already saw on your own machine.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "[1/11] version coherence"
node "$repo_root/scripts/check-version-coherence.mjs"
echo "[2/11] documentation coherence"
"$repo_root/scripts/ci/doc-coherence.sh"
echo "[3/11] license coherence"
"$repo_root/scripts/ci/license-coherence.sh"
echo "[4/11] specification closure"
"$repo_root/scripts/ci/spec-check.sh"
echo "[5/11] range-coder gate"
"$repo_root/scripts/ci/range-gate.sh"
echo "[6/11] transform and quantization gate"
"$repo_root/scripts/ci/transform-gate.sh"
echo "[7/11] cargo fmt"
cargo fmt --all -- --check
echo "[8/11] cargo clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings
echo "[9/11] forbidden API scan and decoder boundary"
"$repo_root/scripts/ci/forbidden-grep.sh"
echo "[10/11] tests"
cargo test --locked --workspace --all-targets
echo "[11/11] rustdoc"
cargo doc --locked --workspace --no-deps
echo "preflight: PASS"

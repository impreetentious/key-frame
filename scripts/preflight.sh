#!/usr/bin/env bash
#
# The local mirror of CI. Run this before opening a change; it is the same set
# of checks the `check` workflow runs, in the same order, so a red pipeline is
# something you already saw on your own machine.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "[1/12] version coherence"
node "$repo_root/scripts/check-version-coherence.mjs"
echo "[2/12] documentation coherence"
"$repo_root/scripts/ci/doc-coherence.sh"
echo "[3/12] license coherence"
"$repo_root/scripts/ci/license-coherence.sh"
echo "[4/12] specification closure"
"$repo_root/scripts/ci/spec-check.sh"
echo "[5/12] range-coder gate"
"$repo_root/scripts/ci/range-gate.sh"
echo "[6/12] transform and quantization gate"
"$repo_root/scripts/ci/transform-gate.sh"
echo "[7/12] bitstream header and packet gate"
"$repo_root/scripts/ci/bitstream-gate.sh"
echo "[8/12] cargo fmt"
cargo fmt --all -- --check
echo "[9/12] cargo clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings
echo "[10/12] forbidden API scan and decoder boundary"
"$repo_root/scripts/ci/forbidden-grep.sh"
echo "[11/12] tests"
cargo test --locked --workspace --all-targets
echo "[12/12] rustdoc"
cargo doc --locked --workspace --no-deps
echo "preflight: PASS"

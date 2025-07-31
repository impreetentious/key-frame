#!/usr/bin/env bash
#
# The local mirror of CI. Run this before opening a change; it is the same set
# of checks the `check` workflow runs, in the same order, so a red pipeline is
# something you already saw on your own machine.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "[1/14] version coherence"
node "$repo_root/scripts/check-version-coherence.mjs"
echo "[2/14] documentation coherence"
"$repo_root/scripts/ci/doc-coherence.sh"
echo "[3/14] license coherence"
"$repo_root/scripts/ci/license-coherence.sh"
echo "[4/14] specification closure"
"$repo_root/scripts/ci/spec-check.sh"
echo "[5/14] range-coder gate"
"$repo_root/scripts/ci/range-gate.sh"
echo "[6/14] transform and quantization gate"
"$repo_root/scripts/ci/transform-gate.sh"
echo "[7/14] bitstream header and packet gate"
"$repo_root/scripts/ci/bitstream-gate.sh"
echo "[8/14] syntax round-trip gate"
"$repo_root/scripts/ci/syntax-gate.sh"
echo "[9/14] independent reference decoder gate"
"$repo_root/scripts/ci/reference-gate.sh"
echo "[10/14] cargo fmt"
cargo fmt --all -- --check
echo "[11/14] cargo clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings
echo "[12/14] forbidden API scan and decoder boundary"
"$repo_root/scripts/ci/forbidden-grep.sh"
echo "[13/14] tests"
cargo test --locked --workspace --all-targets
echo "[14/14] rustdoc"
cargo doc --locked --workspace --no-deps
echo "preflight: PASS"

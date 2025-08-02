#!/usr/bin/env bash
#
# The local mirror of CI. Run this before opening a change; it is the same set
# of checks the `check` workflow runs, in the same order, so a red pipeline is
# something you already saw on your own machine.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "[1/15] version coherence"
node "$repo_root/scripts/check-version-coherence.mjs"
echo "[2/15] documentation coherence"
"$repo_root/scripts/ci/doc-coherence.sh"
echo "[3/15] license coherence"
"$repo_root/scripts/ci/license-coherence.sh"
echo "[4/15] specification closure"
"$repo_root/scripts/ci/spec-check.sh"
echo "[5/15] range-coder gate"
"$repo_root/scripts/ci/range-gate.sh"
echo "[6/15] transform and quantization gate"
"$repo_root/scripts/ci/transform-gate.sh"
echo "[7/15] bitstream header and packet gate"
"$repo_root/scripts/ci/bitstream-gate.sh"
echo "[8/15] syntax round-trip gate"
"$repo_root/scripts/ci/syntax-gate.sh"
echo "[9/15] independent reference decoder gate"
"$repo_root/scripts/ci/reference-gate.sh"
echo "[10/15] probe schema and canonical replay gate"
"$repo_root/scripts/ci/probe-gate.sh"
echo "[11/15] cargo fmt"
cargo fmt --all -- --check
echo "[12/15] cargo clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings
echo "[13/15] forbidden API scan and decoder boundary"
"$repo_root/scripts/ci/forbidden-grep.sh"
echo "[14/15] tests"
cargo test --locked --workspace --all-targets
echo "[15/15] rustdoc"
cargo doc --locked --workspace --no-deps
echo "preflight: PASS"

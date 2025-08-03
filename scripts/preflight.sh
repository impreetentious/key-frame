#!/usr/bin/env bash
#
# The local mirror of CI. Run this before opening a change; it is the same set
# of checks the `check` workflow runs, in the same order, so a red pipeline is
# something you already saw on your own machine.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "[1/17] version coherence"
node "$repo_root/scripts/check-version-coherence.mjs"
echo "[2/17] documentation coherence"
"$repo_root/scripts/ci/doc-coherence.sh"
echo "[3/17] license coherence"
"$repo_root/scripts/ci/license-coherence.sh"
echo "[4/17] specification closure"
"$repo_root/scripts/ci/spec-check.sh"
echo "[5/17] range-coder gate"
"$repo_root/scripts/ci/range-gate.sh"
echo "[6/17] transform and quantization gate"
"$repo_root/scripts/ci/transform-gate.sh"
echo "[7/17] bitstream header and packet gate"
"$repo_root/scripts/ci/bitstream-gate.sh"
echo "[8/17] syntax round-trip gate"
"$repo_root/scripts/ci/syntax-gate.sh"
echo "[9/17] independent reference decoder gate"
"$repo_root/scripts/ci/reference-gate.sh"
echo "[10/17] probe schema and canonical replay gate"
"$repo_root/scripts/ci/probe-gate.sh"
echo "[11/17] end-to-end intra codec gate"
"$repo_root/scripts/ci/intra-gate.sh"
echo "[12/17] native conformance gate"
"$repo_root/scripts/ci/conformance-gate.sh"
echo "[13/17] cargo fmt"
cargo fmt --all -- --check
echo "[14/17] cargo clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings
echo "[15/17] forbidden API scan and decoder boundary"
"$repo_root/scripts/ci/forbidden-grep.sh"
echo "[16/17] tests"
cargo test --locked --workspace --all-targets
echo "[17/17] rustdoc"
cargo doc --locked --workspace --no-deps
echo "preflight: PASS"

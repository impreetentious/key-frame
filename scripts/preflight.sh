#!/usr/bin/env bash
#
# The local mirror of CI. Run this before opening a change; it is the same set
# of checks the `check` workflow runs, in the same order, so a red pipeline is
# something you already saw on your own machine.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "[1/21] version coherence"
node "$repo_root/scripts/check-version-coherence.mjs"
echo "[2/21] documentation coherence"
"$repo_root/scripts/ci/doc-coherence.sh"
echo "[3/21] license coherence"
"$repo_root/scripts/ci/license-coherence.sh"
echo "[4/21] specification closure"
"$repo_root/scripts/ci/spec-check.sh"
echo "[5/21] range-coder gate"
"$repo_root/scripts/ci/range-gate.sh"
echo "[6/21] transform and quantization gate"
"$repo_root/scripts/ci/transform-gate.sh"
echo "[7/21] bitstream header and packet gate"
"$repo_root/scripts/ci/bitstream-gate.sh"
echo "[8/21] syntax round-trip gate"
"$repo_root/scripts/ci/syntax-gate.sh"
echo "[9/21] independent reference decoder gate"
"$repo_root/scripts/ci/reference-gate.sh"
echo "[10/21] probe schema and canonical replay gate"
"$repo_root/scripts/ci/probe-gate.sh"
echo "[11/21] end-to-end intra codec gate"
"$repo_root/scripts/ci/intra-gate.sh"
echo "[12/21] native conformance gate"
"$repo_root/scripts/ci/conformance-gate.sh"
echo "[13/21] inter codec gate"
"$repo_root/scripts/ci/inter-gate.sh"
echo "[14/21] entropy lockstep and transaction gate"
"$repo_root/scripts/ci/entropy-gate.sh"
echo "[15/21] deblock filter gate"
"$repo_root/scripts/ci/deblock-gate.sh"
echo "[16/21] rate-control gate"
"$repo_root/scripts/ci/rate-gate.sh"
echo "[17/21] cargo fmt"
cargo fmt --all -- --check
echo "[18/21] cargo clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings
echo "[19/21] forbidden API scan and decoder boundary"
"$repo_root/scripts/ci/forbidden-grep.sh"
echo "[20/21] tests"
cargo test --locked --workspace --all-targets
echo "[21/21] rustdoc"
cargo doc --locked --workspace --no-deps
echo "preflight: PASS"

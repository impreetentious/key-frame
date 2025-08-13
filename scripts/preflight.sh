#!/usr/bin/env bash
#
# The local mirror of CI. Run this before opening a change; it is the same set
# of checks the `check` workflow runs, in the same order, so a red pipeline is
# something you already saw on your own machine.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "[1/23] version coherence"
node "$repo_root/scripts/check-version-coherence.mjs"
echo "[2/23] documentation coherence"
"$repo_root/scripts/ci/doc-coherence.sh"
echo "[3/23] license coherence"
"$repo_root/scripts/ci/license-coherence.sh"
echo "[4/23] specification closure"
"$repo_root/scripts/ci/spec-check.sh"
echo "[5/23] range-coder gate"
"$repo_root/scripts/ci/range-gate.sh"
echo "[6/23] transform and quantization gate"
"$repo_root/scripts/ci/transform-gate.sh"
echo "[7/23] bitstream header and packet gate"
"$repo_root/scripts/ci/bitstream-gate.sh"
echo "[8/23] syntax round-trip gate"
"$repo_root/scripts/ci/syntax-gate.sh"
echo "[9/23] independent reference decoder gate"
"$repo_root/scripts/ci/reference-gate.sh"
echo "[10/23] probe schema and canonical replay gate"
"$repo_root/scripts/ci/probe-gate.sh"
echo "[11/23] end-to-end intra codec gate"
"$repo_root/scripts/ci/intra-gate.sh"
echo "[12/23] native conformance gate"
"$repo_root/scripts/ci/conformance-gate.sh"
echo "[13/23] inter codec gate"
"$repo_root/scripts/ci/inter-gate.sh"
echo "[14/23] entropy lockstep and transaction gate"
"$repo_root/scripts/ci/entropy-gate.sh"
echo "[15/23] deblock filter gate"
"$repo_root/scripts/ci/deblock-gate.sh"
echo "[16/23] natural corpus bit-exactness gate"
"$repo_root/scripts/ci/corpus-gate.sh"
echo "[17/23] rate-control gate"
"$repo_root/scripts/ci/rate-gate.sh"
echo "[18/23] decoder campaign gate"
"$repo_root/scripts/ci/fuzz-gate.sh"
echo "[19/23] cargo fmt"
cargo fmt --all -- --check
echo "[20/23] cargo clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings
echo "[21/23] forbidden API scan and decoder boundary"
"$repo_root/scripts/ci/forbidden-grep.sh"
echo "[22/23] tests"
cargo test --locked --workspace --all-targets
echo "[23/23] rustdoc"
cargo doc --locked --workspace --no-deps
echo "preflight: PASS"

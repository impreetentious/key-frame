#!/usr/bin/env bash
#
# The local mirror of CI. Run this before opening a change; it is the same set
# of checks the `check` workflow runs, in the same order, so a red pipeline is
# something you already saw on your own machine.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "[1/24] version coherence"
node "$repo_root/scripts/check-version-coherence.mjs"
echo "[2/24] documentation coherence"
"$repo_root/scripts/ci/doc-coherence.sh"
echo "[3/24] license coherence"
"$repo_root/scripts/ci/license-coherence.sh"
echo "[4/24] specification closure"
"$repo_root/scripts/ci/spec-check.sh"
echo "[5/24] range-coder gate"
"$repo_root/scripts/ci/range-gate.sh"
echo "[6/24] transform and quantization gate"
"$repo_root/scripts/ci/transform-gate.sh"
echo "[7/24] bitstream header and packet gate"
"$repo_root/scripts/ci/bitstream-gate.sh"
echo "[8/24] syntax round-trip gate"
"$repo_root/scripts/ci/syntax-gate.sh"
echo "[9/24] independent reference decoder gate"
"$repo_root/scripts/ci/reference-gate.sh"
echo "[10/24] probe schema and canonical replay gate"
"$repo_root/scripts/ci/probe-gate.sh"
echo "[11/24] end-to-end intra codec gate"
"$repo_root/scripts/ci/intra-gate.sh"
echo "[12/24] native conformance gate"
"$repo_root/scripts/ci/conformance-gate.sh"
echo "[13/24] inter codec gate"
"$repo_root/scripts/ci/inter-gate.sh"
echo "[14/24] entropy lockstep and transaction gate"
"$repo_root/scripts/ci/entropy-gate.sh"
echo "[15/24] deblock filter gate"
"$repo_root/scripts/ci/deblock-gate.sh"
echo "[16/24] natural corpus bit-exactness gate"
"$repo_root/scripts/ci/corpus-gate.sh"
echo "[17/24] rate-control gate"
"$repo_root/scripts/ci/rate-gate.sh"
echo "[18/24] decoder campaign gate"
"$repo_root/scripts/ci/fuzz-gate.sh"
echo "[19/24] corruption and error matrix gate"
"$repo_root/scripts/ci/error-matrix-gate.sh"
echo "[20/24] cargo fmt"
cargo fmt --all -- --check
echo "[21/24] cargo clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings
echo "[22/24] forbidden API scan and decoder boundary"
"$repo_root/scripts/ci/forbidden-grep.sh"
echo "[23/24] tests"
cargo test --locked --workspace --all-targets
echo "[24/24] rustdoc"
cargo doc --locked --workspace --no-deps
echo "preflight: PASS"

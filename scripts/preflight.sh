#!/usr/bin/env bash
#
# The local mirror of CI. Run this before opening a change; it is the same set
# of checks the `check` workflow runs, in the same order, so a red pipeline is
# something you already saw on your own machine.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "[1/25] version coherence"
node "$repo_root/scripts/check-version-coherence.mjs"
echo "[2/25] documentation coherence"
"$repo_root/scripts/ci/doc-coherence.sh"
echo "[3/25] license coherence"
"$repo_root/scripts/ci/license-coherence.sh"
echo "[4/25] specification closure"
"$repo_root/scripts/ci/spec-check.sh"
echo "[5/25] range-coder gate"
"$repo_root/scripts/ci/range-gate.sh"
echo "[6/25] transform and quantization gate"
"$repo_root/scripts/ci/transform-gate.sh"
echo "[7/25] bitstream header and packet gate"
"$repo_root/scripts/ci/bitstream-gate.sh"
echo "[8/25] syntax round-trip gate"
"$repo_root/scripts/ci/syntax-gate.sh"
echo "[9/25] independent reference decoder gate"
"$repo_root/scripts/ci/reference-gate.sh"
echo "[10/25] probe schema and canonical replay gate"
"$repo_root/scripts/ci/probe-gate.sh"
echo "[11/25] end-to-end intra codec gate"
"$repo_root/scripts/ci/intra-gate.sh"
echo "[12/25] native conformance gate"
"$repo_root/scripts/ci/conformance-gate.sh"
echo "[13/25] inter codec gate"
"$repo_root/scripts/ci/inter-gate.sh"
echo "[14/25] entropy lockstep and transaction gate"
"$repo_root/scripts/ci/entropy-gate.sh"
echo "[15/25] deblock filter gate"
"$repo_root/scripts/ci/deblock-gate.sh"
echo "[16/25] natural corpus bit-exactness gate"
"$repo_root/scripts/ci/corpus-gate.sh"
echo "[17/25] rate-control gate"
"$repo_root/scripts/ci/rate-gate.sh"
echo "[18/25] decoder campaign gate"
"$repo_root/scripts/ci/fuzz-gate.sh"
echo "[19/25] corruption and error matrix gate"
"$repo_root/scripts/ci/error-matrix-gate.sh"
echo "[20/25] conformance coverage gate"
"$repo_root/scripts/ci/coverage-gate.sh"
echo "[21/25] cargo fmt"
cargo fmt --all -- --check
echo "[22/25] cargo clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings
echo "[23/25] forbidden API scan and decoder boundary"
"$repo_root/scripts/ci/forbidden-grep.sh"
echo "[24/25] tests"
cargo test --locked --workspace --all-targets
echo "[25/25] rustdoc"
cargo doc --locked --workspace --no-deps
echo "preflight: PASS"

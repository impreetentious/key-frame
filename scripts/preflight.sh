#!/usr/bin/env bash
#
# The local mirror of CI. Run this before opening a change; it is the same set
# of checks the `check` workflow runs, in the same order, so a red pipeline is
# something you already saw on your own machine.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "[1/27] version coherence"
node "$repo_root/scripts/check-version-coherence.mjs"
echo "[2/27] documentation coherence"
"$repo_root/scripts/ci/doc-coherence.sh"
echo "[3/27] license coherence"
"$repo_root/scripts/ci/license-coherence.sh"
echo "[4/27] specification closure"
"$repo_root/scripts/ci/spec-check.sh"
echo "[5/27] range-coder gate"
"$repo_root/scripts/ci/range-gate.sh"
echo "[6/27] transform and quantization gate"
"$repo_root/scripts/ci/transform-gate.sh"
echo "[7/27] bitstream header and packet gate"
"$repo_root/scripts/ci/bitstream-gate.sh"
echo "[8/27] syntax round-trip gate"
"$repo_root/scripts/ci/syntax-gate.sh"
echo "[9/27] independent reference decoder gate"
"$repo_root/scripts/ci/reference-gate.sh"
echo "[10/27] probe schema and canonical replay gate"
"$repo_root/scripts/ci/probe-gate.sh"
echo "[11/27] end-to-end intra codec gate"
"$repo_root/scripts/ci/intra-gate.sh"
echo "[12/27] native conformance gate"
"$repo_root/scripts/ci/conformance-gate.sh"
echo "[13/27] inter codec gate"
"$repo_root/scripts/ci/inter-gate.sh"
echo "[14/27] entropy lockstep and transaction gate"
"$repo_root/scripts/ci/entropy-gate.sh"
echo "[15/27] deblock filter gate"
"$repo_root/scripts/ci/deblock-gate.sh"
echo "[16/27] natural corpus bit-exactness gate"
"$repo_root/scripts/ci/corpus-gate.sh"
echo "[17/27] rate-control gate"
"$repo_root/scripts/ci/rate-gate.sh"
echo "[18/27] decoder campaign gate"
"$repo_root/scripts/ci/fuzz-gate.sh"
echo "[19/27] corruption and error matrix gate"
"$repo_root/scripts/ci/error-matrix-gate.sh"
echo "[20/27] conformance coverage gate"
"$repo_root/scripts/ci/coverage-gate.sh"
echo "[21/27] random-access seek gate"
"$repo_root/scripts/ci/seek-gate.sh"
echo "[22/27] native and WebAssembly equality gate"
"$repo_root/scripts/ci/wasm-gate.sh"
echo "[23/27] cargo fmt"
cargo fmt --all -- --check
echo "[24/27] cargo clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings
echo "[25/27] forbidden API scan and decoder boundary"
"$repo_root/scripts/ci/forbidden-grep.sh"
echo "[26/27] tests"
cargo test --locked --workspace --all-targets
echo "[27/27] rustdoc"
cargo doc --locked --workspace --no-deps
echo "preflight: PASS"

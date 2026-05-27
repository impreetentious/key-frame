#!/usr/bin/env bash
#
# The local mirror of CI. Run this before opening a change; it is the same set
# of checks the `check` workflow runs, in the same order, so a red pipeline is
# something you already saw on your own machine.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "[1/35] version coherence"
node "$repo_root/scripts/check-version-coherence.mjs"
echo "[2/35] documentation coherence"
"$repo_root/scripts/ci/doc-coherence.sh"
echo "[3/35] claim coherence"
"$repo_root/scripts/ci/claims-coherence.sh"
echo "[4/35] interface coherence"
"$repo_root/scripts/ci/interface-coherence.mjs"
echo "[5/35] declared scalar use"
"$repo_root/scripts/ci/declared-scalar-use.sh"
echo "[6/35] license coherence"
"$repo_root/scripts/ci/license-coherence.sh"
echo "[7/35] dependency closure"
"$repo_root/scripts/ci/dependency-closure.sh"
echo "[8/35] specification closure"
"$repo_root/scripts/ci/spec-check.sh"
echo "[9/35] range-coder gate"
"$repo_root/scripts/ci/range-gate.sh"
echo "[10/35] transform and quantization gate"
"$repo_root/scripts/ci/transform-gate.sh"
echo "[11/35] bitstream header and packet gate"
"$repo_root/scripts/ci/bitstream-gate.sh"
echo "[12/35] syntax round-trip gate"
"$repo_root/scripts/ci/syntax-gate.sh"
echo "[13/35] independent reference decoder gate"
"$repo_root/scripts/ci/reference-gate.sh"
echo "[14/35] probe schema and canonical replay gate"
"$repo_root/scripts/ci/probe-gate.sh"
echo "[15/35] end-to-end intra codec gate"
"$repo_root/scripts/ci/intra-gate.sh"
echo "[16/35] native conformance gate"
"$repo_root/scripts/ci/conformance-gate.sh"
echo "[17/35] inter codec gate"
"$repo_root/scripts/ci/inter-gate.sh"
echo "[18/35] entropy lockstep and transaction gate"
"$repo_root/scripts/ci/entropy-gate.sh"
echo "[19/35] deblock filter gate"
"$repo_root/scripts/ci/deblock-gate.sh"
echo "[20/35] natural corpus bit-exactness gate"
"$repo_root/scripts/ci/corpus-gate.sh"
echo "[21/35] rate-control gate"
"$repo_root/scripts/ci/rate-gate.sh"
echo "[22/35] quality-metric gate"
"$repo_root/scripts/ci/metric-gate.sh"
echo "[23/35] rate-distortion receipt gate"
"$repo_root/scripts/ci/rd-gate.sh"
echo "[24/35] decoder campaign gate"
"$repo_root/scripts/ci/fuzz-gate.sh"
echo "[25/35] corruption and error matrix gate"
"$repo_root/scripts/ci/error-matrix-gate.sh"
echo "[26/35] conformance coverage gate"
"$repo_root/scripts/ci/coverage-gate.sh"
echo "[27/35] random-access seek gate"
"$repo_root/scripts/ci/seek-gate.sh"
echo "[28/35] native and WebAssembly equality gate"
"$repo_root/scripts/ci/wasm-gate.sh"
echo "[29/35] projection room build, budget, and browser smoke"
"$repo_root/scripts/ci/site-gate.sh"
# The other command the README tells a reader to run.
#
# The `## Verify` block is held to this list step for step, and the terminal
# demo was the one thing on the front page that nothing here ran. It drives all
# four binaries end to end against a clip it generates itself and ends by
# comparing a decode against a hash, so it is a real check as well as a
# performance — and it had every opportunity to rot quietly between the day it
# was written and the day a reader tried it.
echo "[30/35] terminal demo"
"$repo_root/scripts/demo.sh" > /dev/null
echo "[31/35] cargo fmt"
cargo fmt --all -- --check
echo "[32/35] cargo clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings
echo "[33/35] forbidden API scan and decoder boundary"
"$repo_root/scripts/ci/forbidden-grep.sh"
echo "[34/35] tests"
cargo test --locked --workspace --all-targets
echo "[35/35] rustdoc"
cargo doc --locked --workspace --no-deps
echo "preflight: PASS"

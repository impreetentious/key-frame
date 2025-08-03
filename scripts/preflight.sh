#!/usr/bin/env bash
#
# The local mirror of CI. Run this before opening a change; it is the same set
# of checks the `check` workflow runs, in the same order, so a red pipeline is
# something you already saw on your own machine.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "[1/16] version coherence"
node "$repo_root/scripts/check-version-coherence.mjs"
echo "[2/16] documentation coherence"
"$repo_root/scripts/ci/doc-coherence.sh"
echo "[3/16] license coherence"
"$repo_root/scripts/ci/license-coherence.sh"
echo "[4/16] specification closure"
"$repo_root/scripts/ci/spec-check.sh"
echo "[5/16] range-coder gate"
"$repo_root/scripts/ci/range-gate.sh"
echo "[6/16] transform and quantization gate"
"$repo_root/scripts/ci/transform-gate.sh"
echo "[7/16] bitstream header and packet gate"
"$repo_root/scripts/ci/bitstream-gate.sh"
echo "[8/16] syntax round-trip gate"
"$repo_root/scripts/ci/syntax-gate.sh"
echo "[9/16] independent reference decoder gate"
"$repo_root/scripts/ci/reference-gate.sh"
echo "[10/16] probe schema and canonical replay gate"
"$repo_root/scripts/ci/probe-gate.sh"
echo "[11/16] end-to-end intra codec gate"
"$repo_root/scripts/ci/intra-gate.sh"
echo "[12/16] cargo fmt"
cargo fmt --all -- --check
echo "[13/16] cargo clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings
echo "[14/16] forbidden API scan and decoder boundary"
"$repo_root/scripts/ci/forbidden-grep.sh"
echo "[15/16] tests"
cargo test --locked --workspace --all-targets
echo "[16/16] rustdoc"
cargo doc --locked --workspace --no-deps
echo "preflight: PASS"

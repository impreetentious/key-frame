#!/usr/bin/env bash
#
# The local mirror of CI. Run this before opening a change; it is the same set
# of checks the `check` workflow runs, in the same order, so a red pipeline is
# something you already saw on your own machine.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "[1/13] version coherence"
node "$repo_root/scripts/check-version-coherence.mjs"
echo "[2/13] documentation coherence"
"$repo_root/scripts/ci/doc-coherence.sh"
echo "[3/13] license coherence"
"$repo_root/scripts/ci/license-coherence.sh"
echo "[4/13] specification closure"
"$repo_root/scripts/ci/spec-check.sh"
echo "[5/13] range-coder gate"
"$repo_root/scripts/ci/range-gate.sh"
echo "[6/13] transform and quantization gate"
"$repo_root/scripts/ci/transform-gate.sh"
echo "[7/13] bitstream header and packet gate"
"$repo_root/scripts/ci/bitstream-gate.sh"
echo "[8/13] syntax round-trip gate"
"$repo_root/scripts/ci/syntax-gate.sh"
echo "[9/13] cargo fmt"
cargo fmt --all -- --check
echo "[10/13] cargo clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings
echo "[11/13] forbidden API scan and decoder boundary"
"$repo_root/scripts/ci/forbidden-grep.sh"
echo "[12/13] tests"
cargo test --locked --workspace --all-targets
echo "[13/13] rustdoc"
cargo doc --locked --workspace --no-deps
echo "preflight: PASS"

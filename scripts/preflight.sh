#!/usr/bin/env bash
#
# The local mirror of CI. Run this before opening a change; it is the same set
# of checks the `check` workflow runs, in the same order, so a red pipeline is
# something you already saw on your own machine.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "[1/9] version coherence"
node "$repo_root/scripts/check-version-coherence.mjs"
echo "[2/9] documentation coherence"
"$repo_root/scripts/ci/doc-coherence.sh"
echo "[3/9] license coherence"
"$repo_root/scripts/ci/license-coherence.sh"
echo "[4/9] specification closure"
"$repo_root/scripts/ci/spec-check.sh"
echo "[5/9] cargo fmt"
cargo fmt --all -- --check
echo "[6/9] cargo clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings
echo "[7/9] forbidden API scan and decoder boundary"
"$repo_root/scripts/ci/forbidden-grep.sh"
echo "[8/9] tests"
cargo test --locked --workspace --all-targets
echo "[9/9] rustdoc"
cargo doc --locked --workspace --no-deps
echo "preflight: PASS"

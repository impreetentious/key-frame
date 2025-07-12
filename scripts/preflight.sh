#!/usr/bin/env bash
#
# The local mirror of CI. Run this before opening a change; it is the same set
# of checks the `check` workflow runs, in the same order, so a red pipeline is
# something you already saw on your own machine.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "[1/8] version coherence"
node "$repo_root/scripts/check-version-coherence.mjs"
echo "[2/8] documentation coherence"
"$repo_root/scripts/ci/doc-coherence.sh"
echo "[3/8] license coherence"
"$repo_root/scripts/ci/license-coherence.sh"
echo "[4/8] cargo fmt"
cargo fmt --all -- --check
echo "[5/8] cargo clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings
echo "[6/8] forbidden API scan and decoder boundary"
"$repo_root/scripts/ci/forbidden-grep.sh"
echo "[7/8] tests"
cargo test --locked --workspace --all-targets
echo "[8/8] rustdoc"
cargo doc --locked --workspace --no-deps
echo "preflight: PASS"

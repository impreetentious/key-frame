#!/usr/bin/env bash
#
# Native and WebAssembly must agree byte for byte.
#
# The module is built for `wasm32-unknown-unknown` and run under Node's own
# WebAssembly engine with no import object, because it imports nothing. That
# keeps the artifact under test identical to the one the projection room loads:
# no binding generator, no glue file, nothing that could differ between the
# gate's copy and the shipped copy.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo test --locked -p kf-wasm
cargo build --locked --release --target wasm32-unknown-unknown -p kf-wasm
node "$repo_root/scripts/ci/wasm-equality.mjs"

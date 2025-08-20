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

probe_dir="$(mktemp -d)"
trap 'rm -rf "$probe_dir"' EXIT

cargo test --locked -p kf-wasm
cargo build --locked --release --target wasm32-unknown-unknown -p kf-wasm
node "$repo_root/scripts/ci/wasm-equality.mjs" --probe-out "$probe_dir"

# The inspector draws every overlay from the probe's JSON, so the two builds
# must agree on the report as well as on the pixels. Diffing the whole document
# rather than a hash is deliberate: when it does differ, the failure should name
# the field.
for stream in conformance/oracle/*.kfv conformance/hand/*.kfv conformance/encoder/*.kfv; do
  origin="$(basename "$(dirname "$stream")")"
  name="$(basename "$stream" .kfv)"
  cargo run --locked --quiet -p kf-tools --bin kfprobe -- "$stream" > "$probe_dir/native.json"
  if ! diff -u "$probe_dir/native.json" "$probe_dir/${origin}_${name}.json" > "$probe_dir/diff.txt"; then
    echo "wasm-gate: FAILED — the WebAssembly probe differs from the native probe on ${origin}:${name}"
    head -40 "$probe_dir/diff.txt"
    exit 1
  fi
done

echo "wasm-gate: OK — decoded pixels and syntax reports are identical across native and WebAssembly"

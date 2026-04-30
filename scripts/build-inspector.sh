#!/usr/bin/env bash
#
# Builds the projection room, decoder and all.
#
# Everything the page fetches at runtime — the WebAssembly module, the sample
# clip, the bug catalogue, and the regression streams it pins — is produced
# here rather than committed. The module is a build output, and the rest are
# copies of files that already live under version control: keeping second
# copies would create things that have to stay equal with no gate that says so.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# The sample the page opens on load. A real conformance stream, decoded live in
# the browser exactly as any opened file is: nothing about this page is a
# pre-decoded picture.
sample="conformance/encoder/inter_motion64_qp32.kfv"

cargo build --locked --release --target wasm32-unknown-unknown -p kf-wasm

mkdir -p inspector/public
cp target/wasm32-unknown-unknown/release/kf_wasm.wasm inspector/public/kf_wasm.wasm
cp "$sample" inspector/public/sample.kfv

# The catalogue and its pinned streams. This refuses outright if an entry names
# a regression stream that is not there, so the site cannot ship a finding
# nobody can reproduce.
node scripts/collect-cutting-room.mjs

if [[ ! -d inspector/node_modules ]]; then
  npm --prefix inspector ci
fi

npm --prefix inspector run build

echo "build-inspector: OK — inspector/dist is ready to serve"

#!/usr/bin/env bash
#
# Bit-exactness on natural video. Every clip the corpus manifest pins is
# encoded, decoded by both decoders and by the encoder's own closed loop, and
# required to agree sample for sample at five quantizers.
#
# The clip list comes from the manifest rather than from a line in this file,
# so a clip added to the corpus is measured on the day it is added.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

"$repo_root/scripts/fetch-corpus.sh"

checked=0
while IFS=$'\t' read -r name file _url _md5 _width _height _frames; do
  [[ -n "$name" ]] || continue
  cargo run --release --locked --quiet -p kf-tools --example corpus_bitexact -- \
    --input "corpus/clips/$file" --frames 12
  checked=$((checked + 1))
done < <("$repo_root/scripts/corpus-manifest.sh")

if [[ $checked -eq 0 ]]; then
  echo "corpus-gate: FAILED — no clip was checked; the gate would be vacuous"
  exit 1
fi

echo "corpus-gate: OK — $checked natural clip(s) decode bit-exact in both decoders at five QPs"

#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

"$repo_root/scripts/fetch-corpus.sh"
for clip in akiyo_qcif foreman_qcif; do
  cargo run --release --locked --quiet -p kf-tools --example corpus_bitexact -- \
    --input "corpus/clips/$clip.y4m" --frames 12
done

echo "corpus-gate: OK — natural clips decode bit-exact in both decoders at five QPs"

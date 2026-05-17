#!/usr/bin/env bash
#
# Average-bitrate accuracy at disclosed operating points on the pinned corpus.
#
# The clip paths come from the manifest; the targets stay here, because a
# bitrate is a property of the measurement rather than of the clip. A clip the
# manifest gains and this file has no target for is an error, not a skip: the
# corpus and the measurement have to be added to together or the published
# accuracy figure stops describing the corpus it names.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo test --locked -p kf-enc --test rate --test abr
"$repo_root/scripts/fetch-corpus.sh"

measured=0
while IFS=$'\t' read -r name file _url _md5 _width _height _frames; do
  [[ -n "$name" ]] || continue
  case "$name" in
    akiyo_qcif) target=120000 ;;
    foreman_qcif) target=200000 ;;
    *)
      echo "rate-gate: FAILED — the corpus pins $name with no disclosed bitrate target here"
      exit 1
      ;;
  esac
  cargo run --release --locked --quiet -p kf-tools --example measure_rate -- \
    --input "corpus/clips/$file" --bitrate "$target" --frames 48
  measured=$((measured + 1))
done < <("$repo_root/scripts/corpus-manifest.sh")

if [[ $measured -eq 0 ]]; then
  echo "rate-gate: FAILED — no operating point was measured"
  exit 1
fi

echo "rate-gate: OK — leaky-bucket bounds, deterministic ABR, $measured corpus point(s) within 5%"

#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo test --locked -p kf-enc --test rate --test abr
"$repo_root/scripts/fetch-corpus.sh"
cargo run --release --locked --quiet -p kf-tools --example measure_rate -- \
  --input corpus/clips/akiyo_qcif.y4m --bitrate 120000 --frames 48
cargo run --release --locked --quiet -p kf-tools --example measure_rate -- \
  --input corpus/clips/foreman_qcif.y4m --bitrate 200000 --frames 48

echo "rate-gate: OK — leaky-bucket bounds, deterministic ABR, corpus clips within 5%"

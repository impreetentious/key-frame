#!/usr/bin/env bash
#
# Random access must be an optimization, never a different answer. Seeks to
# every frame of every committed vector, and of a freshly encoded clip with
# several keyframes, and requires the result to equal a linear decode in both
# decoders — including which keyframe the seek restarted from and how many
# frames it had to decode to get there.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo test --locked -p kf-dec --test seek
cargo test --locked -p kf-ref --test seek
cargo run --locked --quiet -p kf-tools --example seek_equivalence

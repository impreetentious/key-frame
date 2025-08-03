#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
clip_dir="$repo_root/corpus/clips"
mkdir -p "$clip_dir"

fetch() {
  local file="$1" url="$2" expected_md5="$3"
  local path="$clip_dir/$file"
  if [[ ! -f "$path" ]]; then
    curl --fail --location --retry 3 --output "$path" "$url"
  fi
  local actual_md5
  actual_md5="$(openssl dgst -md5 "$path" | awk '{print $NF}')"
  if [[ "$actual_md5" != "$expected_md5" ]]; then
    echo "corpus: checksum mismatch for $file" >&2
    echo "  expected $expected_md5" >&2
    echo "  actual   $actual_md5" >&2
    exit 1
  fi
  echo "corpus: verified $file"
}

fetch \
  "akiyo_qcif.y4m" \
  "https://media.xiph.org/video/derf/y4m/akiyo_qcif.y4m" \
  "18269969d4333b7c5427431b15dbaa99"
fetch \
  "foreman_qcif.y4m" \
  "https://media.xiph.org/video/derf/y4m/foreman_qcif.y4m" \
  "7328a830f32ee4eeea37c40b8514822c"

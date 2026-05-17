#!/usr/bin/env bash
#
# Fetches the pinned corpus and holds each clip to everything the manifest
# declares about it.
#
# The repository does not redistribute video, so the clips arrive over the
# network and the manifest is what makes that reproducible. It used to be
# reproducible only for the two fields this script happened to restate: the
# URL and the checksum were literals here, and the declared width, height, and
# frame count were read by nothing at all. A clip could have been repinned in
# the manifest and fetched from the old address, or replaced upstream with a
# differently shaped sequence of the same length, and the gates downstream
# would have measured whatever arrived.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
clip_dir="$repo_root/corpus/clips"
mkdir -p "$clip_dir"

# Y4M frames carry a six-byte `FRAME\n` marker. The format allows parameters
# after it, which would make the marker longer; no clip in this corpus uses
# them, and one that did would fail the size check below rather than be
# silently miscounted.
frame_marker_bytes=6

verify_shape() {
  local path="$1" name="$2" width="$3" height="$4" frames="$5"

  local header
  IFS= read -r header < "$path" || true
  if [[ "$header" != YUV4MPEG2* ]]; then
    echo "corpus: $name is not a Y4M stream" >&2
    return 1
  fi

  local declared_width declared_height
  declared_width="$(printf '%s\n' "$header" | tr ' ' '\n' | sed -n 's/^W//p')"
  declared_height="$(printf '%s\n' "$header" | tr ' ' '\n' | sed -n 's/^H//p')"
  if [[ "$declared_width" != "$width" || "$declared_height" != "$height" ]]; then
    echo "corpus: $name is ${declared_width}x${declared_height}, manifest says ${width}x${height}" >&2
    return 1
  fi

  # 4:2:0 at even dimensions: one luma sample per pixel and two chroma planes
  # at a quarter each. The header line includes its own newline.
  local header_bytes payload_bytes expected actual
  header_bytes=$(( ${#header} + 1 ))
  payload_bytes=$(( width * height * 3 / 2 ))
  expected=$(( header_bytes + frames * (frame_marker_bytes + payload_bytes) ))
  actual="$(wc -c < "$path" | tr -d ' ')"
  if [[ "$actual" != "$expected" ]]; then
    echo "corpus: $name is $actual bytes; $frames frames of ${width}x${height} would be $expected" >&2
    return 1
  fi

  echo "corpus: verified $name — ${width}x${height}, $frames frames"
}

while IFS=$'\t' read -r name file url md5 width height frames; do
  [[ -n "$name" ]] || continue
  path="$clip_dir/$file"
  if [[ ! -f "$path" ]]; then
    curl --fail --location --retry 3 --output "$path" "$url"
  fi

  actual_md5="$(openssl dgst -md5 "$path" | awk '{print $NF}')"
  if [[ "$actual_md5" != "$md5" ]]; then
    echo "corpus: checksum mismatch for $file" >&2
    echo "  expected $md5" >&2
    echo "  actual   $actual_md5" >&2
    exit 1
  fi

  # The checksum already pins the bytes, so this cannot disagree with it on a
  # clip that downloaded correctly. It is here for the case the checksum cannot
  # see: a repin that updates the hash and the dimensions apart from each other,
  # which would leave every declared shape wrong and every gate green.
  verify_shape "$path" "$file" "$width" "$height" "$frames"
done < <("$repo_root/scripts/corpus-manifest.sh")

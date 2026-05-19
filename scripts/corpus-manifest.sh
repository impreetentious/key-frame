#!/usr/bin/env bash
#
# The pinned corpus, read from the file that pins it.
#
# `corpus/manifest.toml` is the repository's statement of which clips the
# measurement and bit-exactness gates use, where they come from, and what they
# hash to. Every shell consumer of that statement reads it here, so there is one
# copy of it: the fetch script used to restate the URLs and checksums as
# literals and two gates used to restate the clip names, which made the manifest
# a document about the corpus rather than the corpus itself. Editing a checksum
# there changed nothing that runs.
#
# The tools read the same file through `kf_tools::pinned_clips`, because a
# campaign that shelled out to this script to learn its own clip list would be
# a Rust program with a bash dependency in the middle of it. Two readers of one
# normative file is the same arrangement as the two decoders, and it earns its
# keep the same way: `crates/kf-tools/tests/corpus_manifest.rs` drives both over
# the committed manifest and over the shapes where they could plausibly part,
# and requires the same answer from each.
#
# Reads `$1` when given one, so that test can hand both readers the same
# synthetic manifest; defaults to the repository's own.
#
# Prints one record per clip as
#
#   name<TAB>file<TAB>url<TAB>md5<TAB>width<TAB>height<TAB>frames
#
# in declared order. Every field is required; a clip missing one is an error
# rather than a record with a hole in it, because the fields exist to be
# checked and a blank one checks nothing.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="${1:-$repo_root/corpus/manifest.toml}"

if [[ ! -f "$manifest" ]]; then
  echo "corpus-manifest: FAILED — $manifest is missing" >&2
  exit 1
fi

records="$(awk '
  function flush() {
    if (name == "") return
    if (file == "" || url == "" || md5 == "" || width == "" || height == "" || frames == "") {
      printf "corpus-manifest: clip %s is missing a declared field\n", name > "/dev/stderr"
      bad = 1
      return
    }
    printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\n", name, file, url, md5, width, height, frames
    count += 1
  }
  # The text after `key =`, with the one quoting rule the manifest uses: a
  # value is either a quoted string or a bare integer, and a `#` outside quotes
  # begins a comment. Stripping every quote and keeping the rest — which is
  # what this did — turned `frames = 300 # whole sequence` into the frame count
  # `300 # whole sequence`, which reaches an arithmetic expansion downstream.
  function value(line,   rest, stop) {
    rest = line
    sub(/^[[:space:]]*[a-z_0-9]+[[:space:]]*=[[:space:]]*/, "", rest)
    if (substr(rest, 1, 1) == "\"") {
      rest = substr(rest, 2)
      stop = index(rest, "\"")
      return stop > 0 ? substr(rest, 1, stop - 1) : rest
    }
    sub(/#.*$/, "", rest)
    sub(/[[:space:]]+$/, "", rest)
    return rest
  }
  # Any other table ends the clip being read. Without this rule a `[fetch]` or
  # `[[extras]]` section after the last clip donated its keys to that clip:
  # a `url` there became the address the fetch script downloaded from, silently
  # and while every field still looked declared.
  /^[[:space:]]*\[/ {
    flush()
    name = file = url = md5 = width = height = frames = ""
    next
  }
  /^[[:space:]]*name[[:space:]]*=/   { name   = value($0); next }
  /^[[:space:]]*file[[:space:]]*=/   { file   = value($0); next }
  /^[[:space:]]*url[[:space:]]*=/    { url    = value($0); next }
  /^[[:space:]]*md5[[:space:]]*=/    { md5    = value($0); next }
  /^[[:space:]]*width[[:space:]]*=/  { width  = value($0); next }
  /^[[:space:]]*height[[:space:]]*=/ { height = value($0); next }
  /^[[:space:]]*frames[[:space:]]*=/ { frames = value($0); next }
  END {
    flush()
    if (bad) exit 1
    # A manifest that parsed to nothing would let every consumer loop zero
    # times and report success, which is the one outcome worse than a wrong
    # clip list.
    if (count == 0) {
      print "corpus-manifest: the manifest declares no clips" > "/dev/stderr"
      exit 1
    }
  }
' "$manifest")"

printf '%s\n' "$records"

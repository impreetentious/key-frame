#!/usr/bin/env bash
#
# The pinned corpus, read from the file that pins it.
#
# `corpus/manifest.toml` is the repository's statement of which clips the
# measurement and bit-exactness gates use, where they come from, and what they
# hash to. Every consumer of that statement reads it here, so there is one copy
# of it: the fetch script used to restate the URLs and checksums as literals and
# two gates used to restate the clip names, which made the manifest a document
# about the corpus rather than the corpus itself. Editing a checksum there
# changed nothing that runs.
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
manifest="$repo_root/corpus/manifest.toml"

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
  function value(line,   rest) {
    rest = line
    sub(/^[a-z_0-9]+[[:space:]]*=[[:space:]]*/, "", rest)
    gsub(/"/, "", rest)
    sub(/[[:space:]]+$/, "", rest)
    return rest
  }
  /^\[\[clips\]\]/ { flush(); name = file = url = md5 = width = height = frames = ""; next }
  /^name[[:space:]]*=/   { name   = value($0); next }
  /^file[[:space:]]*=/   { file   = value($0); next }
  /^url[[:space:]]*=/    { url    = value($0); next }
  /^md5[[:space:]]*=/    { md5    = value($0); next }
  /^width[[:space:]]*=/  { width  = value($0); next }
  /^height[[:space:]]*=/ { height = value($0); next }
  /^frames[[:space:]]*=/ { frames = value($0); next }
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

#!/usr/bin/env bash
#
# The dependency set, closed.
#
# This workspace compiles fourteen crates and nothing else. Not "few
# dependencies" — none: the range coder, the CRC32C, the JSON reader and
# writer, the Y4M parser, the SHA-256, the quality metrics, the BD-rate
# integrator, and the WebAssembly boundary are all written here, because a codec
# you can read cannot be a codec whose interesting parts arrive from elsewhere.
#
# That is a property of the tree today rather than a decision anything defends.
# One `cargo add` would end it, quietly and permanently: the lockfile would grow
# a subtree, every gate would stay green, and the claim on the front page would
# become false with no diff that says so. So it is checked.
#
# The second half is a different failure with the same shape. A crate directory
# that is not in the workspace member list is compiled by nothing and tested by
# nothing, while `scripts/ci/forbidden-grep.sh` scans it and reports it clean —
# a crate that looks covered and is not built. The directory listing and the
# member list have to be the same set.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

status=0

# Declared members, in the order the manifest lists them.
members="$(awk '
  /^members = \[/ { inside = 1; next }
  inside && /^\]/  { inside = 0; next }
  inside {
    gsub(/[",]/, "")
    gsub(/^[[:space:]]+|[[:space:]]+$/, "")
    if ($0 != "") print
  }
' Cargo.toml | sed 's#^crates/##' | sort)"

if [[ -z "$members" ]]; then
  echo "dependency-closure: FAILED — Cargo.toml lists no workspace members, so this would check nothing"
  exit 1
fi

# Directories that actually hold a crate.
present="$(find crates -mindepth 2 -maxdepth 2 -name Cargo.toml -print 2>/dev/null \
  | sed 's#^crates/##; s#/Cargo.toml$##' | sort)"

if [[ "$members" != "$present" ]]; then
  echo "dependency-closure: the workspace member list and the crates directory disagree"
  diff <(printf '%s\n' "$members") <(printf '%s\n' "$present") \
    | sed 's/^</  only in Cargo.toml: /; s/^>/  only on disk:      /' | grep -E 'only in|only on' || true
  echo "  A crate on disk that no member line names is compiled by nothing and tested by"
  echo "  nothing, while the forbidden-API scan reads it and reports it clean."
  status=1
fi

# Every package the lockfile resolves must be one of those crates.
locked="$(grep -A1 '^\[\[package\]\]' Cargo.lock \
  | sed -n 's/^name = "\(.*\)"$/\1/p' | sort)"

if [[ -z "$locked" ]]; then
  echo "dependency-closure: FAILED — Cargo.lock names no packages; the scan would be vacuous"
  exit 1
fi

foreign="$(comm -23 <(printf '%s\n' "$locked") <(printf '%s\n' "$members"))"
if [[ -n "$foreign" ]]; then
  echo "dependency-closure: the lockfile resolves packages this workspace does not contain"
  printf '%s\n' "$foreign" | sed 's/^/  /'
  echo "  Every algorithm here is written here on purpose. A dependency is a decision"
  echo "  that needs a decision-log row in docs/CODEC-SPEC.md before a lockfile entry."
  status=1
fi

if [[ $status -ne 0 ]]; then
  echo "dependency-closure: FAILED"
  exit 1
fi

echo "dependency-closure: OK — $(printf '%s\n' "$members" | wc -l | tr -d ' ') crate(s), no package from outside the workspace"

#!/usr/bin/env bash
#
# Every declared number and table has to be used by something.
#
# Three separate audits of this repository found the same defect: a numeric
# scalar declared in a frozen specification asset, described in the generated
# normative document as though it governed the codec, and read by nothing. The
# implementations carried the same values as literals and agreed with each
# other, so editing the declaration changed the published specification and
# broke no test. The transform stage shifts were like this, then the
# interpolation constants, then the packet header geometry.
#
# Fixing the instances does not fix the class. This gate does, in both
# directions: a numeric scalar in `spec/v1/` must be reachable from something
# that runs, and no script the build runs may spell out a declared value instead
# of reading it. The second half caught the nightly campaign budget, written as
# a literal in the workflow beside a declaration a test already policed.
# "Reachable" means the key is named in a crate, a specification script, a CI
# script, or the interface source — either because code reads it, or because
# `spec/check_assets.py` derives it from something else the assets say. Both
# count, because both mean an edit to the declaration makes something fail.
#
# The generated document is deliberately not a consumer. Everything appears
# there; that is what makes it a document rather than a check.
#
# There is no exemption list, and adding one would defeat the gate. A scalar
# nobody can justify checking is a scalar the specification should not declare.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

unread=""
count=0

# The search is scoped to the files that read the asset the key comes from.
#
# It used to be a bare word search over the whole tree, and that made the gate
# vacuous for every generically named declaration. `contexts.toml`'s `count`,
# `syntax.toml`'s `planes`, `mc.toml`'s `candidates`, `search.toml`'s `qp` — the
# word appears hundreds of times in unrelated code, so the gate reported them
# read while nothing anywhere consulted the declaration. Tightening it exposed
# two that genuinely were not: the coded plane list and the motion-vector
# predictor's candidate set, both of them decoder-normative.
#
# Scoping works because of how consumers reach an asset. Every reader finds it
# by name — `V1_ASSETS.iter().find(|asset| asset.name == "mc.toml")` in Rust,
# `V1 / "mc.toml"` in the specification scripts, a path in a CI script — so a
# file that reads an asset contains that asset's file name. A file that does
# not is not reading it, whatever words it happens to contain.
#
# Scoping alone is still not enough, because a generic key collides with an
# ordinary identifier inside the very files that read its asset. `width = 7`
# added to `scans.toml` passed a scoped word search: the scan readers talk about
# plane widths all day, and one of them says the word in a docstring. So the
# occurrence also has to look like a key being addressed rather than a variable
# being used.
#
# Readers address a key inside a string, always: `strip_prefix("filter_taps = [")`
# in Rust, `scalar_int(constants, "bit_depth")` in Python, and `^minimum_qp =`
# inside a pattern. Each of those puts the name against a quote or against the
# anchor that opens the pattern. An identifier in running code does not, and
# neither does a word in prose.
#
# Two files are kept out of the reader set. `spec/generate_docs.py` is excluded
# for the reason the document itself is: it names every asset and renders
# whatever it finds, so counting it would let the generator vouch for the
# declaration it prints. This script is excluded because it names assets to
# explain itself, and a gate that reads its own comments as evidence proves
# nothing.
readers_of() {
  grep -rlF -- "$1" crates spec/*.py scripts inspector/src 2>/dev/null \
    | grep -vE '^(spec/generate_docs\.py|scripts/ci/declared-scalar-use\.sh)$' || true
}

# Whether one of `readers` addresses `key` as a key rather than using the word.
addressed_by() {
  local key="$1" readers="$2"
  printf '%s\n' "$readers" \
    | xargs grep -lqE "[\"'^]${key}\b|\b${key}[\"']" 2>/dev/null
}

# A key also counts as read where the codebase builds the name at runtime. The
# size-suffixed tables are that case: `matrix.rs` and `scan.rs` reach `n4`
# through `format!("n{size} = [")`, so the literal name is never written down
# and a plain search would call a table every transform depends on unread. The
# builder has to be one of the asset's own readers, so this is a narrower
# allowance than it used to be rather than a blanket one.
interpolated='^n(4|8|16|32)$'

for asset in spec/v1/*.toml; do
  base="$(basename "$asset")"
  readers="$(readers_of "$base")"
  if [[ -z "$readers" ]]; then
    unread+="  $base: nothing reads this asset at all"$'\n'
    continue
  fi
  # Scalars and arrays alike. An unread array is the same defect as an unread
  # scalar and hides better: a table of numbers reads as authoritative, and a
  # coefficient-coding parameter table survived in this tree describing a
  # scheme the codec never implemented.
  while read -r key; do
    [[ -n "$key" ]] || continue
    count=$((count + 1))
    if [[ "$key" =~ $interpolated ]] \
      && printf '%s\n' "$readers" | xargs grep -lq 'n{size} = \[' 2>/dev/null; then
      continue
    fi
    if ! addressed_by "$key" "$readers"; then
      unread+="  $base: $key"$'\n'
    fi
  done < <(grep -oE '^[a-z_0-9]+ = (-?[0-9]+|\[)' "$asset" | sed "s/ = .*//" | sort -u)
done

if [[ -n "$unread" ]]; then
  echo "declared-scalar-use: the specification declares values nothing reads"
  printf '%s' "$unread"
  echo "  Each one is normative text a reader would take on trust and no test would defend."
  echo "  Make the implementation read it, or derive it in spec/check_assets.py from"
  echo "  something the assets already state. Do not add an exemption."
  echo "declared-scalar-use: FAILED"
  exit 1
fi

# The converse question, asked of the build's own runners.
#
# A declaration read by something can still be restated somewhere else, and the
# restatement is the copy that goes stale. The nightly job named the campaign
# budget as a literal beside a declaration a test already policed, so raising
# the declared budget would have left that job running the old number and
# reporting a clean campaign. No shell script, Node script, or workflow may
# spell out a declared value; it reads the declaration, the way the gates that
# already do this show how.
#
# Known limit: this matches the number as written. The module size budget was
# restated as `1.5 * 1024 * 1024` — the declared value in a spelling no search
# for its digits can see — and was found by reading rather than by this. Values
# under four digits are skipped, because a short declaration collides with the
# ordinary numbers in an unrelated command line.
restated=""
while read -r name value; do
  [[ -n "$name" ]] || continue
  while IFS= read -r hit; do
    [[ -n "$hit" ]] || continue
    # A line that names the declaration is reading it, not restating it.
    case "${hit#*:*:}" in
      *"$name"*) continue ;;
    esac
    restated+="  $name ($value) spelled out at ${hit%%:*}:$(printf '%s' "$hit" | cut -d: -f2)"$'\n'
  done < <(grep -rnE "(^|[^0-9.])${value}([^0-9.]|$)" \
    --include='*.sh' --include='*.mjs' --include='*.yml' \
    scripts .github 2>/dev/null || true)
done < <(grep -oE '^[a-z_0-9]+ = [0-9]{4,}$' spec/v1/constants.toml | sed 's/ = / /')

if [[ -n "$restated" ]]; then
  echo "declared-scalar-use: a build script spells out a value the specification declares"
  printf '%s' "$restated"
  echo "  Read the declaration instead. A number written in two places is a number that"
  echo "  will eventually disagree with itself, and the copy nothing checks is the one"
  echo "  that keeps running."
  echo "declared-scalar-use: FAILED"
  exit 1
fi

echo "declared-scalar-use: OK — $count declared scalar(s) and table(s), every one reachable from something that runs"

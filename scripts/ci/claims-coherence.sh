#!/usr/bin/env bash
#
# The claims table, kept honest.
#
# `docs/claims.md` maps every substantive claim the README makes to the check
# that would fail if it stopped being true. A table like that is worth exactly
# as much as its worst row: one entry naming a gate that was renamed or deleted
# turns the whole page from an index into a decoration, and it does so silently,
# because prose does not fail to compile.
#
# So every path the table names in backticks has to exist, and every gate that
# preflight runs has to appear in the table. The second half is the one that
# catches drift in the direction people actually drift: a gate added without a
# claim behind it is a check nobody can explain, and a claim whose gate was
# quietly dropped is a promise nobody keeps.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

claims="docs/claims.md"

if [[ ! -f "$claims" ]]; then
  echo "claims-coherence: FAILED — $claims is missing"
  exit 1
fi

status=0

# Every backticked path in the table must exist. Paths are recognised by having
# a directory separator and a known extension, so ordinary prose in backticks —
# a field name, a lint, a command — is left alone.
missing=""
while read -r path; do
  [[ -n "$path" ]] || continue
  if [[ ! -e "$path" ]]; then
    missing+="  $path"$'\n'
  fi
done < <(grep -oE '`[A-Za-z0-9_./-]+\.(sh|rs|mjs|py|toml|json|ts|md)`' "$claims" \
  | tr -d '`' | grep '/' | sort -u)

if [[ -n "$missing" ]]; then
  echo "claims-coherence: the table names paths that do not exist"
  printf '%s' "$missing"
  status=1
fi

# Every gate preflight runs must be accounted for. A gate with no claim behind
# it is a check whose purpose has been forgotten.
unclaimed=""
while read -r gate; do
  [[ -n "$gate" ]] || continue
  if ! grep -q "$gate" "$claims"; then
    unclaimed+="  $gate"$'\n'
  fi
done < <(grep -oE 'scripts/(ci/)?[a-z-]+\.(sh|mjs)' scripts/preflight.sh | sort -u)

if [[ -n "$unclaimed" ]]; then
  echo "claims-coherence: preflight runs checks that no claim explains"
  printf '%s' "$unclaimed"
  echo "  Add the claim each one enforces to $claims, or say why it needs no claim."
  status=1
fi

# A row may also cite a single test by name rather than the file holding it,
# which is the more precise thing to cite and the easier thing to break: a test
# rename is a local edit that leaves the prose behind, and a path check cannot
# see it because the file the test lives in still exists. Every backticked
# `trap_*` name has to be a test that is actually declared somewhere.
absent=""
while read -r test_name; do
  [[ -n "$test_name" ]] || continue
  if ! grep -rqF "fn $test_name(" crates spec conformance 2>/dev/null; then
    absent+="  $test_name"$'\n'
  fi
done < <(grep -oE '`trap_[a-z0-9_]+`' "$claims" | tr -d '`' | sort -u)

if [[ -n "$absent" ]]; then
  echo "claims-coherence: the table names tests that are not declared anywhere"
  printf '%s' "$absent"
  status=1
fi

if [[ $status -ne 0 ]]; then
  echo "claims-coherence: FAILED"
  exit 1
fi

rows="$(grep -cE '^\| .* \| .* \|$' "$claims")"
echo "claims-coherence: OK — $rows claim(s), every named check present and every gate claimed"

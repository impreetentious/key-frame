#!/usr/bin/env bash
#
# Every declared number has to be used by something.
#
# Three separate audits of this repository found the same defect: a numeric
# scalar declared in a frozen specification asset, described in the generated
# normative document as though it governed the codec, and read by nothing. The
# implementations carried the same values as literals and agreed with each
# other, so editing the declaration changed the published specification and
# broke no test. The transform stage shifts were like this, then the
# interpolation constants, then the packet header geometry.
#
# Fixing the instances does not fix the class. This gate does: a numeric scalar
# in `spec/v1/` must be reachable from something that runs, or the build fails.
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

while IFS=: read -r asset key; do
  [[ -n "$key" ]] || continue
  count=$((count + 1))
  if ! grep -rqw -- "$key" crates spec/*.py scripts inspector/src 2>/dev/null; then
    unread+="  $asset: $key"$'\n'
  fi
done < <(
  for asset in spec/v1/*.toml; do
    grep -oE '^[a-z_0-9]+ = -?[0-9]+$' "$asset" \
      | sed "s/ = .*//" \
      | sed "s|^|$(basename "$asset"):|"
  done | sort -u
)

if [[ -n "$unread" ]]; then
  echo "declared-scalar-use: the specification declares numbers nothing reads"
  printf '%s' "$unread"
  echo "  Each one is normative text a reader would take on trust and no test would defend."
  echo "  Make the implementation read it, or derive it in spec/check_assets.py from"
  echo "  something the assets already state. Do not add an exemption."
  echo "declared-scalar-use: FAILED"
  exit 1
fi

echo "declared-scalar-use: OK — $count declared scalar(s), every one reachable from something that runs"

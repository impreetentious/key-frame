#!/usr/bin/env bash
#
# The forbidden-API scan: the bit-exactness constitution, enforced.
#
# clippy covers what a lint can express (floats, unordered maps, ambient
# clocks). This covers what it cannot: a truncating cast with no named rule, a
# platform-sized integer in signal math, and the dependency boundary that keeps
# the two decoders from sharing a wrong assumption.
#
# The perimeter is fail-closed. Every crate under `crates/` is scanned unless
# it is named in HOST_CRATES below, so a new codec crate is covered the day it
# appears rather than the day someone remembers to add it here.
#
# Lines that begin with a comment marker are skipped, so prose may name a
# banned construct. This is a grep, not a parser: a banned construct hidden
# behind a trailing comment on a code line will still be caught, but a macro
# that assembles one from fragments will not. That is the known limit.

set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root_dir"

# Crates outside the codec perimeter: the tools compute quality metrics in
# floating point and read the filesystem, and the wasm surface bridges to JS.
HOST_CRATES=(kf-tools kf-wasm kf-fuzz)

codec_paths=()
for crate_dir in crates/*/; do
  crate="$(basename "$crate_dir")"
  skip=0
  for host in "${HOST_CRATES[@]}"; do
    [[ "$crate" == "$host" ]] && skip=1
  done
  [[ $skip -eq 1 ]] || codec_paths+=("$crate_dir")
done

# A perimeter that matches nothing would report success while checking nothing,
# which is the worst outcome a gate can have. Treat it as a failure.
if [[ ${#codec_paths[@]} -eq 0 ]]; then
  echo "forbidden-grep: FAILED — the codec perimeter is empty; the scan would be vacuous"
  exit 1
fi

status=0

# scan <label> <extended-regex> [path-exempt-regex]
#
# The exemption is applied to grep's output rather than through
# `--exclude-dir`, which has no effect on a directory named as a starting path
# and would silently exempt nothing.
scan() {
  local label="$1" pattern="$2" exempt="${3:-}"
  local hits
  hits="$(grep -rnE --include='*.rs' "$pattern" "${codec_paths[@]}" 2>/dev/null \
    | grep -vE ':[0-9]+:[[:space:]]*(//|/\*|\*)' || true)"
  if [[ -n "$hits" && -n "$exempt" ]]; then
    hits="$(printf '%s\n' "$hits" | grep -vE "$exempt" || true)"
  fi
  if [[ -n "$hits" ]]; then
    echo "forbidden: $label"
    echo "$hits" | sed 's/^/  /'
    status=1
  fi
}

# Integer only. A float on any path that reaches a decoded pixel means two
# machines can disagree about what the stream says.
scan "floating point in a codec crate" \
  '\b(f32|f64)\b'

# A truncating cast is the one conversion the compiler will not argue with. On
# a path that reaches a decoded pixel, `as` silently discards the high bits in
# every profile, debug and release alike, so neither the lint build nor the
# checked-arithmetic release profile can see it: it is not an overflow, it is an
# answer.
#
# Almost every conversion in this workspace is already `From` or `TryFrom`, so
# this is not a ban that would be suppressed everywhere within a week. It asks
# for a rule instead: a numeric `as` cast is allowed where the comment block
# directly above it begins `cast:` and says why the discarded bits are not
# wanted. Four casts in the codec perimeter meet that bar today, and the one
# that truncates on purpose — the range encoder's delayed carry — is the reason
# the rule is a rule rather than a prohibition.
unnamed_casts=""
while IFS= read -r file; do
  [[ -n "$file" ]] || continue
  while IFS= read -r hit; do
    [[ -n "$hit" ]] || continue
    line="${hit%%:*}"
    # Walk up through the comment block immediately above the cast. A rule that
    # has to sit next to the cast is a rule that gets re-read when the cast is
    # edited; one allowed to drift ten lines away is decoration.
    named=0
    probe=$((line - 1))
    while [[ $probe -ge 1 ]]; do
      above="$(sed -n "${probe}p" "$file")"
      [[ "$above" =~ ^[[:space:]]*// ]] || break
      if [[ "$above" == *"cast:"* ]]; then
        named=1
        break
      fi
      probe=$((probe - 1))
    done
    [[ $named -eq 1 ]] || unnamed_casts+="  $file:$hit"$'\n'
  done < <(grep -nE '\bas (u8|u16|u32|u64|u128|usize|i8|i16|i32|i64|i128|isize)\b' "$file" \
    | grep -vE '^[0-9]+:[[:space:]]*(//|/\*|\*)' || true)
done < <(find "${codec_paths[@]}" -name '*.rs' -type f | sort)

if [[ -n "$unnamed_casts" ]]; then
  echo "forbidden: numeric cast with no named rule"
  printf '%s' "$unnamed_casts"
  echo "  Use From or TryFrom where the conversion cannot lose anything."
  echo "  Where it can and that is the intent, put a comment block directly above"
  echo "  the cast that starts 'cast:' and says which bits go and why."
  status=1
fi

# There is deliberately no blanket `usize` ban here. Slice indexing needs it,
# so a repository-wide scan would fire on every buffer access and be
# suppressed everywhere within a week — and a gate that is always suppressed
# is worse than no gate. The rule that platform-sized integers stay out of
# signal math is enforced where it is checkable: each stage names its
# container width, and the stage-width traps drive worst-case inputs through
# it.

# Determinism: ordered containers only, no ambient anything.
scan "unordered container in a codec crate" \
  '\b(HashMap|HashSet)\b'
scan "ambient time in a codec crate" \
  '\b(SystemTime|Instant)\b'
scan "ambient environment, filesystem, or threads in a codec crate" \
  '\bstd::(env|fs|thread|process)\b'

# The reference decoder's isolation is the verification story. It depends only
# on the inert frame storage types, the specification assets, and std — it owns
# its reader, its checksum, its arithmetic, and its state machine. One shared
# helper turns two independent decoders into one decoder with a mirror.
if [[ -d crates/kf-ref ]]; then
  imports="$(grep -rnE --include='*.rs' \
    '\b(use|extern crate)[[:space:]]+kf_(core|range|bitstream|transform|predict|dec|enc|tools|wasm)\b' \
    crates/kf-ref/ 2>/dev/null \
    | grep -vE ':[0-9]+:[[:space:]]*(//|/\*|\*)' || true)"
  if [[ -n "$imports" ]]; then
    echo "forbidden: kf-ref imports a production codec crate"
    echo "$imports" | sed 's/^/  /'
    status=1
  fi

  if [[ -f crates/kf-ref/Cargo.toml ]]; then
    deps="$(awk '/^\[(dependencies|dev-dependencies|build-dependencies)\]/{flag=1;next} /^\[/{flag=0} flag && NF && $0 !~ /^[[:space:]]*#/' \
      crates/kf-ref/Cargo.toml | grep -vE '^[[:space:]]*(kf-frame|kf-spec)\b' || true)"
    if [[ -n "$deps" ]]; then
      echo "forbidden: kf-ref declares a dependency beyond kf-frame and kf-spec"
      echo "$deps" | sed 's/^/  /'
      status=1
    fi
  fi
fi

# Both oracles must be independently runnable. If either ever imports this
# workspace's output, a generator and its consumer can share a wrong formula
# and agree perfectly. The bitstream oracle authors syntax vectors; the metric
# oracle authors the SSIM and BD-rate vectors, and the same argument applies to
# a published number as to a decoded pixel.
for oracle in spec/oracle.py bench/metric_oracle.py; do
  [[ -f "$oracle" ]] || continue
  leaks="$(grep -nE '^[[:space:]]*(import|from)[[:space:]]+[A-Za-z_]' "$oracle" 2>/dev/null \
    | grep -vE '[[:space:]](argparse|dataclasses|hashlib|itertools|json|math|os|pathlib|re|struct|sys|typing)([[:space:].]|$)' \
    || true)"
  if [[ -n "$leaks" ]]; then
    echo "note: $oracle imports beyond the standard-library allowlist"
    echo "$leaks" | sed 's/^/  /'
    status=1
  fi
done

if [[ $status -ne 0 ]]; then
  echo "forbidden-grep: FAILED"
  exit 1
fi

echo "forbidden-grep: OK — ${#codec_paths[@]} codec crate(s) scanned"

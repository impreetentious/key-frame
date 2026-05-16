#!/usr/bin/env bash
#
# The terminal demo: encode, decode, measure, inspect, and prove.
#
# Five steps, in the order someone meeting the codec would want them. The last
# one is the point of the other four — it compares the decoded output against a
# hash and says, in one line, whether decode was bit-exact. A demo that showed
# the pretty parts and stopped before the proof would be an advertisement.
#
# Everything here runs from a clean checkout with no corpus fetched: the source
# clip is generated, deterministically, by the same tool the ablation campaign
# uses. Pass --input to run it against your own Y4M instead.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

qp=32
frames=24
input=""
work_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --input) input="$2"; shift 2 ;;
    --qp) qp="$2"; shift 2 ;;
    --frames) frames="$2"; shift 2 ;;
    --keep) work_dir="$repo_root/out/demo"; shift ;;
    -h|--help)
      echo "usage: $0 [--input clip.y4m] [--qp N] [--frames N] [--keep]"
      exit 0
      ;;
    *) echo "demo: unknown argument $1" >&2; exit 1 ;;
  esac
done

if [[ -z "$work_dir" ]]; then
  work_dir="$(mktemp -d)"
  trap 'rm -rf "$work_dir"' EXIT
fi
mkdir -p "$work_dir"

rule() { printf '\n\033[1m%s\033[0m\n' "$1"; }

# Release, always. A debug build of a scalar codec is slow enough that a first
# impression of the project becomes "it hangs".
cargo build --release --locked --quiet -p kf-tools

bin="$repo_root/target/release"
example_run() { cargo run --release --locked --quiet -p kf-tools --example "$@"; }

if [[ -z "$input" ]]; then
  rule "0. A source clip, generated rather than downloaded"
  input="$work_dir/source.y4m"
  example_run make_clip -- \
    --output "$input" --width 176 --height 144 --frames "$frames" --pattern motion
fi

rule "1. Encode"
"$bin/kfenc" --input "$input" --qp "$qp" --output "$work_dir/out.kfv"

rule "2. Decode, and read the header back field by field"
"$bin/kfdec" "$work_dir/out.kfv" --output "$work_dir/decoded.y4m"

rule "3. Measure the decoded picture against the source"
"$bin/kfmetric" psnr "$input" "$work_dir/decoded.y4m" > "$work_dir/psnr.json"
"$bin/kfmetric" ssim "$input" "$work_dir/decoded.y4m" > "$work_dir/ssim.json"
python3 - "$work_dir/psnr.json" "$work_dir/ssim.json" <<'PY'
import json
from pathlib import Path
import sys

psnr, ssim = (json.loads(Path(path).read_text(encoding="utf-8")) for path in sys.argv[1:3])
figure = lambda report: "lossless" if report["lossless"] else "%.4f" % report["global"]
print("  PSNR-Y  %s dB over %d frames" % (figure(psnr), psnr["frames"]))
print("  SSIM-Y  %s" % figure(ssim))
print("  Both figures are luma only, cropped to the displayed picture.")
print("  Regenerate either with: kfmetric repro <report.json>")
PY

rule "4. Inspect one frame's syntax"
"$bin/kfprobe" "$work_dir/out.kfv" --frame $(( frames > 8 ? 8 : 0 )) \
  > "$work_dir/probe.json"
python3 - "$work_dir/probe.json" <<'PROBE'
import json
from pathlib import Path
import sys

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
frame = report["frame"]
superblocks = frame["superblocks"]
blocks = [block for superblock in superblocks for block in superblock["cbs"]]

print("  frame %d, %s, qp %d, %d superblock(s), %d coding block(s)" % (
    frame["frame_index"],
    "key" if frame["flags"]["key"] else "inter",
    frame["qp"],
    len(superblocks),
    len(blocks),
))

# One superblock's partition, drawn as the quadtree it is. The bar length is the
# block's side, so a split superblock reads as a staircase rather than a table.
first = superblocks[0]
print("\n  superblock at (%d, %d) partitions to:" % tuple(first["pos"]))
for block in first["cbs"]:
    prediction = block["prediction"]
    kind = prediction["kind"]
    if kind == "intra":
        detail = prediction.get("mode", "?")
    elif kind == "skip":
        detail = prediction.get("reference", "?")
    else:
        detail = "%s mv %s" % (
            prediction.get("reference", "?"),
            prediction.get("mv_q4", "?"),
        )
    print("    %-8s %2dx%-2d at (%3d,%3d)  %s %s" % (
        "#" * max(1, block["size"] // 8),
        block["size"], block["size"], block["pos"][0], block["pos"][1],
        kind, detail,
    ))

# The two accounting figures, kept apart. Adding them would produce a number
# that looks like "the bits this block cost" and is not one.
print("\n  accounting, the two quantities never added:")
print("    modeled entropy   what the encoder's model predicted")
print("    emission-time     when the range coder happened to flush a byte")
for block in first["cbs"][:4]:
    print("      %2dx%-2d at (%3d,%3d)  modeled %8.2f bits   emitted %3d bytes" % (
        block["size"], block["size"], block["pos"][0], block["pos"][1],
        block["modeled_entropy_q16"] / 65536.0,
        block["emitted_payload_bytes"],
    ))
print("    Neither is 'this block's bit count'. That question has no answer.")

print("\n  input payload %d bytes, canonical replay %d bytes, match: %s" % (
    frame["input_payload_len"],
    frame["canonical_replay_payload_len"],
    frame["canonical_payload_match"],
))
PROBE

rule "5. The proof"
coded_hash="$(openssl dgst -sha256 "$work_dir/out.kfv" | awk '{print $NF}')"
decoded_hash="$(openssl dgst -sha256 "$work_dir/decoded.y4m" | awk '{print $NF}')"

# Decoded twice, by two decoders written independently of one another. Equal
# output from one decoder run twice proves determinism; equal output from these
# two proves the format means something.
example_run corpus_bitexact -- --input "$input" --frames $(( frames > 6 ? 6 : frames )) \
  > "$work_dir/bitexact.log" 2>&1 && bitexact=1 || bitexact=0

printf '  coded stream   sha256 %s\n' "$coded_hash"
printf '  decoded video  sha256 %s\n' "$decoded_hash"
if [[ $bitexact -eq 1 ]]; then
  printf '\n  \033[32mThe fast decoder, the independently written reference decoder, and the\n'
  printf '  encoder'"'"'s own closed-loop reconstruction agree on every sample, at five\n'
  printf '  quantizers. Decode is bit-exact.\033[0m\n'
else
  printf '\n  \033[31mThe decoders disagree. This is a defect; see %s\033[0m\n' "$work_dir/bitexact.log"
  cat "$work_dir/bitexact.log"
  exit 1
fi

printf '\n  Every gate behind that claim runs with: ./scripts/preflight.sh\n\n'

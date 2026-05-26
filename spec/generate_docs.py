#!/usr/bin/env python3
"""Render the decoder-normative document from the frozen v1 assets.

Everything the document states about a declared value is interpolated from the
asset that declares it. That rule is the whole design of this script, and it is
newer than the script: the document used to restate the assets in prose, so an
edit to a frozen declaration changed the implementation, changed the asset
inventory hash at the foot of this document, and left the sentence above it
saying the old number. Both halves regenerated cleanly and every gate stayed
green, which is the worst shape a specification defect can take — the artifact
a third party implements from is the one carrying the stale value.

The loop filter's minimum edge length was found that way, written out as the
English word "eight" where no search for the number could see it. This script
now renders every declared value it mentions, small counts spelled as words
included, and `scripts/ci/declared-scalar-use.sh` fails the build if a number
the assets declare is typed into the template instead.
"""

import argparse
import ast
import hashlib
import json
from pathlib import Path
import re
import sys
import textwrap


ROOT = Path(__file__).resolve().parent
V1 = ROOT / "v1"
OUTPUT = ROOT.parent / "docs" / "bitstream.md"

# Counts the document spells as words. A tap count written as "six" is the same
# defect as an edge length written as "eight" — invisible to a search for the
# digit — so the word is rendered from the declaration rather than typed.
WORDS = {
    0: "zero",
    1: "one",
    2: "two",
    3: "three",
    4: "four",
    5: "five",
    6: "six",
    7: "seven",
    8: "eight",
    9: "nine",
    10: "ten",
    11: "eleven",
    12: "twelve",
}

# Byte widths of the declared field types. `bytes` is the trailing payload,
# whose extent is the packet's declared length rather than a fixed width.
FIELD_WIDTHS = {"u8": 1, "u16": 2, "u32": 4}


def word(value):
    """Small count as the English word the document uses."""
    if value not in WORDS:
        raise ValueError("no word for %d; the document should print the digit" % value)
    return WORDS[value]


def asset_text(asset):
    return (V1 / asset).read_text(encoding="utf-8")


def asset_scalar(asset, name):
    """One declared scalar from any frozen asset."""
    text = asset_text(asset)
    match = re.search(r"^%s\s*=\s*(.+)$" % re.escape(name), text, re.MULTILINE)
    if not match:
        raise ValueError("missing %s in %s" % (name, asset))
    return match.group(1).strip().strip('"')


def asset_int(asset, name):
    return int(asset_scalar(asset, name))


def asset_list(asset, name):
    """One declared array, parsed rather than pattern-matched out of the text."""
    text = asset_text(asset)
    match = re.search(r"^%s\s*=\s*(\[.*?\])$" % re.escape(name), text, re.MULTILINE | re.DOTALL)
    if not match:
        raise ValueError("missing array %s in %s" % (name, asset))
    return ast.literal_eval(match.group(1))


def section_scalar(asset, section, name):
    """One declared scalar from inside a named table of an asset."""
    text = asset_text(asset)
    body = text.split("[%s]" % section, 1)
    if len(body) != 2:
        raise ValueError("missing section [%s] in %s" % (section, asset))
    match = re.search(r"^%s\s*=\s*(.+)$" % re.escape(name), body[1], re.MULTILINE)
    if not match:
        raise ValueError("missing %s in [%s] of %s" % (name, section, asset))
    return match.group(1).strip().strip('"')


def scalar(name):
    return asset_scalar("constants.toml", name)


def constant(name):
    return int(scalar(name))


def crc_vector(name):
    """One committed CRC32C result, by the name the vector file gives it."""
    vectors = json.loads((V1 / "vectors.json").read_text(encoding="utf-8"))
    for entry in vectors["crc32c"]:
        if entry["name"] == name:
            return entry["crc32c"]
    raise ValueError("vectors.json declares no crc32c vector named %s" % name)


def bit_width(value):
    """Shift a power-of-two declaration stands for, refusing anything else.

    The document writes `range >> 12` and `range < 1<<24` where the assets
    declare 4096 and 16777216. Deriving the shift keeps those two spellings of
    one fact from parting; refusing a non-power-of-two keeps the derivation
    from quietly rounding if a declaration ever stops being one.
    """
    width = value.bit_length() - 1
    if value <= 0 or 1 << width != value:
        raise ValueError("%d is not a power of two; no shift stands for it" % value)
    return width


def tap_shape(declared):
    """`weak_4_tap` as the document writes it: `weak four-tap`."""
    match = re.fullmatch(r"([a-z]+)_([0-9]+)_tap", declared)
    if not match:
        raise ValueError("filter name %r does not declare a tap count" % declared)
    return "%s %s-tap" % (match.group(1), word(int(match.group(2))))


def declared_fields(section):
    """[(name, offset, width)] for one header, in the order the asset lists."""
    text = asset_text("fields.toml")
    body = text.split("[%s]" % section, 1)
    if len(body) != 2:
        raise ValueError("fields.toml declares no [%s] section" % section)
    match = re.search(r"fields = \[(.*?)\]", body[1], re.DOTALL)
    if not match:
        raise ValueError("fields.toml: %s declares no field list" % section)
    parsed = []
    for entry in re.findall(r'"([^"]+)"', match.group(1)):
        name, rest = entry.split(":", 1)
        kind, offset = rest.split("@", 1)
        parsed.append((name, int(offset), FIELD_WIDTHS.get(kind, "variable")))
    return parsed


def field_table(section, rules):
    """The header layout table, built from the field list the asset declares.

    Offsets and widths used to be typed here beside the same numbers in
    `fields.toml`, which is the arrangement that lets a reader implement one
    layout while both decoders implement another. The prose in the rule column
    stays written by hand — it is the part no asset states — but it is keyed by
    the declared field name, and a field the map does not name is an error
    rather than a row this script quietly omits.
    """
    declared = declared_fields(section)
    named = {name for name, _offset, _width in declared}
    unknown = sorted(set(rules) - named)
    if unknown:
        raise ValueError(
            "fields.toml [%s] declares no field named: %s" % (section, ", ".join(unknown))
        )
    rows = ["| Offset | Width | Field | Rule |", "| ---: | ---: | --- | --- |"]
    for name, offset, width in declared:
        if name not in rules:
            raise ValueError(
                "fields.toml [%s] declares %r and this document does not describe it"
                % (section, name)
            )
        label, rule = rules[name]
        rows.append("| %s | %s | %s | %s |" % (offset, width, label, rule))
    return "\n".join(rows)


def crc_span(section, start_key, length_key):
    """`covers bytes 4–15`, derived from the declared start and length."""
    start = int(section_scalar("fields.toml", section, start_key))
    length = int(section_scalar("fields.toml", section, length_key))
    return "%d–%d" % (start, start + length - 1)


def context_groups():
    """`partition split (12), skip (3), ...` from the declared group ids.

    The names are the document's, the counts are the asset's. A group the map
    does not name fails rather than vanishing from the sentence, which is what
    would otherwise happen the first time the context bank grew a group.
    """
    labels = {
        "partition_split": "partition split",
        "skip": "skip",
        "is_inter": "P-frame inter choice",
        "intra_mode": "intra mode",
        "ref_select": "reference choice",
        "mv_prefix": "motion prefix",
        "has_coeff": "coefficient presence",
        "last_position": "last position",
        "sig": "significance",
        "gt1": "greater-than-one",
        "gt2": "greater-than-two",
    }
    text = asset_text("contexts.toml")
    groups = re.findall(r'name = "([a-z_0-9]+)"\nids = (\[[^\]]*\])', text)
    if not groups:
        raise ValueError("contexts.toml declares no groups")
    unknown = sorted({name for name, _ids in groups} - set(labels))
    if unknown:
        raise ValueError(
            "contexts.toml declares groups this document does not name: %s"
            % ", ".join(unknown)
        )
    parts = ["%s (%d)" % (labels[name], len(ast.literal_eval(ids))) for name, ids in groups]
    listed = "%s, and %s" % (", ".join(parts[:-1]), parts[-1])
    # Wrapped here rather than by newlines placed in the template: this
    # sentence is as long as the context bank has groups, and a hand-placed
    # break would land in the wrong place the first time one was added. The
    # text carries no code span, so filling it cannot split one.
    return textwrap.fill(
        "The groups are %s. Bypass suffix and sign bins have no context id." % listed,
        width=78,
        break_on_hyphens=False,
    )


def intra_mode_list():
    """`DC, planar, horizontal, vertical, D45, D135, D117, and D153`."""
    modes = asset_list("intra.toml", "modes")
    rendered = [mode.upper() if re.fullmatch(r"d[0-9]+|dc", mode) else mode for mode in modes]
    return "%s, and %s" % (", ".join(rendered[:-1]), rendered[-1])


def intra_angle_list():
    """`D45 = 32, D135 = -32, D117 = -21, and D153 = -11`.

    The document named the four angular modes and never said what any of them
    projects at, so the one thing a second implementer needs from `[angles]`
    was the one thing the specification did not carry.
    """
    text = asset_text("intra.toml")
    body = text.split("[angles]", 1)
    if len(body) != 2:
        raise ValueError("intra.toml declares no [angles] section")
    declared = re.findall(r"^([a-z][a-z0-9]*)\s*=\s*(-?\d+)$", body[1].split("\n[", 1)[0], re.MULTILINE)
    if not declared:
        raise ValueError("intra.toml [angles] declares no angle")
    modes = asset_list("intra.toml", "modes")
    unlisted = sorted({name for name, _value in declared} - set(modes))
    if unlisted:
        raise ValueError(
            "intra.toml [angles] names modes the mode list does not: %s" % ", ".join(unlisted)
        )
    parts = ["%s = %s" % (name.upper(), value) for name, value in declared]
    return "%s, and %s" % (", ".join(parts[:-1]), parts[-1])


def template():
    """The document's own source text, before any declaration is substituted.

    `scripts/ci/declared-scalar-use.sh` reads this to ask whether a value the
    assets declare has been typed into the prose instead of interpolated. It
    asks the generator rather than parsing this file, because the answer to
    "what is the template" belongs to the thing that owns the template.
    """
    source = ast.parse(Path(__file__).read_text(encoding="utf-8"))
    for node in ast.walk(source):
        if (
            isinstance(node, ast.Call)
            and isinstance(node.func, ast.Attribute)
            and node.func.attr == "format"
            and isinstance(node.func.value, ast.Constant)
            and isinstance(node.func.value.value, str)
        ):
            return node.func.value.value
    raise ValueError("this module renders no formatted template")


def render():
    inventory = []
    for path in sorted(V1.iterdir()):
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        inventory.append("| `%s` | `%s` |" % (path.name, digest))

    version = scalar("bitstream_version")
    bit_depth = constant("bit_depth")
    coding_blocks = asset_list("constants.toml", "coding_block_sizes")
    transform_sizes = asset_list("constants.toml", "transform_sizes")
    superblock = constant("superblock_size")
    largest_transform = max(transform_sizes)
    per_side = superblock // largest_transform
    probability_total = constant("probability_total")

    sequence_rules = {
        "magic": ("magic", "ASCII `%s`" % section_scalar("fields.toml", "sequence", "magic_ascii")),
        "version": ("version", "exactly %s" % version),
        "width": ("width", "section 1"),
        "height": ("height", "section 1"),
        "chroma": ("chroma", "%s, meaning `%s`" % (scalar("chroma_code"), scalar("chroma_name"))),
        "depth": ("depth", str(bit_depth)),
        "fps_num": ("fps numerator", "nonzero"),
        "fps_den": ("fps denominator", "nonzero"),
        "kf_interval": ("nominal key interval", "nonzero metadata"),
        "golden_interval": ("nominal golden interval", "nonzero metadata"),
        "flags": ("flags", "zero in version %s" % version),
        "header_crc32c": (
            "header CRC32C",
            "covers bytes %s" % crc_span("sequence", "crc_start", "crc_length"),
        ),
    }
    packet_rules = {
        "sync": ("sync", "ASCII `%s`" % section_scalar("fields.toml", "packet", "sync_ascii")),
        "payload_len": (
            "payload length",
            "%s through %s bytes" % (scalar("decoder_initial_bytes"), scalar("max_payload_bytes")),
        ),
        "frame_index": ("frame index", "strictly increasing, no wrap"),
        "flags": (
            "flags",
            "key bit %s, golden-refresh bit %s, show bit %s"
            % (
                section_scalar("fields.toml", "packet.flags", "key"),
                section_scalar("fields.toml", "packet.flags", "golden_refresh"),
                section_scalar("fields.toml", "packet.flags", "show"),
            ),
        ),
        "frame_qp": ("frame QP", "%s through %s" % (scalar("qp_min"), scalar("qp_max"))),
        "reserved": ("reserved", "zero"),
        "header_crc32c": (
            "header CRC32C",
            "covers bytes %s" % crc_span("packet", "header_crc_start", "header_crc_length"),
        ),
        "payload_crc32c": ("payload CRC32C", "covers payload only"),
        "payload": ("payload", "range-coded syntax"),
    }

    return """# Key Frame bitstream version {version}

This document is decoder-normative. It is generated from the inert assets in
`spec/v1/`; `spec/generate_docs.py --check` rejects drift. All integers are
little-endian unless a bit-level operation states otherwise. Key Frame is an
original educational format and is not compatible with a standardized codec.

## 1. Limits and sample format

Version {version} carries progressive, {bit_depth}-bit, JPEG-sited 4:2:0 frames. Width
and height are even and lie in [{min_width}, {max_width}] × [{min_height},
{max_height}]. Encoders pad to {superblock}-pixel superblocks by edge replication;
decoders crop output to the header dimensions. Coding blocks are
{coding_blocks} pixels; transform sizes are {transform_sizes}. A {superblock} coding
block contains {transforms_per_block} raster-ordered {largest_transform} transforms. Coefficient
magnitude is at most {coeff_cap}.

## 2. Sequence header

The sequence header is {sequence_bytes} bytes:

{sequence_table}

An unsupported version, invalid dimension, zero required field, unexpected
chroma/depth, reserved bit, or CRC mismatch rejects the stream before any
frame allocation.

## 3. Frame packets and synchronization

Each packet begins with a {packet_bytes}-byte fixed header followed by its payload:

{packet_table}

Show is always one and a keyframe always refreshes golden. A failed sync or
fixed-header candidate advances the scanner by one byte. A structurally valid
header whose payload CRC fails consumes its declared, capped extent. Once an
index is known, a gap invalidates both references and context continuity before
classifying the candidate. Only a validated keyframe can begin or recover a
decode chain.

## 4. CRC32C

CRC32C uses reflected Castagnoli polynomial `0x82f63b78`, initial register
`0xffffffff`, a reflected bytewise update, and final xor `0xffffffff`:

```text
crc = 0xffffffff
for byte in input:
    crc ^= byte
    repeat 8 times:
        crc = (crc >> 1) ^ (0x82f63b78 if (crc & 1) else 0)
return crc ^ 0xffffffff
```

The empty vector is `{crc_empty}`; ASCII `123456789` is `{crc_ascii}`. Further
header vectors are committed in `spec/v1/vectors.json`.

## 5. Range coder

The probability `p1` always means P(symbol=1), lies in [{probability_min},{probability_max}], and has total
{probability_total}. Encoder state starts as `low=0:u64`, `range={range_initial}:u32`, `cache=0`,
`pending=1`. For one context-coded symbol:

```text
p0 = {probability_total} - p1
bound = (range >> {probability_bits}) * p0
if symbol == 0: range = bound
else: low += bound; range -= bound
p1 = clamp({probability_min}, {probability_max}, p1 + floor((target - p1) / {adaptation_divisor}))
while range < 1<<{range_top_bits}:
    range <<= 8
    shift_low()
```

`target` is {probability_total} for one and zero for zero. Floor division is normative for a
negative difference. Bypass uses local `p1={bypass_probability}` and does not modify a context.

```text
shift_low():
    low32 = low & 0xffffffff
    carry = low >> 32                 # must be zero or one
    if low32 < 0xff000000 or carry != 0:
        emit_u8_wrapping(cache + carry)
        emit pending-1 bytes of u8_wrapping(0xff + carry)
        cache = low32 >> 24
        pending = 0
    pending += 1
    low = (low32 & 0x00ffffff) << 8
```

Finish calls `shift_low` exactly {finish_calls} times. The decoder initializes
`range={range_initial}` and forms `code:u32` with {initial_bytes} wrapping byte shifts. It uses
the same bound, chooses zero when `code < bound`, otherwise subtracts bound
from code and range, then renormalizes by wrapping shifts and bounded reads.
Unread CRC-bound bytes after dimension-derived syntax are arithmetic
finalization tail; their count has no separate cap.

## 6. Context bank

There are exactly {context_count} stable context ids. Their names, conditioning, and
initial `p1` values are literal in `contexts.toml`. Keyframes reset to those
initials. Valid P-frames begin from the prior committed bank. Frame decoding
uses a copy and commits only after complete syntax, payload, reconstruction,
and CRC validation; corruption discards the copy.

{context_groups}

## 7. Payload syntax

Superblocks are raster ordered. Each starts with a top-down quadtree split from
{superblock} to a minimum leaf of {smallest_block}, followed by leaves in tree order. A keyframe leaf
codes `intra_mode` then residual. A P-frame leaf codes `skip`; skip codes only
`ref_select`. Otherwise `is_inter` selects either `ref_select + mvd + residual`
or `intra_mode + residual`.

Residual order is Y, U, V, then derived transform blocks in raster order. Each
transform starts with `has_coeff`; zero ends that transform. One continues with
last x/y, scan significance, greater-than-one, greater-than-two, a magnitude
remainder, and one bypass sign per nonzero coefficient. The last coordinate is
implicitly significant. Scan position order is the {scan_direction} order
literal in `scans.toml`, one table per transform size, as {scan_encoding} pairs.
The magnitude remainder is exp-Golomb k=0 coded entirely in bypass: there is
no adaptive parameter, and a decoder needs no state to read it. Motion
magnitude uses the same exp-Golomb k=0 code, but its first three prefix bins
use contexts, the remaining prefix/suffix are bypass, and zero has no sign.
`0` sign means positive and `1` means negative.

## 8. Prediction and reconstruction

Intra modes are {intra_modes}.
They use reconstructed top/left samples; unavailable samples use the nearest
available value or {intra_fallback} when neither side exists, and a top-right reference
never crosses the superblock the block sits in.

DC fills the block with the rounded mean of the available top and left samples,
and with {intra_fallback} when neither side is available. Planar blends the four
corners: for side N, `(left[row]*(N-1-column) + top_right*(column+1) +
top[column]*(N-1-row) + bottom_left*(row+1) + N) / (2*N)`. Horizontal replicates
the left column; vertical replicates the top row.

Each angular mode projects onto one reference line. For declared angle `d`,
`p = column*{angular_denominator} + (row+1)*d`; the reference index is the Euclidean quotient of
`p` by {angular_denominator} and `f` is its Euclidean remainder. A nonnegative index `i` reads
`top[i]` and a negative one reads `left[-i-1]`, each clamped to the last
reference the block has. The declared angles are
{intra_angles}. Angular interpolation is
`(({angular_denominator}-f)*a + f*b + {angular_rounding}) >> {angular_shift}`.

Inter uses LAST or GOLDEN with one quarter-pel motion vector. Full-pixel
motion lies in [{mv_fullpel_min}, {mv_fullpel_max}] and a vector carries {mv_fractional_bits} fractional
bits, so a coded difference component whose magnitude exceeds
`({mv_fullpel_max} - ({mv_fullpel_min})) << {mv_fractional_bits}` is invalid syntax and rejects the
frame. The predictor is
the componentwise median of left, above, and above-right (falling back to
above-left); a candidate contributes only when available, inter-coded, and on
the selected reference, otherwise it contributes zero. Reference extension is
{edge_extension} pixels. The {filter_taps_word}-tap filter `{filter_taps}/{filter_denominator}` runs horizontal before
vertical. Its half phase remains at the current stage scale. Luma quarter and
three-quarter phases are the rounded 1:1 blends of integer→half and half→next;
the positive rounding bias is one before floor division by two. Motion vectors
remain in quarter-luma-pixel units on chroma, producing eighth-chroma-pixel
positions: phases between integer and half use rounded 3:1, 1:1, and 1:3
blends, with bias two before division by four where applicable. A two-axis
sample rounds `(value+{two_stage_rounding})>>{two_stage_shift}`; a one-axis sample uses `(value+{single_stage_rounding})>>{single_stage_shift}`.

## 9. Transform and quantization

Transform matrices for {transform_sizes_ascending} are the literal signed integers in
`transforms.toml`. Multiply/accumulate uses {accumulator}. Forward shifts are
`{forward_shift1}` then `{forward_shift2}`; inverse shifts are {inverse_shift1} then `{inverse_shift2}`, each with
rounding offset `1<<(shift-1)`. Reconstruction narrows at named stage ends,
clamps residual to [{post_inverse_min},{post_inverse_max}], adds prediction, then clips to [0,{sample_max}].

QP is {qp_min} through {qp_max}. The {qscale_count} dequant scales are literal in `quant.toml` and
dequant is `{dequant_formula}` in {accumulator}. There is no delta QP. Encoder
modeled-entropy costs are the {cost_rows}-row Q{cost_bits} arrays in `costs.toml`; they are an
RDO estimate and are never presented as per-block payload ownership.

## 10. Deblocking and reference state

Deblocking runs after full-frame reconstruction and before the result enters a
reference slot. Coding-block and derived-transform edges of at least {minimum_edge}
luma pixels are filtered vertical-then-horizontal. Strength {deblock_strengths}
comes from `deblock.toml`; a candidate is skipped when `|p0-q0| >= alpha(QP)`,
`|p1-p0| >= beta(QP)`, or `|q1-q0| >= beta(QP)`. Luma strength 1 is the
{luma_weak} `delta = {weak_delta}`
with `p0' = {weak_p0}` and `q0' = {weak_q0}`. Luma
strength 2 is the {luma_strong} smoother in the same asset. Chroma uses the
weak filter at every nonzero strength. Threshold arrays alpha, beta, and tc
are indexed by the rounded frame-QP average. Filtered output becomes LAST; a
key or authoritative golden-refresh packet also becomes GOLDEN.

## 11. Corruption state machine

Malformed sequence data is a stream error. An invalid packet header, payload
CRC, range read, syntax value, or reconstruction constraint reports a corrupt
frame, repeats the last shown image for display only when one exists,
invalidates both references and contexts, and enters `NeedsKeyframe`. A packet
gap is dependency loss. While recovery is required, accepted packet indices
still advance, but non-key payloads are not entropy-decoded. A valid keyframe
decodes from literal contexts without references and atomically installs its
image, contexts, and both reference slots only after full success.

## 12. Probe accounting

Probe output distinguishes Q{cost_bits} modeled entropy from canonical replay timing.
Leaf bytes, superblock-structure bytes, and frame flush bytes sum to canonical
replay payload length. Only a byte-identical replay permits that length to be
equated with input payload length. Delayed carry prevents symbol ownership;
noncanonical accepted tails remain unattributed.

## 13. Version history

| Bitstream | Status | Change |
| ---: | --- | --- |
| {version} | frozen | Initial {bit_depth}-bit 4:2:0 contract |

## 14. Frozen asset inventory

The SHA-256 values below identify the literal inputs used for this rendering.

| Asset | SHA-256 |
| --- | --- |
{inventory}
""".format(
        version=version,
        min_width=scalar("min_width"),
        max_width=scalar("max_width"),
        min_height=scalar("min_height"),
        max_height=scalar("max_height"),
        coeff_cap=scalar("coefficient_abs_max"),
        bit_depth=bit_depth,
        sample_max=(1 << bit_depth) - 1,
        superblock=superblock,
        smallest_block=min(coding_blocks),
        coding_blocks="%s, or %s" % (", ".join(str(size) for size in coding_blocks[:-1]), coding_blocks[-1]),
        transform_sizes="%s, or %s" % (", ".join(str(size) for size in transform_sizes[:-1]), transform_sizes[-1]),
        transform_sizes_ascending="%s, and %s"
        % (", ".join(str(size) for size in sorted(transform_sizes)[:-1]), max(transform_sizes)),
        largest_transform=largest_transform,
        transforms_per_block=word(per_side * per_side),
        sequence_bytes=section_scalar("fields.toml", "sequence", "size_bytes"),
        sequence_table=field_table("sequence", sequence_rules),
        packet_bytes=section_scalar("fields.toml", "packet", "fixed_size_bytes"),
        packet_table=field_table("packet", packet_rules),
        crc_empty=crc_vector("empty"),
        crc_ascii=crc_vector("ascii_123456789"),
        probability_min=scalar("probability_min"),
        probability_max=scalar("probability_max"),
        probability_total=probability_total,
        probability_bits=bit_width(probability_total),
        bypass_probability=probability_total // 2,
        adaptation_divisor=1 << constant("adaptation_shift"),
        range_initial="0x%08x" % constant("range_initial"),
        range_top_bits=bit_width(constant("range_top")),
        finish_calls=word(constant("encoder_finish_calls")),
        initial_bytes=word(constant("decoder_initial_bytes")),
        context_count=asset_scalar("contexts.toml", "count"),
        context_groups=context_groups(),
        intra_modes=intra_mode_list(),
        intra_angles=intra_angle_list(),
        intra_fallback=asset_scalar("intra.toml", "unavailable_fallback"),
        mv_fullpel_min=scalar("mv_fullpel_min"),
        mv_fullpel_max=scalar("mv_fullpel_max"),
        mv_fractional_bits=scalar("mv_fractional_bits"),
        scan_direction=asset_scalar("scans.toml", "direction").replace("_", " "),
        scan_encoding=asset_scalar("scans.toml", "coordinate_encoding").replace("flat_", "flat ").replace("_", "/"),
        angular_denominator=asset_scalar("intra.toml", "angular_denominator"),
        angular_rounding=asset_scalar("intra.toml", "angular_rounding_offset"),
        angular_shift=bit_width(asset_int("intra.toml", "angular_denominator")),
        edge_extension=asset_scalar("mc.toml", "edge_extension_pixels"),
        filter_taps="[%s]" % ",".join(str(tap) for tap in asset_list("mc.toml", "filter_taps")),
        filter_taps_word=word(len(asset_list("mc.toml", "filter_taps"))),
        filter_denominator=asset_scalar("mc.toml", "filter_denominator"),
        single_stage_rounding=asset_scalar("mc.toml", "single_stage_rounding"),
        single_stage_shift=asset_scalar("mc.toml", "single_stage_shift"),
        two_stage_rounding=asset_scalar("mc.toml", "two_stage_rounding"),
        two_stage_shift=asset_scalar("mc.toml", "two_stage_shift"),
        accumulator=asset_scalar("transforms.toml", "accumulator"),
        forward_shift1=asset_scalar("transforms.toml", "forward_shift1"),
        forward_shift2=asset_scalar("transforms.toml", "forward_shift2"),
        inverse_shift1=asset_scalar("transforms.toml", "inverse_shift1"),
        inverse_shift2=asset_scalar("transforms.toml", "inverse_shift2"),
        post_inverse_min=asset_scalar("transforms.toml", "post_inverse_min"),
        post_inverse_max=asset_scalar("transforms.toml", "post_inverse_max"),
        qp_min=scalar("qp_min"),
        qp_max=scalar("qp_max"),
        qscale_count=len(asset_list("quant.toml", "qscale")),
        dequant_formula=asset_scalar("quant.toml", "dequant_formula"),
        cost_rows=asset_scalar("costs.toml", "probability_max"),
        cost_bits=asset_scalar("costs.toml", "fractional_bits"),
        minimum_edge=asset_scalar("deblock.toml", "minimum_edge_px"),
        deblock_strengths="%s, or %s"
        % (
            ", ".join(str(value) for value in asset_list("deblock.toml", "strengths")[:-1]),
            asset_list("deblock.toml", "strengths")[-1],
        ),
        luma_weak=tap_shape(asset_scalar("deblock.toml", "luma_strength_1")),
        luma_strong=tap_shape(asset_scalar("deblock.toml", "luma_strength_2")),
        weak_delta=section_scalar("deblock.toml", "weak", "delta"),
        weak_p0=section_scalar("deblock.toml", "weak", "p0_out"),
        weak_q0=section_scalar("deblock.toml", "weak", "q0_out"),
        inventory="\n".join(inventory),
    )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--template", action="store_true")
    args = parser.parse_args()
    if args.template:
        sys.stdout.write(template())
        return 0
    rendered = render()
    if args.write:
        OUTPUT.write_text(rendered, encoding="utf-8")
        print("bitstream-doc: wrote %s" % OUTPUT)
        return 0
    if args.check or not args.write:
        if not OUTPUT.exists() or OUTPUT.read_text(encoding="utf-8") != rendered:
            print("bitstream-doc: generated document drifted; run spec/generate_docs.py --write", file=sys.stderr)
            return 1
        print("bitstream-doc: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())

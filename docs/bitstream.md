# Key Frame bitstream version 1

This document is decoder-normative. It is generated from the inert assets in
`spec/v1/`; `spec/generate_docs.py --check` rejects drift. All integers are
little-endian unless a bit-level operation states otherwise. Key Frame is an
original educational format and is not compatible with a standardized codec.

## 1. Limits and sample format

Version 1 carries progressive, 8-bit, JPEG-sited 4:2:0 frames. Width
and height are even and lie in [64, 4096] × [64,
2304]. Encoders pad to 64-pixel superblocks by edge replication;
decoders crop output to the header dimensions. Coding blocks are 64, 32, 16,
or 8 pixels. Transform sizes are 32, 16, 8, or 4; a 64 coding block contains
four raster-ordered 32 transforms. Coefficient magnitude is at most 32767.

## 2. Sequence header

The sequence header is 24 bytes:

| Offset | Width | Field | Rule |
| ---: | ---: | --- | --- |
| 0 | 4 | magic | ASCII `KFV1` |
| 4 | 2 | version | exactly 1 |
| 6 | 2 | width | section 1 |
| 8 | 2 | height | section 1 |
| 10 | 1 | chroma | 1, meaning `C420jpeg` |
| 11 | 1 | depth | 8 |
| 12 | 2 | fps numerator | nonzero |
| 14 | 2 | fps denominator | nonzero |
| 16 | 2 | nominal key interval | nonzero metadata |
| 18 | 1 | nominal golden interval | nonzero metadata |
| 19 | 1 | flags | zero in version 1 |
| 20 | 4 | header CRC32C | covers bytes 0–19 |

An unsupported version, invalid dimension, zero required field, unexpected
chroma/depth, reserved bit, or CRC mismatch rejects the stream before any
frame allocation.

## 3. Frame packets and synchronization

Each packet begins with a 24-byte fixed header followed by its payload:

| Offset | Width | Field | Rule |
| ---: | ---: | --- | --- |
| 0 | 4 | sync | ASCII `KFP1` |
| 4 | 4 | payload length | 5 through 16777216 bytes |
| 8 | 4 | frame index | strictly increasing, no wrap |
| 12 | 1 | flags | key bit 0, golden-refresh bit 1, show bit 2 |
| 13 | 1 | frame QP | 0 through 63 |
| 14 | 2 | reserved | zero |
| 16 | 4 | header CRC32C | covers bytes 4–15 |
| 20 | 4 | payload CRC32C | covers payload only |
| 24 | variable | payload | range-coded syntax |

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

The empty vector is `00000000`; ASCII `123456789` is `e3069283`. Further
header vectors are committed in `spec/v1/vectors.json`.

## 5. Range coder

The probability `p1` always means P(symbol=1), lies in [1,4095], and has total
4096. Encoder state starts as `low=0:u64`, `range=0xffffffff:u32`, `cache=0`,
`pending=1`. For one context-coded symbol:

```text
p0 = 4096 - p1
bound = (range >> 12) * p0
if symbol == 0: range = bound
else: low += bound; range -= bound
p1 = clamp(1, 4095, p1 + floor((target - p1) / 32))
while range < 1<<24:
    range <<= 8
    shift_low()
```

`target` is 4096 for one and zero for zero. Floor division is normative for a
negative difference. Bypass uses local `p1=2048` and does not modify a context.

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

Finish calls `shift_low` exactly five times. The decoder initializes
`range=0xffffffff` and forms `code:u32` with five wrapping byte shifts. It uses
the same bound, chooses zero when `code < bound`, otherwise subtracts bound
from code and range, then renormalizes by wrapping shifts and bounded reads.
Unread CRC-bound bytes after dimension-derived syntax are arithmetic
finalization tail; their count has no separate cap.

## 6. Context bank

There are exactly 144 stable context ids. Their names, conditioning, and
initial `p1` values are literal in `contexts.toml`. Keyframes reset to those
initials. Valid P-frames begin from the prior committed bank. Frame decoding
uses a copy and commits only after complete syntax, payload, reconstruction,
and CRC validation; corruption discards the copy.

The groups are partition split (12), skip (3), P-frame inter choice (3), intra
mode (10), reference choice (2), motion prefix (6), coefficient presence (8),
last position (32), significance (44), greater-than-one (16), and
greater-than-two (8). Bypass suffix and sign bins have no context id.

## 7. Payload syntax

Superblocks are raster ordered. Each starts with a top-down quadtree split from
64 to a minimum leaf of 8, followed by leaves in tree order. A keyframe leaf
codes `intra_mode` then residual. A P-frame leaf codes `skip`; skip codes only
`ref_select`. Otherwise `is_inter` selects either `ref_select + mvd + residual`
or `intra_mode + residual`.

Residual order is Y, U, V, then derived transform blocks in raster order. Each
transform starts with `has_coeff`; zero ends that transform. One continues with
last x/y, scan significance, greater-than-one, greater-than-two, Golomb-Rice
remainder, and one bypass sign per nonzero coefficient. The last coordinate is
implicitly significant. Motion magnitude is exp-Golomb k=0: its first three
prefix bins use contexts, the remaining prefix/suffix are bypass, and zero has
no sign. `0` sign means positive and `1` means negative.

## 8. Prediction and reconstruction

Intra modes are DC, planar, horizontal, vertical, D45, D135, D117, and D153.
They use reconstructed top/left samples; unavailable samples use the nearest
available value or 128 when neither side exists. Angular interpolation is
`((32-f)*a + f*b + 16) >> 5`.

Inter uses LAST or GOLDEN with one quarter-pel motion vector. The predictor is
the componentwise median of left, above, and above-right (falling back to
above-left); a candidate contributes only when available, inter-coded, and on
the selected reference, otherwise it contributes zero. Reference extension is
64 pixels. The six-tap filter `[1,-5,20,20,-5,1]/32` runs horizontal before
vertical. A two-stage sample rounds `(value+512)>>10`; a one-stage sample uses
`(value+16)>>5`.

## 9. Transform and quantization

Transform matrices for 4, 8, 16, and 32 are the literal signed integers in
`transforms.toml`. Multiply/accumulate uses i64. Forward shifts are
`log2(N)+1` then `log2(N)+8`; inverse shifts are 7 then `13-log2(N)`, each with
rounding offset `1<<(shift-1)`. Reconstruction narrows at named stage ends,
clamps residual to [-32768,32767], adds prediction, then clips to [0,255].

QP is 0 through 63. The 64 dequant scales are literal in `quant.toml` and
dequant is `(coefficient*scale+8)>>4` in i64. There is no delta QP. Encoder
modeled-entropy costs are the 4095-row Q16 arrays in `costs.toml`; they are an
RDO estimate and are never presented as per-block payload ownership.

## 10. Deblocking and reference state

Deblocking runs after full-frame reconstruction and before the result enters a
reference slot. Coding-block and derived-transform edges use strength 0, 1, or
2 from `deblock.toml`; luma uses weak four-tap or strong six-tap filtering,
while chroma uses weak filtering. Threshold arrays alpha, beta, and tc are
indexed by the rounded frame-QP average. Filtered output becomes LAST; a key or
authoritative golden-refresh packet also becomes GOLDEN.

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

Probe output distinguishes Q16 modeled entropy from canonical replay timing.
Leaf bytes, superblock-structure bytes, and frame flush bytes sum to canonical
replay payload length. Only a byte-identical replay permits that length to be
equated with input payload length. Delayed carry prevents symbol ownership;
noncanonical accepted tails remain unattributed.

## 13. Version history

| Bitstream | Status | Change |
| ---: | --- | --- |
| 1 | frozen at F0 | Initial 8-bit 4:2:0 contract |

## 14. Frozen asset inventory

The SHA-256 values below identify the literal inputs used for this rendering.

| Asset | SHA-256 |
| --- | --- |
| `constants.toml` | `51a1a36be5afe388763fc933e287f93d87470a19c3c7d33133a027886f585d5e` |
| `contexts.toml` | `a7f5d7054b767990120185e9deb7f8a5110e0250ad1b3bf96f6fe09a5fc49abf` |
| `costs.toml` | `7c64b47fcbf16abe8ce4129c94411c85009474a8628c7054325e2c8f4780968a` |
| `deblock.toml` | `cbf282fdbe220f3f0bad92abfc331b3e35124a6b6e20d9f337d8660e26bcec5a` |
| `fields.toml` | `43f34961d982ae143e39ce586577f00933e49953f21ee8e808537f410c46b0fc` |
| `intra.toml` | `806381ebe7948c9d473284ed00c9b66e2a53d6f3e4339407d0eb43c10f090ac4` |
| `manifest.toml` | `a35f272d672849c5eb429eac05259430e2ff6eede10df847271bb3d14646e431` |
| `mc.toml` | `a3dba431b9dcf8a4cfa0d0593dbed1c127314b4f3e1b296eab4af36832e534b9` |
| `quant.toml` | `17432a941a4e1bd0beb6730139fd803097c0a15b863db37c18f46b07d1ad0f11` |
| `scans.toml` | `231a562dfbc01351511587be5a201bff1907168b670580bd8967a6551923fb78` |
| `search.toml` | `fc54535e12f2ba02ddb0be0abb865c0e7c791ec43b3b854e49849ca716ee7f69` |
| `syntax.toml` | `92467acd24f90bf69448624f536de514ffcb72849b17be202a5cd8e29e08689a` |
| `transform-vectors.toml` | `c7d775b5895a34e9865f358feccd5db1a1fd969ed8b79fb33de95581e66e2786` |
| `transforms.toml` | `32be222922aa6e454d843786d1b3d4f0e21a6cfba756b46b9efd2a766e187910` |
| `vectors.json` | `4ffa43d95a72bee5ce0b333d8cc25789dcad453fdcd037cccab121c737b7255d` |

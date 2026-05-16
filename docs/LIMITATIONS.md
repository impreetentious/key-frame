# Limitations

Everything below is a real constraint of Key Frame version one. None of it is a
bug, and none of it is a plan. Each entry says what the limitation is, why the
design accepts it, and — where one exists — what lifting it would take.

This file exists because the alternative is worse. A repository that measures
itself carefully and then leaves the reader to discover the boundaries by
running into them has spent its rigour on the wrong half of the problem.

## Compression efficiency

**The rate–distortion gap to mature encoders is large, and it is not going to
close.** Version one has no B-frames, one motion vector per block, flat
quantization with no per-superblock adaptation, a single-pass rate controller,
and a mode decision priced with modeled entropy rather than trial encodes. Each
of those is a deliberate simplification in favour of a decision a reader can
follow, and each costs measurable efficiency.

This repository never claims to be competitive with, to beat, or to rival a
standardized encoder. Those three words are banned from it as performance
claims — the only places they appear are statements of the rule itself, here and
in [ADR-0001](adr/0001-foundation-decisions.md) — and the rate–distortion tab of
the [projection room](../inspector/) carries a caveat box saying plainly that
the comparison on offer is this codec against itself.

Where the individual costs are measured, they are measured:
`bench/results/rd-campaign.json` records what each encoder tool is worth, and
`rd_verify` re-encodes the points and fails if any figure has moved.

- No B-frames or reordered prediction. Adding them would be the largest
  bitstream revision the format could take — reordering changes decode order,
  which the [normative document](bitstream.md) fixes in its decoding-order
  section, along with everything built on it.
- One motion vector per coding block, quarter-pixel, six-tap.
  [Prediction and reconstruction](bitstream.md) defines it.
- Flat quantization across the frame. Per-superblock delta-QP would be a syntax
  addition and therefore a bitstream-version bump, not the activation of a
  hidden field. [Transform and quantization](bitstream.md) defines what exists.
- Single-pass leaky-bucket rate control, encoder-side and outside the normative
  document entirely: a decoder never learns how a QP was chosen.
- Modeled-entropy rate–distortion decisions rather than trial encodes, also
  encoder-side.

## What can and cannot be ablated

**The loop filter cannot be turned off, so its contribution is not on the
charts.** Deblocking in version one is unconditional: there is no frame-header
flag for it, because a decoder permitted to skip the filter would reconstruct a
different picture from identical bytes. Every ablation the repository publishes
narrows what the *encoder* will choose while leaving the format untouched, so
every ablated stream is an ordinary `.kfv` file that both decoders reconstruct
bit-exactly.

Measuring "filter off" would mean publishing a curve produced by a decoder that
disagrees with the specification. The reasoning is recorded in full in
[ADR-0015](adr/0015-conformant-encoder-ablations.md).

The five ablations that are available — golden reference, skip, inter
prediction, sub-pixel refinement, and quadtree splitting — are exactly the
choices the bitstream leaves to an encoder.

## Speed

**Scalar and single-threaded, everywhere, by construction.** There is no SIMD
path and no tiling. Encoding is dominated by the rate–distortion search, which
evaluates every candidate through the real reconstruction path rather than an
approximation, because the closed loop is the invariant the whole project
protects.

The forbidden-API scan keeps threads, ambient clocks, and the filesystem out of
every codec crate, so this is not an omission that could be quietly reversed. A
parallel or vectorized path would have to prove bit-exact equality with the
scalar one across the full suite before it could ship.

## Format scope

- **8-bit only.** The bit-depth field exists in the sequence header and version
  one accepts exactly one value. See
  [limits and sample format](bitstream.md).
- **4:2:0 only,** `C420jpeg` siting, progressive, even dimensions from 64×64 to
  4096×2304.
- **No container.** A `.kfv` file is a sequence header followed by sync-framed
  packets, as [frame packets and synchronization](bitstream.md) describes.
  There is no timing model beyond the frame rate in the header, no audio, no
  track multiplexing, and no seeking index beyond what a decoder builds by
  scanning.
- **No network streaming and no GPU path.**
- **Nothing else decodes it.** The bitstream is original. That is the point —
  it is small enough to specify completely — but it means a `.kfv` file is
  useful only with this repository's decoders or one written against
  [`docs/bitstream.md`](bitstream.md).

## Corruption and recovery

**Damage is detected, contained, and reported — never concealed.** A frame that
fails either CRC32C is not partially applied, never enters the reference state,
and never reaches later prediction. What the decoder does not do is
reconstruct anything from it: the frames between the damage and the next valid
keyframe are reported as lost rather than estimated, and no error concealment of
any kind is attempted.

Recovery happens only at a keyframe that itself validates. A stream damaged in
its first keyframe produces no pictures at all until the next one arrives, which
for the default keyframe interval can be a long time. The
[corruption state machine](bitstream.md) defines every case, and the error
matrix asserts each of them in both decoders.

## Measurement and reporting

- **Every published number is self-measured.** There is no third-party
  verification of any figure in this repository. What there is instead: two
  independently written decoders that must agree sample for sample before a
  number is recorded, an independently written metric oracle
  (`bench/metric_oracle.py`) that the Rust implementation is checked against,
  and a receipt for every figure that re-runs on demand.
- **The corpus is two QCIF clips.** `akiyo_qcif` and `foreman_qcif`, pinned by
  checksum and fetched rather than redistributed. They were chosen as a
  contrast — a nearly static head-and-shoulders scene against one with real
  camera and subject motion — not as a representative sample of video. A wider
  corpus would give more trustworthy averages; it would also make the campaign
  slow enough to stop being re-run, and a receipt nobody regenerates is the
  failure this design is most concerned with.
- **Quality is luma only,** cropped to the displayed picture. Chroma error is
  not reported. PSNR and SSIM are pinned to the last constant, and anything
  computed differently is required to call itself something else.
- **Bitrate figures assume the clip's own frame rate** and cover the whole
  stream including its sequence header. The average-bitrate accuracy figures
  are the exception: they exclude the sequence header, because the controller
  is never given a budget for bytes it does not emit.
- **Average-bitrate accuracy is a steady-state figure with a measured
  envelope.** The controller is a single-pass leaky bucket that starts from an
  initial fill and converges, so it is measurably worse over a short clip than a
  long one: on the pinned corpus the error runs from 5% to 13% over 24 frames
  and settles by 48. The published accuracy is measured over 48 frames and the
  receipt records the count. If you encode a clip shorter than about a second
  and a half at a bitrate target, expect it to undershoot.

  Even converged, it is not uniformly within 5%. Across twelve operating points
  on the corpus the mean absolute error is 2.2% and the worst is 5.3%, with two
  points above 5% — both aggressive targets on the high-motion clip. The cause
  is structural rather than a tuning failure: the quantizer moves in steps of
  two with no finer control, so at some targets no achievable sequence of
  quantizers lands inside 5%. Closing that would take per-superblock quantizer
  adjustment, which is a syntax addition and therefore a bitstream-version bump.
  See [ADR-0016](adr/0016-complexity-term-in-rate-control.md).

## Accounting semantics

**"How many bits does this block cost?" has no answer, and the tools refuse to
invent one.** An adaptive range coder emits bytes when its interval demands it,
not when a symbol is coded, so the bytes that leave the encoder during a block
are an artifact of when the coder happened to flush. The probe reports two
separate quantities — modeled entropy, which is what the encoder's model
predicted, and emission-time bytes, which is when the flush occurred — and never
adds them into a single figure labelled as the block's cost. The projection
room's heatmap makes the reader choose which one they are looking at.

Canonical replay is reported the same way. A payload that replays
byte-identically is labelled as matching; a valid stream with a noncanonical
tail reports a mismatch without attributing it to any block.

## Conformance scope

**Coverage is a measurement, not a claim, and the measurement has a
denominator.** The committed streams code every syntax element and every context
slot the format can reach: 14 of 14 elements and 116 of 116 reachable context
ids, measured by decoding every vector in both decoders.

Twenty-eight of the 144 context slots are held in reserve. The frozen context
bank was sized for neighbour conditioning that version one declares but does not
use, so no conformant stream can reach those slots. Coverage is reported against
the 116 that are reachable rather than as a percentage of the whole bank, which
would be a lower number describing nothing.

Conformance covers the decoder's output. It does not certify that an encoder
written elsewhere would make good decisions, only that any stream it produced
would decode identically here.

## Platforms

**Linux x86_64 and macOS arm64 are the supported native targets.** Both are
reproduced in continuous integration and required to be byte-identical.
Windows is not supported and is not tested. The WebAssembly module targets
`wasm32-unknown-unknown` and imports nothing.

---

Each limitation above is either enforced by a gate or recorded in a decision
record. If you find one that is neither, that is a defect in this file.

# Key Frame — Codec Specification

The single normative document for this repository. It states what the project
does, where it stops, and every decision that produced those two answers.

Read §1 before interpreting a report, a benchmark, or a green test.

## 1. Status and how to read this

This project is at a verified stopping point, not a finished state. What
follows is true of the tree today; it is not a promise about a later tree.

The middle sections state what is proved and where it stops. The decision log
carries a **disposition**, which is the only part written for a reviewer rather
than a user:

| Disposition | Meaning |
|---|---|
| **Open** | A different answer is available and costs only engineering. No claim of this project is touched. Reopen freely. |
| **Load-bearing** | A different answer is available in code, but taking it deletes a claim this project makes. Owner decision only. Do not propose reversing one as an improvement. |
| **Bound** | Not a decision. Resolving it needs capability that does not exist in this repository. |
| **Settled** | The decision already went the permissive way. Nothing to reopen; the row exists so it is not re-litigated. |

Dispositions in §13 were assigned from the decision records themselves, not
from a fresh audit of the tree. Checking them against the code is the first
task of any review of this document.

## 2. The central claim

A video codec decides, for every block of every frame, how to predict it, how
to transform whatever the prediction missed, and how many bits that decision
costs. The compressed file records none of that reasoning. This codec is built
so the reasoning can be read.

Normative behaviour is fixed by frozen vectors and checked by two independent
decoders that must agree bit for bit. Prose is not the specification; the
vectors are.

Where this document says something is refused, unchecked, or absent, that is
the claim working rather than a gap in it. A proposal that removes a refusal is
not thereby an improvement.

## 3. Compression efficiency

**The rate–distortion gap to mature encoders is large, and it is not going to
close.** Version one has no B-frames, one motion vector per block, flat
quantization with no per-superblock adaptation, a single-pass rate controller,
and a mode decision priced with modeled entropy rather than trial encodes. Each
of those is a deliberate simplification in favour of a decision a reader can
follow, and each costs measurable efficiency.

This repository never claims to be competitive with, to beat, or to rival a
standardized encoder. Those three words are banned from it as performance
claims — the only places they appear are statements of the rule itself, here and
in the decision log (§13, row 0001) — and the rate–distortion tab of
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

## 4. What can and cannot be ablated

**The loop filter cannot be turned off, so its contribution is not on the
charts.** Deblocking in version one is unconditional: there is no frame-header
flag for it, because a decoder permitted to skip the filter would reconstruct a
different picture from identical bytes. Every ablation the repository publishes
narrows what the *encoder* will choose while leaving the format untouched, so
every ablated stream is an ordinary `.kfv` file that both decoders reconstruct
bit-exactly.

Measuring "filter off" would mean publishing a curve produced by a decoder that
disagrees with the specification. The reasoning is recorded in full in
the decision log (§13, row 0015).

The five ablations that are available — golden reference, skip, inter
prediction, sub-pixel refinement, and quadtree splitting — are exactly the
choices the bitstream leaves to an encoder.

## 5. Speed

**Scalar and single-threaded, everywhere, by construction.** There is no SIMD
path and no tiling. Encoding is dominated by the rate–distortion search, which
evaluates every candidate through the real reconstruction path rather than an
approximation, because the closed loop is the invariant the whole project
protects.

The forbidden-API scan keeps threads, ambient clocks, and the filesystem out of
every codec crate, so this is not an omission that could be quietly reversed. A
parallel or vectorized path would have to prove bit-exact equality with the
scalar one across the full suite before it could ship.

## 6. Format scope

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

## 7. Corruption and recovery

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

## 8. Measurement and reporting

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
  long one. `scripts/ci/rate-gate.sh` measures both lengths at each disclosed
  operating point on every run and requires the 24-frame error to be worse than
  the 48-frame one; both figures are printed where you can read them. The
  published accuracy is the 48-frame measurement and the receipt records the
  count. If you encode a clip shorter than about a second and a half at a
  bitrate target, expect it to miss by more — in either direction. This page
  used to say it would undershoot; of the two pinned clips, the low-motion one
  undershoots at 24 frames and the high-motion one overshoots, and the gate now
  prints the sign of each miss so the direction is something a reader sees.

  No short-clip percentage is published here. This paragraph used to state that
  the error "runs from 5% to 13% over 24 frames", and nothing in the repository
  produced either number: the receipt measures 48 frames, the decision record
  behind the controller measures a different sweep, and re-measuring the six
  campaign targets at 24 frames gives a range that contains neither bound. A
  figure with nothing behind it is the thing this page exists to not have, so
  the checked property replaced it.

  Even converged, it is not uniformly within 5%. The campaign measures six
  average-bitrate operating points — three targets on each of the two pinned
  clips, forty-eight frames each — and across them the mean absolute error is
  2.32% and the worst is 5.32%, with one point above 5% on the high-motion clip.
  Those three figures are the receipt's own: `rd_verify` re-encodes each target
  and compares every recorded number, and `crates/kf-tools/tests/receipt_shape.rs`
  recomputes this aggregate and fails if the sentence you are reading stops
  describing it. The cause is structural rather than a tuning failure: the
  quantizer moves in steps of two with no finer control, so at some targets no
  achievable sequence of quantizers lands inside 5%. Closing that would take
  per-superblock quantizer adjustment, which is a syntax addition and therefore a
  bitstream-version bump. The wider twelve-point sweep that chose the complexity
  term is in the decision log (§13, row 0016) as the
  record of that decision; it compared three encoder variants, two of which no
  longer exist, so it is history rather than a figure this build can recheck.

## 9. Accounting semantics

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

## 10. Conformance scope

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

## 11. Platforms

**Linux x86_64 and macOS arm64 are the supported native targets.** The
conformance suite is reproduced, and required to be byte-identical, on whichever
of them runs `./scripts/preflight.sh`. The two-target matrix that would run both
against each other on every change is configured and has never executed; see the
next section. Windows is not supported and is not tested. The WebAssembly module
targets `wasm32-unknown-unknown` and imports nothing.

## 12. Continuous integration

**No hosted pipeline has ever run.** The repository ships three GitHub Actions
workflows — the full gate list plus the two-target bit-exactness matrix, a
nightly decoder campaign and sixty-point receipt recheck, and the projection-room
deploy — and its only remote is a GitLab project, where GitHub Actions do not
run. The definitions are correct and reviewed; nothing has executed them.

What follows from that is stated rather than worked around. Every claim in this
repository names a check `./scripts/preflight.sh` runs, because that is the
command a reader can run themselves. Three things the workflows would do are
larger than preflight: the second native target, the nightly campaign's full
declared budget against preflight's smaller one, and all sixty rate–distortion
points against the two ladder ends the change-time gate re-encodes.

Two of those three have been executed by hand on a development machine, and the
pages that describe them say when and at what budget rather than implying a
schedule: the decoder campaigns at their full declared count
([writeup 5](WRITEUPS.md#5-fuzzing-my-own-decoder)) and the sixty-point
receipt recheck ([writeup 6](WRITEUPS.md#6-honest-ratedistortion)). The
first cannot be: reproducing a stream on a second native target needs a second
native target, and running it by hand on one machine would prove nothing about
the other. The decision log (§13, row 0018) records the decision
and what was considered instead.

---

Each limitation above is either enforced by a gate or recorded in a decision
record. If you find one that is neither, that is a defect in this file.

## 13. Decision log

Every recorded decision, the alternative it rejected, and whether a
different answer is available. Superseded rows are kept: a decision that
was reversed is still a decision somebody has to not make again.

| # | Decision | Rejected alternative | Disposition |
|---|---|---|---|
| 0001 | Foundation decisions | Compatibility with an existing standard, floating-point signal math, hand-written normative documentation, a single decoder verified against itself, a shared helper library between the two decoders,… | **Load-bearing** |
| 0002 | Worked-vector augmentation | - Treat implementation unit tests as the vectors: rejected because code would become the sole source of expected behavior. - Bump bitstream version: rejected because no decoded value or syntax… | **Load-bearing** |
| 0003 | Packet vector completion | - Keep only packet round trips: rejected because a reader and writer can share the same wrong layout. - Bump the bitstream version: rejected because this records the already-frozen layout instead of… | **Load-bearing** |
| 0004 | Intra oracle stream | - Generate the first stream with `kfenc`: rejected because the encoder does not yet exist and would not be independent. - Hand-enter bytes without an emitting oracle: rejected because later review… | **Load-bearing** |
| 0005 | Freeze the probe version-one wire format | — | **Settled** |
| 0006 | Complete the version-one motion-compensation phases | — | **Settled** |
| 0007 | Pin canonical motion-search centers | — | **Settled** |
| 0008 | Deblock filter vectors | - Leave formulas in prose only: rejected because two independent decoders would not have a byte-level expected output. - Bump bitstream version: rejected because no syntax element changed; the asset… | **Load-bearing** |
| 0009 | Reconstruction freeze | - Keep staging hashes and relabel them as frozen: rejected because the label would hide that they were collected before the filter was live. - Encode the oracle and hand cases with `kfenc`: rejected… | **Load-bearing** |
| 0010 | Rate-control tables | - Leave first-frame QP as an implicit mid-range constant: rejected because extreme bitrates would start far from the bucket's steady QP. - Scale the per-frame budget by the complexity EWMA in version… | **Open** |
| 0011 | Initial Rate-Control Bucket Fill | - Start empty: rejected because it forces a QP collapse on every encode. - Start full: rejected because it would raise QP before any frame is coded. | **Open** |
| 0012 | Reserved Context Slots | - Renumber the bank to the 116 live ids: rejected. The numbering is frozen and drives both decoders and the generated normative document; renumbering would invalidate every existing stream to save a… | **Load-bearing** |
| 0013 | Raw WebAssembly Boundary | - `wasm-bindgen` as originally planned: rejected because its imports make the shipped module unrunnable by the equality gate, which would have to test a second build instead. - A WASI binary with a… | **Open** |
| 0014 | Probe Crate Extraction | - Fold the probe into `kf-dec`: rejected. Reporting would then sit on the critical path of every consumer that only wants pixels, and the fast decoder's API is a bit-exactness contract, not a place… | **Open** |
| 0015 | Conformant Encoder Ablations | - Build the loop filter ablation anyway, behind a feature flag. Rejected. The streams would not decode correctly in either shipped decoder, so the curve would describe a build that does not exist. A… | **Load-bearing** |
| 0016 | Complexity Term in Rate Control | - Delete the average as dead code. Defensible, and it was the cheaper option. Rejected because the reactive-only controller is measurably worse, and because the design that was written down is the… | **Open** |
| 0017 | Checked Arithmetic in Release Builds | - A dedicated checked profile for the campaigns only. Rejected. It would leave the corpus gate, the rate measurements, and the shipped module on unchecked arithmetic, and it would mean the campaign… | **Load-bearing** |
| 0018 | Preflight is the arbiter, not a hosted pipeline | - Port the pipeline to `.gitlab-ci.yml` and retire the workflows. Rejected. The macOS runner needed for the second half of the bit-exactness matrix is not on the free tier, so the matrix would still… | **Open** |

## 14. How to review this document

A reviewer is asked to find rows where a different decision is available and
worth taking. The rules of that review:

1. **Only Open rows are in scope.** Load-bearing rows are the claim in §2;
   proposing their reversal is proposing a different project. Bound rows are
   not decisions. Settled rows already went the permissive way.
2. **Check the dispositions first.** They were read off the decision records,
   not off the code. A mislabelled row is the most likely defect here.
3. **Name the failure, not the preference.** Reopen a row only if you can state
   what concretely goes wrong for a solo owner of a private repository if it
   stays as it is.
4. **A refusal is a feature until proven otherwise.** A proposal that reduces
   the count of things this project declines to do is not thereby an
   improvement.
5. **Disposition changes are the owner's call.** If a row looks mislabelled,
   say so and stop. Do not act on the relabelling.

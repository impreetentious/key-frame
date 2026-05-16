# ADR-0016: Complexity Term in Rate Control

## Context

The average-bitrate controller is a fixed-point leaky bucket. Its design has
always been "one per-frame quantizer chosen from bucket fullness **and** a
complexity average", and the code computed both: an exponentially weighted
average of each frame's transition cost, updated every frame with a shift of
three.

An audit of the whole rate path found that the average was never read. It was
written on every frame and then discarded. The quantizer came from bucket
fullness alone.

That is worse than an unimplemented feature, because nothing was obviously
missing. The controller worked, the accuracy gate passed at its two operating
points, and the field sat in the struct looking load-bearing.

Fullness on its own makes the controller strictly reactive. It raises the
quantizer only after the overspend has already been paid for, so every time the
content changes difficulty it spends a frame or two at the wrong quantizer
before catching up. On a clip that alternates between easy and hard passages —
which is most real video — that is where the error accumulates.

## Decision

The complexity average biases the bucket thresholds.

A frame more than a quarter harder than the running average shifts the decision
thresholds as though the bucket were **one sixteenth of capacity fuller**, so the
quantizer rises sooner. A frame more than a quarter easier shifts them by the
same amount the other way. Anything in between changes nothing. The biased value
is clamped back inside the bucket, so the bias can move a decision one band
early but can never push it somewhere fullness alone could not reach.

The sixteenth is measured, not chosen. Across twelve operating points on the
pinned corpus — six bitrate targets on each of two clips, forty-eight frames
each — mean absolute rate error came out as:

| Bias | Mean error | Worst error | Points over 5% |
| --- | --- | --- | --- |
| none (the old behaviour) | 3.28% | 6.74% | 1 |
| capacity / 12 | 2.81% | 5.31% | 2 |
| **capacity / 16** | **2.21%** | **5.31%** | 2 |
| capacity / 24 | 3.14% | 5.36% | 2 |

## Consequences

Mean absolute error falls by a third and the worst case improves from 6.74% to
5.31%.

**It is not an improvement everywhere, and that is worth stating plainly.** The
two operating points the rate-control gate has always tested were unusually good
under the old behaviour — 0.69% and 0.33% — and are now 2.31% and 1.37%. Both
remain comfortably inside the ±5% the gate requires, and the wider sweep is
better, so this is a real trade rather than a free win: the controller is no
longer overfitted to two points that happened to be measured.

Two operating points still exceed 5%, both of them aggressive targets on the
high-motion clip. That is a structural limit rather than a tuning failure: the
quantizer moves in steps of two with no finer control, so at some targets no
achievable sequence of quantizers lands inside 5%. It is recorded in
[`docs/LIMITATIONS.md`](../LIMITATIONS.md) with the measured envelope rather
than left for someone to discover.

Every average-bitrate stream changes. Constant-quantizer encoding is untouched,
so every committed conformance stream and every bit-exactness gate is unaffected
— they are all constant-quantizer by construction. The campaign receipts were
regenerated.

Determinism is preserved and tested: the bias reads only the controller's own
history, so two runs over the same frames produce identical quantizer, fill, and
bias traces.

## Alternatives considered

- **Delete the average as dead code.** Defensible, and it was the cheaper
  option. Rejected because the reactive-only controller is measurably worse, and
  because the design that was written down is the better design — the honest
  fix was to finish it rather than to amend the design to match the code.
- **Scale the bias continuously with the complexity ratio.** Rejected. A smooth
  response invites chasing noise, and any finer reaction is really a prediction
  of the next frame's cost, which single-pass control cannot make honestly. The
  three-band rule reacts to a difficulty change without pretending to forecast
  one.
- **Let the bias reach across a whole band.** Rejected. At a third of capacity
  the complexity term would override fullness rather than inform it, which is a
  different controller from the one the design describes. A test asserts the
  bias stays well under a third.
- **Add a second pass.** Out of scope for version one, and named as a limitation
  and an expansion track rather than smuggled in behind a bug fix.

## Supersedes / superseded by

Extends [ADR-0010](0010-rate-control-tables.md) and
[ADR-0011](0011-rate-bucket-initial-fill.md), neither of which is reversed: the
tables, the initial fill, and the band structure are unchanged.

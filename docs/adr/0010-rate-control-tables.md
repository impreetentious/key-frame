# ADR-0010: Rate-control tables

## Context

Version one names a Q16.16 leaky bucket, a complexity EWMA, a ±2 QP step, and
a first-frame QP table, but the committed quantizer asset only stored the
bucket dimensions. Two encoders could pick different initial QPs or different
third-boundary steps and still emit legal bitstreams, breaking encoder
determinism.

## Decision

Record the missing encoder-normative arithmetic in `spec/v1/quant.toml`:
`first_qp[i] = max(0, 63-2*i)` for `i = min(31, floor(log2(budget_bits)))`,
fill updates that saturate into `[0, capacity]`, and QP steps from fill
thirds with a maximum step of two. Five worked rows pin the third boundaries
and the 0/63 clamps. Bitstream version remains 1. Frame QP is already a
packet field; this is encoder policy, not a syntax change.

## Consequences

The encoder ABR path and its tests replay the literal rows. Threshold edits
fail the asset derivation. Constant-QP encoding is unchanged.

## Alternatives considered

- Leave first-frame QP as an implicit mid-range constant: rejected because
  extreme bitrates would start far from the bucket's steady QP.
- Scale the per-frame budget by the complexity EWMA in version one: deferred;
  the EWMA is tracked with saturating arithmetic so a later policy can use it
  without widening the bitstream.

## Supersedes / superseded by

Extends the encoder-determinism consequences of
[ADR-0001](0001-foundation-decisions.md) and the worked-vector approach of
[ADR-0002](0002-worked-vector-augmentation.md).

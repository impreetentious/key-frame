# ADR-0004: Intra oracle stream

## Context

P3 needs the independent reference decoder to consume a complete stream without
trusting bytes emitted by the production syntax writer. The freeze fixed every bin and
context involved in a 64×64 DC keyframe with zero residual, but did not combine
those decisions into a full `.kfv` artifact.

## Decision

The standard-library oracle now emits `intra64_dc_all_zero`: a complete 64×64
sequence plus key packet whose payload contains the ten frozen context-coded
bins for an unsplit DC block and six all-zero transform blocks. The vector also
pins the decoded planar YUV SHA-256. Bitstream version remains 1 because the
vector composes existing syntax without changing it.

## Consequences

Both Rust decoders must consume bytes authored outside their writer path and
produce the same neutral frame. Header, packet, range, context, partition,
prediction, transform-presence, and frame-storage boundaries are exercised by
one end-to-end artifact.

## Alternatives considered

- Generate the first stream with `kfenc`: rejected because the encoder does not
  yet exist and would not be independent.
- Hand-enter bytes without an emitting oracle: rejected because later review
  could not reproduce the context adaptation and checksums.

## Supersedes / superseded by

Extends the verification consequences of [ADR-0001](0001-foundation-decisions.md).

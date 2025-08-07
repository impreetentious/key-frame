# ADR-0008: Deblock filter vectors

## Context

The specification freeze recorded deblock strengths, decision rows, and the
alpha/beta/tc threshold arrays, but not the weak four-tap and strong six-tap
arithmetic or worked sample rows. A production filter would otherwise have to
invent those operations, making the implementation the source of truth.

## Decision

Record the missing decoder-normative arithmetic in `spec/v1/deblock.toml`:
vertical-then-horizontal passes, eight-pixel minimum edges, activity tests
against alpha/beta, weak four-tap delta clamped by tc, strong six-tap
smoothing, chroma using the weak filter at every nonzero strength, and five
worked sample rows. Bitstream version remains 1. This is a conformance
augmentation of already-frozen version-one reconstruction, not a syntax change.

## Consequences

Both decoder implementations and the encoder closed loop must replay the
literal rows. Threshold or formula edits fail the asset derivation, Rust
replay, and generated-document hash. Installing the filtered frame into
reference slots remains mandatory.

## Alternatives considered

- Leave formulas in prose only: rejected because two independent decoders
  would not have a byte-level expected output.
- Bump bitstream version: rejected because no syntax element changed; the
  asset only writes down the reconstruction rule version one already named.

## Supersedes / superseded by

Extends the verification consequences of [ADR-0001](0001-foundation-decisions.md)
and the worked-vector approach of [ADR-0002](0002-worked-vector-augmentation.md).

# ADR-0002: F0 worked-vector augmentation

## Context

F0 froze transform matrices, stage widths, shifts, clamps, and the independent
range/CRC vectors. The first inverse-transform implementation exposed a
verification gap: the numeric semantics were frozen, but their required
impulse, DC, and extreme worked examples had not been materialized as a
machine-readable asset.

## Decision

Add `spec/v1/transform-vectors.toml` with 24 exact input/output cases derived
from the already-frozen matrices and stage rules. This is a conformance
augmentation, not a semantic change: bitstream version remains 1. The asset is
embedded by `kf-spec`, checked by the independent derivation script, replayed
by `kf-transform`, and included in generated-document inventory hashes.

## Consequences

G2 can prove the implementation against literal expected outputs instead of
testing only self-consistency. Future changes to a matrix, shift, or clamp now
fail in the asset derivation, Rust replay, and generated-document drift gates.
Any change to those semantics still requires a bitstream-version decision.

## Alternatives considered

- Treat implementation unit tests as the vectors: rejected because code would
  become the sole source of expected behavior.
- Bump bitstream version: rejected because no decoded value or syntax changed;
  the new asset only records existing version-one results.

## Supersedes / superseded by

Extends the F0 verification consequences of [ADR-0001](0001-foundation-decisions.md).

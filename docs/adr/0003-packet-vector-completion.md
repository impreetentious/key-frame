# ADR-0003: Packet vector completion

## Context

The frozen B.2 layout and CRC coverage were already literal in `fields.toml`,
but the frozen oracle output contained only the sequence-header checksum
example. The packet layer needs an independently authored complete packet to
catch disagreement over the header CRC span, flag byte, payload checksum, and
little-endian placement.

## Decision

Extend the standard-library oracle and `vectors.json` with a five-byte keyframe
packet at index zero and QP 32. The vector uses the existing frozen field order
and CRC32C algorithm. Bitstream version remains 1 because no accepted bytes or
decoded semantics change.

## Consequences

The production packet writer and reader must reproduce and consume one full
independent B.2 packet without using their own output as the expectation. Any
future CRC-span or flag-layout change fails the oracle, Rust vector, generated
document hash, and bitstream gate.

## Alternatives considered

- Keep only packet round trips: rejected because a reader and writer can share
  the same wrong layout.
- Bump the bitstream version: rejected because this records the already-frozen
  layout instead of changing it.

## Supersedes / superseded by

Extends the verification consequences of [ADR-0001](0001-foundation-decisions.md).

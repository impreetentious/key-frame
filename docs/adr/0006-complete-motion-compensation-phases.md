# ADR-0006: Complete the version-one motion-compensation phases

- Status: accepted
- Date: 2025-06-28
## Context

The frozen motion-compensation asset fixed quarter-luma-pixel motion, the half-pel
six-tap filter, stage order, and rounding, but omitted the literal quarter-phase
sequences promised by Appendix C.3. It also did not say how a luma motion vector
maps onto a subsampled chroma plane. Inter prediction had not yet been
implemented and no P-frame conformance stream existed, so preserving that
omission would make independently written decoders guess at pixel values.

## Decision

Keep motion vectors in quarter-luma-pixel units for every plane. At luma half
phase, apply the literal six-tap filter. Derive quarter and three-quarter phases
by 1:1 integer blending between the adjacent integer and half positions, adding
one before floor division by two. Preserve the current stage scale while
blending. For 4:2:0 chroma, the same vector addresses eighth-chroma-pixel
positions; derive the intervening eighth phases with 3:1, 1:1, and 1:3 integer
blends, adding two before floor division by four where applicable.

Horizontal filtering and its phase blend occur before vertical filtering. The
final rounding remains `(value + 512) >> 10` for the separable path and
`(value + 16) >> 5` when only one filtered axis is present. The literal phase
sequences and independent vectors are normative v1 assets.

This completes a missing frozen table without changing any previously decodable
inter stream: none existed. The bitstream version therefore remains one.

## Consequences

Luma and chroma represent the same physical displacement, negative motion uses
Euclidean phase decomposition, and implementations cannot substitute a
platform or library interpolation convention. Any later filter or phase change
requires a new bitstream version and regenerated conformance streams.

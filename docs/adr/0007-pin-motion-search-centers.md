# ADR 0007: Pin canonical motion-search centers

- Status: accepted
- Date: 2025-06-28
## Context

The frozen search asset specified the diamond and subpel visit orders but did
not define how a fractional MVP becomes the initial full-pel center or which
resolution each of the two subpel rounds visits. These choices do not affect
decoder compatibility, but leaving them implicit would let canonical encoders
produce different valid streams from identical inputs.

## Decision

Evaluate zero MV first. Round each MVP component to the nearest full-pel
position, with exact half ties away from zero, and evaluate that center second.
Run the literal diamond ring until a complete ring leaves the best candidate
unchanged. Then run one eight-neighbor half-pel ring (step two in q4 units) and
one eight-neighbor quarter-pel ring (step one), in the committed order. Use
luma SAD during search, retain the earlier candidate on equal SAD, and clamp
every candidate through the normative interpolation-support bounds.

## Consequences

Scalar encoder implementations now agree on candidate order and resolution.
The final reference/mode choice still uses the complete RD cost and the frozen
LAST-before-GOLDEN tie rule. No bitstream-version change is required because
the decoded syntax and reconstruction are unchanged.

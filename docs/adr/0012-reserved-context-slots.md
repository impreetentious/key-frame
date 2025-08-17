# ADR-0012: Reserved Context Slots

## Context

`spec/v1/contexts.toml` froze 144 context ids and, for each group, the
conditioning that selects among them: decoded split-neighbor state for the
partition tree, a clamped count of available neighbors for the skip and inter
flags, a neighbor class for the first intra-mode bin, and the median-predictor
reference for reference selection.

Version one codes none of that conditioning. Its selection functions read only
depth, plane class, transform size, coordinate-prefix position, and the running
count of nonzero levels. Measuring the conformance suite instead of asserting
its coverage made the gap countable: exactly 28 of the 144 ids cannot be
selected by any version-one stream, of any origin.

The 28 fall into four causes.

- Nine partition ids: the neighbor slots at every depth, plus the depth-three
  slot, because a minimum-size block codes no split decision at all.
- Eleven prediction ids: the neighbor slots for the skip and inter flags, the
  two unused first-bin intra classes, the four unused mode-tree bins beyond the
  three the eight-mode tree needs, and the golden-predictor reference slot.
- One coefficient-presence id: luma transforms are the block side capped at
  thirty-two, so luma never reaches the 4×4 transform. Only chroma does.
- Six last-position ids: each transform size codes only as many contexted
  prefix bins as its coordinate needs, and the fifth bin of a 32×32 coordinate
  is bypass-coded rather than contexted.

Something had to give, because three claims could not all hold: the bank is
frozen, the suite covers every id, and coverage is measured rather than
asserted.

## Decision

Hold the 28 ids in reserve rather than renumbering the bank or widening the
coder to reach them.

`conformance/syntax-coverage.toml` states each group's reachable and reserved
ids and why the reserved ones are unreachable. The coverage gate decodes every
committed vector in both decoders and checks the inventory in both directions:
the suite must code every reachable id, and no vector of any origin may code a
reserved one. Bitstream version remains 1.

Coverage is now reported as two separate numbers — 116 of 116 reachable context
ids and 14 of 14 syntax elements — never as a single percentage of 144. A
percentage of the whole bank would round the reserved slots away.

## Consequences

An id cannot be moved between the two halves to make a gate pass: moving a
reachable id into reserve fails immediately, because a vector still codes it.

Reaching a reserved id later means implementing the conditioning the asset
already describes. That changes the coded bits and so requires a new bitstream
version, not an edit to this inventory.

The reserved slots stay in the bank, initialized and adapted like any other,
costing 56 bytes of state that no stream reads. That is the price of a frozen
numbering, and it is smaller than the cost of a renumbering that would
invalidate every stream produced so far.

`docs/LIMITATIONS.md` carries the plain-language version of this for readers who
never open the inventory.

## Alternatives considered

- Renumber the bank to the 116 live ids: rejected. The numbering is frozen and
  drives both decoders and the generated normative document; renumbering would
  invalidate every existing stream to save a rounding error in a report.
- Implement the declared conditioning now: rejected as a scope change. It is a
  compression improvement wearing a coverage costume, and it would break the
  reconstruction freeze.
- Keep asserting 100% of 144 in the inventory: rejected. It was never true, and
  the only reason it survived this long is that nothing measured it.

## Supersedes / superseded by

Extends [ADR-0009](0009-reconstruction-freeze.md).

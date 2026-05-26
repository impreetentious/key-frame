# Cutting room

A catalogue of real decoder and encoder defects that produce a stream you can
replay. Entries are not invented to fill the shelf.

Each finding is a `NNN-slug.md` file with:

- id, crate, found-by, regression stream path, fixed-in
- Symptom, Hunt, Root cause, Fix, Lesson

Regression streams live under `conformance/crashes/` and are decoded by the
ordinary test suite after the fix lands.
`scripts/collect-cutting-room.mjs` refuses to build the projection room if an
entry names a stream that is not there, or if that directory holds a stream no
entry names.

## What this shelf does not hold

That requirement is the shelf's scope, not a measure of how many defects this
codec has had. A finding earns an entry here when it produces a file: a
sequence of bytes that decoded wrongly before the fix and decodes correctly
after it. Campaigns, differential runs, and conformance produce findings of
that shape, which is why every entry so far came from one of them.

The other kind is a governance defect — a declared value that governed nothing,
a rule the specification stated and no code read, a limit two implementations
enforced by agreeing with each other rather than with the declaration. Those
are real defects, several of them decoder-normative, and none of them produces
a crashing stream. There is nothing to pin, so there is nothing to catalogue
under a schema built around a pinned stream, and inventing one would break the
rule at the top of this page.

They are written up instead in [`docs/claims.md`](../claims.md), where each one
appears beside the check that now closes it: the deblock skip rules that had no
vector, the motion-vector bound carried as a literal, the transform stage
shifts, the interpolation constants, the minimum edge length spelled as the
English word "eight", and the declared values the normative document restated
instead of reading. `scripts/ci/declared-scalar-use.sh` is the gate that class
produced.

## Entries

- [000-false-sync-prefix](000-false-sync-prefix.md) — false `KFP1` between the
  sequence header and the first packet.

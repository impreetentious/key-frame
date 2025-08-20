# ADR-0014: Probe Crate Extraction

## Context

The syntax probe — the parse layer plus the shadow canonical replay that
reconciles modeled entropy against emitted bytes — lived inside `kf-tools`
alongside the Y4M reader and the command-line binaries. That was the right home
while its only consumer was `kfprobe`.

The projection room changed that. Every overlay it draws reads the probe's JSON:
the partition grid, the intra glyphs, the motion vectors, the entropy heatmap,
the per-block panel. So the WebAssembly surface needs the probe, and it cannot
reach `kf-tools`: that crate owns filesystem access, floating-point metrics, and
the binaries, none of which belong in a browser module.

Three ways out, and the choice matters more than it looks.

## Decision

Extract the probe into its own crate, `kf-probe`, depending on `kf-bitstream`
and `kf-range` and nothing else. `kf-tools` re-exports it so the binaries keep
one import surface, and `kf-wasm` depends on it directly.

The crate boundary states what the probe is: a reader of streams, not a decoder
of pictures. It never reconstructs a pixel, so it needs neither decoder, and
nothing that depends on it inherits one.

## Consequences

Both callers get the same report from the same code. The equality gate diffs the
WebAssembly module's JSON against the native tool's for every committed vector,
so the numbers an overlay draws are checked as tightly as the pixels beneath
them — a heatmap keyed off a subtly different figure would be a wrong picture
rather than a wrong pixel, which is harder to notice and no less wrong.

The crate graph gains a node. The reference decoder's isolation is untouched:
`kf-probe` sits on the production side, and the dependency scan still refuses
anything beyond `kf-frame` and `kf-spec` inside `kf-ref`.

## Alternatives considered

- Fold the probe into `kf-dec`: rejected. Reporting would then sit on the
  critical path of every consumer that only wants pixels, and the fast decoder's
  API is a bit-exactness contract, not a place to grow a JSON writer.
- Duplicate the probe inside `kf-wasm`: rejected outright. Two copies of a
  reconciliation that must agree to the byte is the exact failure the two-decoder
  rule exists to prevent — and unlike the decoders, these two would not be
  independently derived, only copied.
- Let `kf-wasm` depend on `kf-tools`: rejected. It would drag the filesystem,
  the metrics floats, and the binaries into a browser module.

## Supersedes / superseded by

Resolves the open question left in [ADR-0013](0013-raw-webassembly-boundary.md).

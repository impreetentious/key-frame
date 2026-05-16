# ADR-0015: Conformant Encoder Ablations

## Context

The repository publishes rate–distortion curves, and the curves it considers
worth publishing are ablations: the same clip encoded with one encoder tool
turned off, so a reader can see what that tool is actually worth. The headline
comparison is deliberately this codec against itself, because a readable scalar
codec measured against a mature standard encoder produces a number that says
nothing anyone did not already know.

That leaves a question with teeth: what does "turned off" mean?

Two answers are available and only one of them is honest.

An ablation can narrow what the **encoder** is willing to choose. The search
stops offering a candidate; the bitstream still means exactly what it meant, and
the resulting file is an ordinary `.kfv` that both decoders reconstruct
bit-exactly. Or an ablation can change what the **format** does — skip a
reconstruction stage, reinterpret a field — in which case the file it produces
is not a Key Frame stream at all, and the curve drawn from it describes a
program nobody can run.

The distinction is not academic here. The most commonly cited codec ablation is
the loop filter, and Key Frame's loop filter is unconditional: there is no
frame-header flag that disables deblocking, because a decoder permitted to skip
it would reconstruct a different picture from identical bytes.

## Decision

Ablations narrow the encoder's candidate set and nothing else. A `Toolset`
carries five switches — golden reference, skip, inter prediction, sub-pixel
refinement, and quadtree splitting — each of which removes candidates from the
rate–distortion search while leaving every normative rule intact. The full
toolset is the default, and it is what every gate, every conformance stream, and
every headline figure uses.

The loop filter is **not** ablatable in version one, and the charts say so
rather than omitting the row. Measuring it would require shipping a decoder that
disagrees with the specification.

Every named toolset is required, by test, to produce a stream that decodes
identically in `kf-dec`, `kf-ref`, and the encoder's own closed loop, and to
change the bytes relative to the full toolset. The second requirement matters as
much as the first: a switch that is read but never acted on produces a flat
ablation curve, which reads as "this tool is worth nothing" rather than as the
dead branch it is.

## Consequences

Every published curve is a fact about a coding decision rather than about a
fork. Any point on any of them can be handed to either decoder, or to the
projection room in a browser, and reproduced.

The set of measurable ablations is bounded by what the bitstream permits an
encoder to choose. That is a real limitation and it is named in
[`docs/LIMITATIONS.md`](../LIMITATIONS.md) rather than worked around. Lifting it
for the loop filter would mean a syntax change, a bitstream-version bump, and
its own decision record — which is the expansion-track path, not something to
smuggle in behind a benchmark.

The encoder gains a configuration parameter that production never varies. It is
threaded explicitly rather than held in a global, so a reader can see at every
call site which search is being run, and the constructors install the full
toolset so that forgetting to configure one cannot silently ablate a gate.

## Alternatives considered

- **Build the loop filter ablation anyway, behind a feature flag.** Rejected.
  The streams would not decode correctly in either shipped decoder, so the
  curve would describe a build that does not exist. A benchmark that cannot be
  reproduced by the artifact under test is not evidence.
- **Add a frame-header flag disabling deblocking.** Rejected for version one.
  It is a syntax change to a frozen bitstream, and it would let a conformant
  encoder ship streams that look worse for no reason. If it is ever wanted, it
  is an expansion track with its own record.
- **Ablate by post-processing the reconstruction.** Rejected. The closed loop is
  the load-bearing rule of the encoder: an ablation that changed the
  reconstruction without changing what the decoder does would break
  encoder/decoder agreement, which is the one invariant the whole repository
  exists to protect.
- **Let ablations vary GOP structure too** (for example, all-intra as a
  keyframe-interval change rather than a search restriction). Rejected. It would
  compare two different GOP structures and call the difference a tool. The
  `no-inter` ablation keeps P frames as P frames, with the same headers and the
  same context carry, and only changes which candidates the search considers.

## Supersedes / superseded by

None.

# ADR-0001: Foundation decisions

## Context

Key Frame ships a bitstream, and a bitstream is a promise: anyone who
implements the documented contract must get the same pixels back. That promise
survives only if the arithmetic is integer, the normative text is generated
from frozen data rather than from the encoder that happens to exist, and a
second decoder written independently agrees bit for bit. The following
decisions close the highest-risk architectural choices before implementation
begins.

## Decisions

1. **Apache-2.0 license** — a codec and its bitstream want maximum reuse;
   nothing here needs copyleft.
2. **Original bitstream only** — Key Frame implements no proprietary or
   standardized format and will never gain compatibility with one. The IP
   posture ships verbatim in the README, and the project is never marketed as a
   replacement for a standardized codec.
3. **Integer-only codec core** — no `f32` or `f64` in frame storage, bit io,
   range coding, transforms, prediction, syntax, or either decoder. Floats
   exist only in the tools' metrics and the inspector's interface, where they
   cannot reach a decoded pixel.
4. **Specified intermediate precision** — every arithmetic stage names its
   container width and rounding rule, with explicit casts at the named points,
   because implicit widening is what makes an optimized path diverge from the
   naive one.
5. **A deterministic encoder** — the same input and settings produce a
   byte-identical stream on every platform; search orders are fixed and any
   pseudo-randomness comes from a from-scratch seeded generator with test
   vectors. Version one uses none.
6. **The specification is inert data** — field widths, tables, scan orders,
   context identifiers and initial values, and worked coder examples are frozen
   as literal assets; the normative document is generated from them and diffed.
   Code is never the sole source of normative truth.
7. **An independent oracle in another language** — a standard-library-only
   script consuming the same inert assets emits the hand-worked header,
   checksum, and coder vectors. It imports no output of this workspace, so a
   generator and its consumer cannot share a wrong formula and look correct.
8. **Two decoders with a hard boundary** — the reference decoder depends only
   on the inert frame storage types, the specification assets, and the standard
   library. It owns its byte reader, checksum, arithmetic, and state machine,
   and the dependency graph is checked in continuous integration.
9. **Frame storage shares nothing but storage** — the shared frame crate
   exposes planes and indexing, never codec arithmetic, so the one permitted
   executable dependency between the decoders cannot carry a coding decision.
10. **8-bit 4:2:0 in version one** — one pixel format, fully specified and fully
    tested, ahead of a matrix of half-supported ones.
11. **A quadtree over 64-pixel superblocks** — a single partition mechanism down
    to the smallest block size, rather than a family of special cases.
12. **An explicit prediction tree on inter frames** — skip, inter, and intra are
    distinct syntax branches, and skip selects a reference while carrying
    neither a motion-vector difference nor residual. Implicit modes are how a
    syntax layer becomes unreadable.
13. **Two references, one motion vector, no bi-prediction** — a last and a
    golden reference are enough to demonstrate reference management and its
    failure modes without doubling the search and the proof surface.
14. **Six-tap quarter-pel interpolation, horizontal then vertical** — the order
    is normative because reversing it changes the result.
15. **Transform sizes four through thirty-two with fixed large-block tiling** —
    the largest coding block uses a fixed tiling rather than a transform-tree
    syntax, keeping the coefficient layer describable.
16. **A per-transform coefficient-presence flag with a stated coefficient cap
    and wide accumulation** — the extreme-input behavior of every stage is
    documented and tested rather than discovered by a corrupt stream.
17. **Flat quantization with one frame-level quantizer** — no delta-quantizer
    syntax in version one, so rate control has exactly one lever and its
    behavior is legible.
18. **A byte-oriented adaptive binary range coder** — with fixed-probability
    bypass, a stated adaptation rate, an opaque delayed finalization tail,
    transactional context carry across valid inter frames, and a literal reset
    at keyframes. The finalization tail is the classic place a coder is subtly
    wrong, so it is specified by worked vector rather than by description.
19. **Sync-framed packets with a literal checksum contract** — flags are
    authoritative, an index gap invalidates references and contexts, and
    recovery happens at the next valid keyframe and nowhere else. No damaged-
    reference concealment: silently decoding past corruption is a worse failure
    than refusing to.
20. **Rate–distortion decisions use modeled entropy, with emission cost
    measured separately** — the encoder's optimization signal and the actual
    byte cost are distinct quantities, reconciled by replaying a canonical
    shadow encode with a stated conservation invariant. Attributing bytes to
    individual symbols would be a fiction.
21. **Pre-freeze hashes are staging, never conformance** — a hash produced
    before the bitstream freeze is labelled as such, so an early phase cannot
    quietly become a compatibility promise.
22. **Fixed metric definitions** — the quality metrics and the rate–distortion
    aggregation method are pinned in advance, because choosing them after
    seeing results is how honest numbers become dishonest ones.
23. **A canonical encoder configuration for published numbers** — one frame
    pattern with exact scene-cut and reference-refresh counters, so a published
    curve is reproducible by command.
24. **Fixed-point rate control** — the rate controller is integer like
    everything else; a float controller would make the encoder's output
    platform-dependent through the back door.
25. **No competitive claims, ever** — the honest gap to a disclosed orientation
    baseline is the content, and the word "competitive" does not appear in this
    project's documentation.
26. **Scalar and single-threaded in version one** — vectorization and threading
    are later tracks gated behind bit-exact equality with the scalar path, not
    defaults.
27. **Multi-origin conformance** — vectors come from more than one source and
    include streams neither decoder produced, so the suite tests the contract
    rather than the implementation.
28. **From-scratch dependencies** — codec crates use the standard library only;
    the entropy coder, the transforms, the Y4M reader, and the metrics are the
    product. Serialization is allowed for tool output, `proptest` in tests, and
    the WebAssembly bindings are sanctioned in advance rather than discovered
    later.
29. **No facade** — no tool or interface presents canned data as engine output,
    and a fixture is labelled a fixture inside the artifact itself.
30. **Owner-gated commits, tags, and releases** — implementation work leaves an
    auditable change queue; repository state and release timing remain the
    owner's decision, and build coordination artifacts stay outside the
    repository so it stands alone.

## Consequences

The codec has a narrow dependency surface, a specification that exists
independently of its implementation, and a second decoder whose only purpose is
to disagree. Several choices cost real efficiency: integer-only fixed point is
more work than float, two decoders are twice the maintenance, and refusing to
conceal a damaged reference produces visibly worse output than a forgiving
decoder would. Those costs buy the project's only real claim — that a stream
decodes to the same pixels everywhere, and that the reason is written down.

Any reversal requires a new numbered record. Anything touching the bitstream
also requires a version decision, regenerated oracle vectors, and review of both
decoder implementations.

## Alternatives considered

Compatibility with an existing standard, floating-point signal math,
hand-written normative documentation, a single decoder verified against itself,
a shared helper library between the two decoders, generated normative tables,
and early vectorization were each rejected. Every one of them would either
break bit-exactness across platforms or turn the verification story into a
common-mode failure.

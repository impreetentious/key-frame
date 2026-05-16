# A codec you can read

A video codec makes a decision for every block of every frame: how to predict
it, what to do with whatever the prediction missed, and how many bits that
choice is worth. A two-hour film at thirty frames a second is somewhere north of
two hundred million of those decisions.

The compressed file records none of the reasoning. It records the outcome — this
block is intra, mode DC, here are its coefficients — and nothing about why that
outcome beat the alternatives. The reasoning existed for a few microseconds
inside an encoder and was discarded.

That is the correct engineering choice. Nobody wants to ship the encoder's
deliberations in every file. But it means that if you want to understand why a
particular block of a particular video looks the way it does, the file cannot
tell you, and the encoder that made the decision is a production codebase where
the answer is spread across a million lines written for speed by people who
already knew the answer.

So you read the specification instead. Standards documents are precise and they
are complete and they are almost perfectly opaque to someone learning, because
they describe the decoder as a machine to be implemented rather than a set of
ideas to be understood. They are reference material for people who already have
the shape in their heads.

Key Frame exists because there is a third option nobody takes: build a codec
small enough to hold in your head, and then refuse to make it fast.

## Legibility as a constraint, not a documentation task

The easy version of this project is a normal codec with unusually good comments.
That version fails, and it fails in a way worth naming, because the failure is
not obvious until you are a long way in.

Comments describe code. They do not constrain it. A codebase that is optimised
for speed and commented for clarity is still optimised for speed: the control
flow is still shaped by the profiler, the abstractions are still whatever made
the inner loop tighter, and the comments are a translation layer between what
the code says and what it means. Translation layers rot. Six months later the
loop has been restructured and the comment describes a program that no longer
exists.

The version that works is to make legibility a constraint the code has to
satisfy, checked the way any other constraint is checked. In this repository
that means:

**The signal path is integer-only, and a lint enforces it.** Not "we avoid
floats" — `f32` and `f64` are denied types in every codec crate, and a
repository-wide scan fails the build on the word. This started as a
bit-exactness rule and turned out to be a legibility rule too: integer code has
one meaning, and a reader never has to wonder whether a discrepancy is a bug or
a rounding difference.

**The specification is data, not prose.** The transform matrices, the scan
orders, the 144 context definitions, the quantizer tables, the deblocking
decision thresholds — all of it lives as literal values in `spec/v1/`, in a
crate that is forbidden from containing executable code. The normative document
is generated from those values. The tests are generated from those values. The
implementation reads those values. There is no second copy that can drift.

**Every claim links to the gate that enforces it.** The README says the two
decoders agree bit-exactly. There is a gate that decodes every committed stream
in both and compares every sample. The README says conformance coverage is
complete; there is a counter, instrumented in both decoders, that reports 14 of
14 syntax elements and 116 of 116 reachable contexts, and the build fails if it
is not.

None of that is documentation. It is the same rigour a fast codec spends on
performance, spent on comprehensibility instead.

## The reference decoder is the interesting part

There are two decoders in this repository. `kf-dec` is the one you would ship.
`kf-ref` is deliberately naive, depends on almost nothing, and is written
independently against the frozen specification — its own byte reader, its own
checksum, its own arithmetic, its own state machine. A dependency scan fails the
build if it ever imports anything from the production side.

This is expensive. Every feature is implemented twice, by hand, without sharing
the helper that would make the second one easy. It is also the single most
valuable thing in the project, for a reason that took a while to become obvious.

A specification and one implementation are indistinguishable from an
implementation and a description of it. You cannot tell, from the inside,
whether the document says what the code does or whether the code says what the
document does. The moment there is a second implementation written from the
document alone, the document has to actually be right — every ambiguity becomes
a disagreement, and every disagreement is a real defect in the specification
that would otherwise have shipped as folklore.

The catalogue of bugs this has caught is in the
[cutting room](../cutting-room/). The first entry is a good one: the
resynchronization rule after a corrupt packet. The fast decoder advanced one
byte per failed header candidate. The reference decoder treated any header
failure as the end of the stream. Both behaviours are defensible; only one is
what the specification says. Without the second implementation there would have
been no disagreement, and the specification's rule would have quietly meant
whatever the one decoder happened to do.

## What this costs

It is slow. Scalar, single-threaded, no SIMD, and the rate–distortion search
evaluates every candidate through the real reconstruction path rather than an
approximation of it. Encoding a QCIF clip takes seconds where a production
encoder takes milliseconds.

It compresses worse than mature codecs, by a lot, and
[says so plainly](../LIMITATIONS.md). No B-frames, one motion vector per block,
flat quantization, single-pass rate control. Each of those is a simplification
chosen so the decision stays followable.

Those are not apologies. They are the trade. The project would be worse at what
it is for if it were faster, because every one of the techniques that would make
it fast works by making the relationship between the code and the idea less
direct.

## Who this is for

Someone who has read that a codec uses "an adaptive binary arithmetic coder" and
wants to know what one actually looks like — not the paper, the hundred lines.
Someone who wants to see a motion vector chosen and then see the exact bits it
cost. Someone who suspects that the parts of compression that sound like magic
are mostly bookkeeping, and would like that suspicion confirmed in detail.

The [projection room](../../inspector/) is the shortest path in: it decodes real
streams in the browser and answers a click on any block with the syntax that
coded it. Nothing on that page is a pre-decoded picture. That constraint is
enforced by a build gate too — the site refuses to ship if a single frame of
decoded video is found in it.

Everything else is the same idea applied repeatedly: make the claim checkable,
then check it.

# Freezing a bitstream

There is a moment in a format project where you have to stop changing the
bitstream. Not "we should be careful now" — actually stop, in a way that has
teeth, because from that moment on every file anyone has produced has to keep
decoding.

The question is what "stop" means mechanically. "We agreed not to change it" is
not a mechanism. Neither is a version number nobody checks.

## What a frozen format has to survive

The freeze has to hold against four things, and only the first is obvious.

**Deliberate change.** Someone wants a new syntax element. This one is easy —
you say no, or you bump the version.

**Accidental change.** Someone reorders two writes in the syntax layer while
refactoring. The encoder and decoder still agree, every round-trip test passes,
and every file produced before the change is now undecodable. Nothing in a
round-trip test can see this, because both sides moved together.

**Drift between the document and the code.** The specification says the
quantizer step for QP 40 is one value; the implementation uses another. Both are
self-consistent. A third party implementing from the document produces streams
this decoder rejects, and the disagreement is discovered by the third party,
which is the worst possible discoverer.

**Drift between the document and itself.** The prose says the header is
twenty-four bytes; the field table adds up to twenty-six. Nobody notices because
nobody adds up field tables.

## Data first, code second

The approach here inverts the usual order. Normally an implementation exists and
a document describes it. Here the values came first, as data, and everything
else is derived from them.

`spec/v1/` holds literal TOML: the transform matrices, every scan order, all 144
context definitions with their initial probabilities, the quantizer scale table,
the deblocking decision thresholds, the field offsets and widths of every header,
the frozen search order. `kf-spec` is a crate that embeds those files and is
forbidden — by a gate that greps for `fn` and `impl` — from containing a single
executable function. It is data with a compiler in front of it.

From that:

- `docs/bitstream.md` is **generated**. Not written and checked; generated, by
  `spec/generate_docs.py`, and a gate regenerates it and fails on any diff. The
  document cannot disagree with the tables because it is made of them.
- The implementation **reads** those tables rather than holding its own copies.
  There is no second array of transform coefficients to fall out of step.
- The test vectors are **authored** from those tables by `spec/oracle.py`, a
  Python implementation that shares no code with the Rust and imports nothing
  outside the standard library.

The result is that "the specification" and "the implementation" are not two
artifacts that have to be kept in agreement. There is one artifact, and two
things that read it.

## The check I did not expect to need

Having the tables be authoritative is not enough on its own, because a table can
be internally wrong. `spec/check_assets.py` verifies derivations: that the
inverse transform matrix is actually the transpose of the forward one, that the
scan orders are permutations, that the context count matches the number of
contexts, that the quantizer table is monotone.

Then there is `spec/mutation_check.py`, which is the one that earned its keep.
It takes the frozen assets, mutates exactly one entry, and requires the checks
to reject it. Six mutations, six required rejections.

The first time it ran, two of them passed. Which is to say: two declared scalars
in the assets — `transforms.toml`'s `dc_scale` and `contexts.toml`'s `count` —
could be changed to any value at all and nothing noticed, because no derivation
compared them against anything. They were documentation pretending to be data.
`check_assets.py` now cross-checks both.

That is a good bug to have found, and it is only findable by a check whose
purpose is to attack the checks. A test suite that only ever runs on correct
input tells you nothing about what it would catch.

## The freeze itself

Freezing was a specific set of things becoming true at once, each with a gate
behind it:

- Every normative placeholder resolved. A scan greps the spec and the generated
  document for `TODO`, `TBD`, `placeholder`, "to be decided", "exact table", and
  "per the table" — that last pair because prose that says "per the table"
  without saying which table is a placeholder with better manners.
- Both decoders implementing every reconstruction stage, agreeing bit-exactly.
- The conformance suite drawn from three independent origins — vectors authored
  by the Python oracle, vectors written by hand, and vectors produced by the
  encoder — because a suite made only of encoder output tests the encoder's
  habits rather than the format.
- Every staging artifact **deleted**. Not marked provisional; removed, along with
  the generator that made them. A pre-freeze vector left in the tree is a
  vector someone will eventually treat as normative.
- The bitstream version pinned at 1.

The version pin is a separate contract from the repository version, and the
version-coherence script says so in a comment: a release can happen without a
syntax change, and a syntax change must be a deliberate decision rather than a
side effect of bumping a patch number.

## What it costs afterwards

Everything is harder now, which is the point.

The ablation study wanted to measure what the loop filter is worth. It cannot,
because deblocking in version one is unconditional — there is no header flag,
because a decoder allowed to skip it would reconstruct differently from the same
bytes. Adding one would be a syntax change, a version bump, an architectural
decision record, regenerated vectors, and a review of both decoders.

So the study does not measure it, and the
[limitations page](../LIMITATIONS.md) says why. That is what a freeze with teeth
feels like from the inside: an experiment you wanted to run, that you do not
run, because running it would mean the format was never really frozen.

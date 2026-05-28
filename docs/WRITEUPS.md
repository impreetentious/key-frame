# Key Frame — Writeups

Six pieces about building Key Frame, written while building it. They are not
documentation — [`docs/bitstream.md`](bitstream.md) is the contract and
The [decision log](CODEC-SPEC.md#13-decision-log) is the record of decisions. These are the arguments
behind those decisions, written for someone who wants to know why a codec is
shaped the way it is.

Read them in order or don't; each stands alone.

1. [A codec you can read](#1-a-codec-you-can-read) — why legibility is a
   design constraint and not a documentation task.
2. [The hundred lines that terrify me](#2-the-hundred-lines-that-terrify-me)
   — the range coder, and what it means to write code with no margin.
3. [Drift: the bug that eats codecs](#3-drift-the-bug-that-eats-codecs) — the closed loop, and the
   failure mode that has no symptom until it has every symptom.
4. [Freezing a bitstream](#4-freezing-a-bitstream) — what it takes to make a
   format a contract instead of a habit.
5. [Fuzzing my own decoder](#5-fuzzing-my-own-decoder) — four campaigns, one
   real bug, and why the bug was in the decoder I trusted more.
6. [Honest rate–distortion](#6-honest-ratedistortion) — how to publish
   compression numbers without lying, including to yourself.


## 1. A codec you can read

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

### Legibility as a constraint, not a documentation task

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

### The reference decoder is the interesting part

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
[cutting room](cutting-room/). The first entry is a good one: the
resynchronization rule after a corrupt packet. The fast decoder advanced one
byte per failed header candidate. The reference decoder treated any header
failure as the end of the stream. Both behaviours are defensible; only one is
what the specification says. Without the second implementation there would have
been no disagreement, and the specification's rule would have quietly meant
whatever the one decoder happened to do.

### What this costs

It is slow. Scalar, single-threaded, no SIMD, and the rate–distortion search
evaluates every candidate through the real reconstruction path rather than an
approximation of it. Encoding a QCIF clip takes seconds where a production
encoder takes milliseconds.

It compresses worse than mature codecs, by a lot, and
[says so plainly](CODEC-SPEC.md). No B-frames, one motion vector per block,
flat quantization, single-pass rate control. Each of those is a simplification
chosen so the decision stays followable.

Those are not apologies. They are the trade. The project would be worse at what
it is for if it were faster, because every one of the techniques that would make
it fast works by making the relationship between the code and the idea less
direct.

### Who this is for

Someone who has read that a codec uses "an adaptive binary arithmetic coder" and
wants to know what one actually looks like — not the paper, the hundred lines.
Someone who wants to see a motion vector chosen and then see the exact bits it
cost. Someone who suspects that the parts of compression that sound like magic
are mostly bookkeeping, and would like that suspicion confirmed in detail.

The [projection room](../inspector/) is the shortest path in: it decodes real
streams in the browser and answers a click on any block with the syntax that
coded it. Nothing on that page is a pre-decoded picture. That constraint is
enforced by a build gate too — the site refuses to ship if a single frame of
decoded video is found in it.

Everything else is the same idea applied repeatedly: make the claim checkable,
then check it.

## 2. The hundred lines that terrify me

The range coder in this repository is about a hundred and eighty lines. It is
the smallest load-bearing component in the project and by a wide margin the one
I am most careful around, because it is the only place where being *nearly*
right produces output that looks completely fine and is completely wrong.

Here is the whole encoder, minus bookkeeping:

```rust
fn encode_bin(&mut self, symbol: bool, p1: u16, finalization: bool) {
    let p0 = 4096_u32 - u32::from(p1);
    let bound = (self.range >> 12) * p0;
    if symbol {
        self.low += u64::from(bound);
        self.range -= bound;
    } else {
        self.range = bound;
    }
    while self.range < RANGE_TOP {
        self.range <<= 8;
        self.shift_low(finalization);
    }
}
```

That is arithmetic coding. Everything else in the entropy layer — the 144
contexts, the adaptation rule, the binarizations that turn a motion vector into
a sequence of bins — is scaffolding around those nine lines.

### Why it is different from other code

Most bugs are local. A wrong sign in a transform makes one block look wrong. A
wrong bounds check crashes on one input. You find it, you fix it, the blast
radius was one thing.

An arithmetic coder has no locality at all. Its entire state is an interval,
carried forward, narrowed by every symbol. Every bit that has ever been coded is
still in there. If the encoder and the decoder disagree about the interval by
one — one, at any point in a frame — then every symbol after that point decodes
to garbage. Not degraded: garbage. The partition tree parses as nonsense, the
modes come out as whatever the wrong bits say, and the decoder either produces a
picture of static or wanders into an error.

And the failure is invisible from the encoder's side. The encoder does not fail.
It produces bytes. Those bytes are a perfectly valid-looking file. You find out
when something else tries to read it, which in a normal development loop might
be much later, on a different input, with no obvious connection to the change
that caused it.

### The three things that actually go wrong

**Carry propagation.** `low` can overflow. When it does, the carry has to
propagate backwards into bytes you have already decided on but not yet written —
which is why there is a `cache` byte and a `pending` count, and why the encoder
holds back a run of `0xFF` bytes rather than emitting them. This is the part
everyone gets subtly wrong on the first attempt, and the symptom is that
everything works until the coder happens to produce a run of `0xFF`, which on
typical content might be one frame in fifty.

The code asserts what it believes:

```rust
assert!(carry <= 1, "invariant: range encoder delayed carry is at most one bit");
```

That assertion has never fired. It is there because the day it does fire is the
day I want to be told immediately rather than three stages downstream.

**Probability semantics.** `p1` is the probability of a one, in 1/4096ths. The
bound is computed from `p0`. Swap those, or compute the bound from `p1`, and the
coder still works — it still round-trips, it still produces a file the decoder
reads back correctly. It just compresses worse, by a few percent, forever, with
no error anywhere. This is the failure mode that scares me most, because nothing
detects it except knowing what the number is supposed to mean. There is a trap
test named exactly that: `trap_probability_semantics`.

**Finalization.** The encoder has to flush enough of `low` for the decoder to
finish the last symbol. The rule here is exactly five `shift_low` calls. Four
works for most streams. It fails for the ones where the final interval is
narrow, which is content-dependent and therefore rare and therefore terrifying.

### What you do about it

You do not test a range coder by round-tripping it. Round-tripping proves the
encoder and decoder share an assumption; it does not prove the assumption is the
right one. Both of the failures above round-trip perfectly.

So: **an independent oracle.** `spec/oracle.py` implements the coder a second
time, in Python, with no shared code and no shared author state beyond the
specification. It emits the vectors. The Rust has to match them byte for byte.
That catches probability semantics, because a second implementation written from
the definition uses the definition.

**Exhaustive round-trips where exhaustive is possible.** Every one of the 2^16
binary sequences of length sixteen, at three initial probabilities — one near
zero, one at a half, one near one. Sixty-five thousand sequences is nothing to a
computer and it covers every carry pattern a sixteen-bin stream can produce.

**Literal vectors for the cases that are hard to reach by accident.** The
all-zero stream is six bytes and the probability lands at exactly 949 after
twenty-four zeros; both are asserted. A crafted carry vector. A short-init
vector. A finalization-tail vector. Each one is a specific way to be wrong,
pinned so it cannot come back.

**Accounting that has to balance.** The encoder tracks how many bytes it has
emitted, and `finish` asserts that the count equals the actual output length.
It is a trivial invariant that would catch a whole class of emission bugs.

### The part I did not expect

Building the coder's accounting turned into a small crisis about what a "bit
cost" even is.

The natural question — how many bits did this block cost? — has no answer. The
coder emits a byte when its interval demands one, which may be several symbols
after the block that made the interval narrow. Attributing that byte to a block
is a choice, not a measurement, and every reasonable choice gives a different
number.

So the tools report two quantities and refuse to combine them. **Modeled
entropy** is what the cost model predicted, in Q16 fixed point, summed over the
block's symbols — that is what the encoder's own decisions were priced with, so
it is the honest answer to "what did the encoder think this cost". **Emission
time bytes** is when the coder actually flushed, bucketed by block — that is a
timing fact and explicitly not ownership.

The projection room shows both and makes you pick which one the heatmap is
colouring. The block panel prints both with a note saying neither is the block's
bit count. It would have been easy to add them, or to pick one and call it "the
cost", and the number would have looked authoritative and been meaningless.

That is the real lesson of the hundred lines: the arithmetic is the easy part.
Knowing what the numbers coming out of it are allowed to claim is the hard part.

## 3. Drift: the bug that eats codecs

Here is a bug you can write in one line and not find for a month.

An encoder predicts each block from what came before, codes the difference,
and moves on. The question is: predict from *what*, exactly? From the original
source frame, or from the frame as the decoder will reconstruct it?

The source frame is right there. It is what you are compressing. Using it is the
obvious thing, and it is wrong, and the wrongness is almost invisible.

### Why it looks fine

The decoder does not have the source. It has only what it decoded — which is the
source plus quantization error. If the encoder predicts from the source and the
decoder predicts from its reconstruction, they are predicting from two different
pictures, and their reconstructions diverge.

On a keyframe, they diverge by the quantization error, which is small. On the
next frame, the encoder codes a residual against a prediction the decoder cannot
form, so the decoder's error is now the old error plus the new one. On the next,
plus another. The gap compounds.

For the first few frames, nothing looks wrong. The picture the decoder produces
is slightly softer than the encoder thought it would be. Ten frames in, there is
a visible haze. Thirty frames in, the picture is visibly wrong in a way that
looks like a completely different bug — bad motion compensation, or a broken
transform, or a chroma issue — because by then the error has accumulated into
something structured.

And it will not reproduce on your short test clip. Three frames is not enough
for drift to become visible. You need dozens, on content with real motion, which
is exactly the test you write last.

### The rule

The encoder must predict from its own reconstruction: it must decode what it
just encoded, using the same code path the decoder will, and predict from that.
Everything in the encoder that touches a reference frame reads the
reconstruction, never the source. The source is read exactly twice — once to
compute the residual, once to score a candidate — and never as a prediction
input.

This is called the closed loop and it is the load-bearing rule of any encoder.
It costs real time: the encoder is now running the inverse transform, the
reconstruction, and the deblocking filter inside its own search, for every
candidate it evaluates. In this repository that is the single largest cost in
encoding, and it is not negotiable.

### Making it impossible instead of remembering it

A rule you have to remember is a rule you will eventually break, usually while
optimising something at eleven at night. So the repository does not rely on
remembering.

**The encoder returns its reconstruction, and it is checked against both
decoders.** `EncodedStream` carries `reconstructed_frames`, and the gates assert
that they equal what `kf-dec` produces and what `kf-ref` produces, sample for
sample, on every committed stream and on natural corpus video at five
quantizers. If the encoder ever predicted from something the decoder cannot see,
those three would stop agreeing on the frame after the first one.

**The rate–distortion search evaluates candidates through the real path.** There
is no approximate reconstruction for scoring purposes. A fast approximation is
the natural optimisation here and it is the exact thing that reintroduces drift,
because the approximation is what the encoder would then be predicting from.

**A named trap.** `trap_encdec_mismatch` exists specifically to fail when the
loop opens.

The interesting thing about that list is that none of it is a comment saying
"predict from the reconstruction". The rule is enforced by making its violation
produce a test failure, immediately, on a three-frame clip — instead of a haze
that shows up thirty frames into a clip nobody runs during development.

### The variant that is harder

There is a second kind of drift and it is worse, because the closed loop does
not catch it.

The encoder and decoder can also share state that is not pixels: the adaptive
probability contexts. Every symbol coded updates a probability, and the decoder
performs the same updates as it reads. If they ever fall out of step — one
extra symbol coded, one context updated in a different order, one context not
reset on a keyframe — the arithmetic decoder is now using different
probabilities than the encoder used, and the entire remainder of the frame
decodes to nonsense.

Unlike pixel drift this fails loudly and immediately. But it can also happen in
a way that does *not* fail: if the encoder speculatively codes a candidate to
price it, and that speculative coding mutates the live context state, then the
encoder's contexts have seen symbols the decoder will never see. The stream is
still decodable — the decoder just gets different probabilities from that point,
which costs compression rather than correctness.

So the mode search takes an immutable snapshot of the context bank, prices each
candidate against a copy, and only commits the winner's updates. There is a trap
for that too — `trap_rdo_context_snapshot` — which checks both that the live
state is untouched by pricing and that the price of a candidate does not depend
on the order candidates were evaluated in. The second half matters more than it
looks: an order-dependent price means the search is scoring candidates against
each other's side effects.

### The general shape

Both of these are the same bug wearing different clothes. An encoder is a
program that has to model, exactly, what a different program will do with its
output. Every piece of state that program will have, the encoder has to have
identically — not approximately, not usually, identically — and every place the
encoder has access to something the decoder does not is a place where that
correspondence can quietly break.

The way you survive it is to stop treating the correspondence as something you
maintain and start treating it as something you test. Decode everything you
encode. Compare every sample. Do it on every gate, not in a benchmark you run
before releases.

## 4. Freezing a bitstream

There is a moment in a format project where you have to stop changing the
bitstream. Not "we should be careful now" — actually stop, in a way that has
teeth, because from that moment on every file anyone has produced has to keep
decoding.

The question is what "stop" means mechanically. "We agreed not to change it" is
not a mechanism. Neither is a version number nobody checks.

### What a frozen format has to survive

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

### Data first, code second

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

### The check I did not expect to need

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

### The freeze itself

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

### What it costs afterwards

Everything is harder now, which is the point.

The ablation study wanted to measure what the loop filter is worth. It cannot,
because deblocking in version one is unconditional — there is no header flag,
because a decoder allowed to skip it would reconstruct differently from the same
bytes. Adding one would be a syntax change, a version bump, an architectural
decision record, regenerated vectors, and a review of both decoders.

So the study does not measure it, and the
[limitations page](CODEC-SPEC.md) says why. That is what a freeze with teeth
feels like from the inside: an experiment you wanted to run, that you do not
run, because running it would mean the format was never really frozen.

## 5. Fuzzing my own decoder

A decoder is a program that reads bytes it did not write. That is the entire
security story, and it is why a decoder is one of the few kinds of program where
"it works on valid input" is close to worthless as a claim.

Key Frame's decoders have been through eighty million mutated inputs: twenty
million against each of four campaigns, which is what `spec/v1/constants.toml`
declares and `crates/kf-fuzz/tests/campaign.rs` holds the harness to. This is
what that found, and — more usefully — what it did not.

A note on "a night", because this page used to open with it. The budget above
belongs to a nightly workflow, and no hosted pipeline has ever run it — the
repository's remote does not run the pipeline it ships, which
[`docs/CODEC-SPEC.md`](CODEC-SPEC.md) states plainly and
[decision 0018](CODEC-SPEC.md#13-decision-log) records. The run behind this
page was executed by hand on a development machine at the full declared budget,
from the same fixed seeds the workflow would use, and took a little over ten
hours. Preflight runs the same four campaigns at a smaller declared count on
every change. The sentence that stood here for months described a schedule
rather than a result, and it understated its own subject fourfold.

### Four campaigns, because random bytes are not enough

The naive fuzzer generates random bytes and feeds them in. It is worth having,
and it finds almost nothing after the first hour, for a reason that is obvious
once you see it: a random buffer fails the sync word check in the first four
bytes. It never reaches the interesting code. You are testing your `if` statement
at extraordinary expense.

So there are four targets, and three of them start from a valid stream:

**Arbitrary bytes** — random buffers of one to 384 bytes. The cheap one. It
tests that nothing before validation can be made to panic.

**Structured mutation** — take a real committed stream and damage it in one of
five specific ways: flip a bit, truncate at a random point, insert a false sync
word, lie about a payload length, or delete a run of bytes from the middle. Each
of these is a thing that actually happens to files, and each one gets the
decoder deep into its state machine before anything goes wrong.

**Header fuzz** — take a real stream and corrupt only the header fields, which
is where every dimension, count, and length that later code will trust comes
from. A width of four billion is not a random byte pattern; it is a specific
attack on every allocation downstream.

**Frame deletion** — remove whole packets from a valid inter-coded stream, so
the decoder sees frame 0, frame 1, frame 4. This exercises the dependency
tracking: frames 2 and 3 are gone, so frame 4 predicts from a reference that
does not exist.

The generator is seeded and deterministic — `Xoshiro256PlusPlus` with an
explicit seed per target — so a campaign that finds something can be replayed
exactly rather than approximately.

### The one real bug

Eighty million iterations, zero panics, zero hangs, and no case where the two
decoders disagreed about a stream either of them accepted. One genuine finding,
and it is the interesting kind: it was not a crash.

The structured-mutation campaign inserted eight garbage bytes between the
sequence header and the first packet, and the bytes happened to begin `KFP1` —
the packet sync word. The fast decoder accepted the stream. The reference
decoder rejected it.

Neither decoder crashed. Neither produced a wrong picture. They simply disagreed
about whether the file was a file, which means the *specification* was
ambiguous and one of the two implementations had guessed.

The rule, as written, is that a failed sync or header candidate advances one
byte. The fast decoder implemented that. The reference decoder had been written
to parse packets at a running offset and treat any header failure as a stream
error — a completely defensible reading, and the wrong one.

The fix was to give the reference decoder its own cursor with the real rule:
failed headers skip one byte, a payload-CRC failure still consumes the declared
extent, and only a fully validated packet is reconstructed. It is entry
[000](cutting-room/000-false-sync-prefix.md) in the cutting room, with the
66-byte stream that produced it checked in as a regression.

The lesson in that entry is the one I would keep: *resynchronization has to be
written twice, independently, or a shared skip becomes a shared blind spot.*

### Why differential fuzzing is the whole point

A single-implementation fuzzer can only find crashes, hangs, and assertion
failures. Those are worth finding. But the bugs that actually matter in a format
project are disagreements, and a disagreement needs two things to disagree.

Running both decoders on every mutated input turns the fuzzer from a crash
detector into a specification checker. Every input where they differ is either a
bug in one of them or an ambiguity in the document — and it is worth noticing
that those are the same finding wearing different hats. If the document had been
unambiguous, the reference decoder's author (me, a month earlier, with no memory
of the other implementation) would have implemented the same rule.

### What fuzzing did not find, and what does

Fuzzing is bad at the failures that are not about malformed input. The error
matrix exists for those: thirteen named traps drawn from the corruption section
of the specification, each asserted in both decoders and compared frame for
frame. Truncation, header CRC failure, payload CRC failure with reference
invalidation, dependency loss until a keyframe, a non-key first frame,
corruption before the first shown frame, recovery at a keyframe, index gaps,
leading loss, hidden frames, the golden-refresh requirement on keyframes,
zero-size packets, and oversize payload lengths — plus a sweep of single-byte
payload wounds across every packet, as a backstop against a poisoned reference
that a named case would miss.

Building that matrix turned up something bigger than any fuzz finding: the
recovery state machine described in the specification *did not exist*. Both
decoders aborted the entire stream on the first fault, so most of those named
cases had nothing to test against. The strict all-or-nothing path was the
only path there was.

Fuzzing had not found it because fuzzing was checking that damage was rejected,
and damage *was* rejected — just far more aggressively than the format says. The
decoders were wrong in a direction that looks like caution. That is a shape of
bug a fuzzer structurally cannot see, because it produces no crash and no
disagreement between two implementations that share the same missing feature.

Writing that state machine, twice, with separate status vocabularies rather than
a shared enum, was where the real work went. And it produced its own surprise:
two of the trap expectations were wrong on the first run and the code was right.
A keyframe arriving contiguously after a dependency-lost frame is an ordinary
shown frame, not a recovery point — recovery only applies when the gap lands on
*that* packet. I had written the tests from a half-memory of the rule; the
implementation had been written from the rule.

### The honest summary

Eighty million iterations found one bug. That sounds like a poor return, and by
the usual metric it is.

But the campaign is not really a bug-finding exercise any more; it is a
regression harness with a very large input space. Its value is not the bug it
found in November, it is that the same bug cannot come back, and neither can any
of the other thousands of shapes a full campaign explores while nobody watches.
The seeds are fixed, so every run of it explores the same eighty million; that
is what makes it a regression harness rather than a search.

The bugs that matter were found by writing things twice and requiring agreement.
Fuzzing is how you keep them found.

## 6. Honest rate–distortion

Compression benchmarks are the most quietly dishonest numbers in this field, and
almost none of the dishonesty is deliberate. It comes from the fact that there
are a dozen defensible choices in any comparison, each of them moves the answer
by a few percent, and the person making them is the person who wants a
particular answer.

This is what it took to publish numbers I would defend.

### PSNR is not one number

"PSNR" names a family. Do you compute it per frame and average, or from summed
squared error over the whole clip? Luma only, or a weighted combination with
chroma? Do you crop to the display dimensions or include the padding the codec
added to reach a superblock boundary? What do you report when the error is zero?

Every one of those changes the answer, and the first one changes it a lot. Per
frame averaging weights an easy frame the same as a hard one, and — worse — a
single lossless frame in a clip makes the average infinite. Summed squared error
over the whole clip is the honest form, and it is what this repository uses.

Zero error reports as `lossless: true` with a `null` figure, never a large finite
number. A reader who sees a decibel figure is entitled to assume error was
measured, not that it was absent.

SSIM is worse, because there are more knobs: window size, sigma, whether the
weights are normalized, population or sample moments, what happens at the edges,
whether you evaluate at every sample or only where the window fits inside the
frame. That last one matters more than it sounds — cropping the border excludes
exactly the region a codec finds hardest, which flatters every result.

So the definitions are pinned to the last constant: the Wang eleven-tap Gaussian
form, sigma 1.5, literal normalized weights, reflected edges, population
moments, a value at every displayed sample, `L = 255`, `K1 = 0.01`,
`K2 = 0.03`. And the rule that makes the pin mean something: *anything computed
differently must call itself something else.*

### Two implementations, again

The same argument that produced two decoders produces two metric
implementations.

`bench/metric_oracle.py` computes SSIM by direct 2D convolution with the full
eleven-by-eleven outer-product window. The Rust applies the same window as two
separable passes. Separability is an algebraic identity, so they must agree — and
if the Rust ever crosses its row and column passes, or drops a tap at an edge,
the identity breaks and the vectors catch it.

The Gaussian weights are frozen as literals in the Rust, so its numbers do not
depend on which `exp` the platform ships. That is only safe if the literals
actually *are* the derivation, so the oracle derives them from `exp` and the
vector set asserts the two match.

The vectors that matter are the awkward ones. A plane narrower than the window,
where every window overhangs an edge and the reflection rule decides the whole
answer. A three-by-three plane, where the window is nearly four times the plane
and a coordinate has to fold back and forth several times — an implementation
that mirrors once and clamps gives a different number there and nowhere else. A
single row, where the vertical direction collapses to one line and a crossed
pair of separable passes cannot hide.

### BD-rate is a recipe, not a quantity

Bjøntegaard delta rate is the standard way to say "this is X% better", and two
implementations of it routinely differ by more than the effect being measured.

The choices: what you interpolate (log rate against quality, here), with what
(monotone PCHIP), over what interval (only the quality range the two curves
share), integrated how (analytically, in closed form).

Monotone matters. An unconstrained cubic spline through rate–quality points
overshoots between them, and an overshoot is a claim that some quality is
cheaper than any measurement said it was. PCHIP cannot overshoot, so the curve
stays inside the evidence.

The interval matters more. Integrating outside the shared quality range is
extrapolation into a region nobody measured. So the implementation *refuses*:
fewer than four distinct finite points, or no overlap, produces a named error
rather than a number. A refusal is information. A number computed over a region
that was never sampled is not.

The Python oracle integrates each cubic piece with two-point Gauss–Legendre,
which is exact for cubics; the Rust evaluates the Hermite antiderivative in
closed form. Same integral, no shared arithmetic. They check each other on
committed vectors, slope for slope and area for area, on every run.

There was briefly a third implementation, in TypeScript, because the projection
room draws these curves in a browser and it seemed natural for the page to
compute what it drew. It agreed with the Rust to the second decimal the first
time it ran, which was the most reassuring moment in building the page and, on
reflection, the wrong lesson to take from it. Two implementations exist in order
to disagree usefully: something compares them, and a disagreement fails the
build. A third that nothing compares against is not a third check. It is a
second answer to the same question, sitting where a reader would assume the
first one had been verified, with no way to notice the day the two part.

So it is gone. The campaign computes each bitrate difference, writes it into the
receipt beside the points it came from, and `rd_verify` recomputes it on every
run; the page states what the receipt says. The browser smoke suite fetches that
receipt and matches every rendered cell against it, which is a weaker-sounding
check than a third implementation and a much stronger one — it can actually
fail.

### The comparison this project will not make

The obvious next move is to benchmark against x264 and report the gap.

The repository does not do it, and the words "competitive", "beats", and
"rivals" are banned from it permanently. Not because the answer would be
embarrassing — the answer is that a deliberately readable scalar codec with no
B-frames loses badly to decades of tuning, which everyone already knows — but
because the comparison would be *meaningless in a way that looks meaningful*.
Publishing a number implies the two things were doing the same job. They are not.

What is published instead is this codec against itself: the same clip encoded
with one tool turned off. Those ablation curves are the pedagogically valuable
product, because they answer questions a reader actually has. What is a second
reference frame worth? What does quarter-pixel motion buy over integer? What
does the quadtree cost and what does it save?

Some of the answers are surprising, which is how you know the measurement is
doing something. On these two clips, the golden reference is worth
approximately nothing — a few hundredths of a percent, in both directions
depending on the clip. That is a real result about this encoder's GOP policy on
this content, and it is published as measured rather than quietly dropped for
being unflattering.

### Receipts, or it did not happen

Every figure carries the means to check it.

Each quality report names its inputs by content hash — not by filename, because
two different clips can share a path — and `kfmetric repro <report.json>`
recomputes the entire thing from those inputs and exits non-zero if any figure
has moved. It does not print the command that would regenerate the number; it
regenerates the number.

The campaign receipt records, for every point, the coded size, the stream's
hash, and both metrics. `rd_verify` re-encodes each point and compares every
field. `scripts/ci/rd-gate.sh` runs it on every change for the ends of each
quality ladder, because a gate slow enough to skip is a gate that gets skipped.
All sixty points are the nightly job's, which is a workflow this repository has
never had a runner for — `docs/CODEC-SPEC.md` says so under continuous
integration. That is a gap in scheduling rather than in the check: running
`rd_verify --receipts bench/results/rd-campaign.json` yourself re-derives all
sixty points and the six average-bitrate targets, and every figure on this page
came back exact when it was last run that way.

The whole thing is enforced by a rule that sounds obvious and is not: **quality
is measured against the decoded stream, never against the encoder's own
reconstruction.** Those two are required to be identical, and the campaign
checks that they are. But measuring the encoder's copy would make a drift bug
invisible in exactly the numbers a reader would use to judge the codec — the one
place you least want a blind spot.

### What honesty actually costs

Roughly: an extra implementation of every metric, a vector set aimed at the
cases where implementations diverge, a refusal to answer questions the data does
not support, a published limitations page, and one comparison not made.

It also costs an ablation. The loop filter cannot be turned off in this format,
so its contribution is not on the charts — and rather than measure it with a
decoder that disagrees with the specification, the charts say plainly that this
one is unavailable and why.

That last one is the test of whether any of this is real. It is easy to be
rigorous about numbers that flatter you.

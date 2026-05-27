# Fuzzing my own decoder

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
[`docs/LIMITATIONS.md`](../LIMITATIONS.md) states plainly and
[ADR-0018](../adr/0018-preflight-is-the-arbiter.md) records. The run behind this
page was executed by hand on a development machine at the full declared budget,
from the same fixed seeds the workflow would use, and took a little over ten
hours. Preflight runs the same four campaigns at a smaller declared count on
every change. The sentence that stood here for months described a schedule
rather than a result, and it understated its own subject fourfold.

## Four campaigns, because random bytes are not enough

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

## The one real bug

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
[000](../cutting-room/000-false-sync-prefix.md) in the cutting room, with the
66-byte stream that produced it checked in as a regression.

The lesson in that entry is the one I would keep: *resynchronization has to be
written twice, independently, or a shared skip becomes a shared blind spot.*

## Why differential fuzzing is the whole point

A single-implementation fuzzer can only find crashes, hangs, and assertion
failures. Those are worth finding. But the bugs that actually matter in a format
project are disagreements, and a disagreement needs two things to disagree.

Running both decoders on every mutated input turns the fuzzer from a crash
detector into a specification checker. Every input where they differ is either a
bug in one of them or an ambiguity in the document — and it is worth noticing
that those are the same finding wearing different hats. If the document had been
unambiguous, the reference decoder's author (me, a month earlier, with no memory
of the other implementation) would have implemented the same rule.

## What fuzzing did not find, and what does

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

## The honest summary

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

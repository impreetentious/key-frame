# The hundred lines that terrify me

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

## Why it is different from other code

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

## The three things that actually go wrong

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

## What you do about it

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

## The part I did not expect

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

# Drift: the bug that eats codecs

Here is a bug you can write in one line and not find for a month.

An encoder predicts each block from what came before, codes the difference,
and moves on. The question is: predict from *what*, exactly? From the original
source frame, or from the frame as the decoder will reconstruct it?

The source frame is right there. It is what you are compressing. Using it is the
obvious thing, and it is wrong, and the wrongness is almost invisible.

## Why it looks fine

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

## The rule

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

## Making it impossible instead of remembering it

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

## The variant that is harder

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

## The general shape

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

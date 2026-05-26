# Honest rate–distortion

Compression benchmarks are the most quietly dishonest numbers in this field, and
almost none of the dishonesty is deliberate. It comes from the fact that there
are a dozen defensible choices in any comparison, each of them moves the answer
by a few percent, and the person making them is the person who wants a
particular answer.

This is what it took to publish numbers I would defend.

## PSNR is not one number

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

## Two implementations, again

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

## BD-rate is a recipe, not a quantity

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

## The comparison this project will not make

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

## Receipts, or it did not happen

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
never had a runner for — `docs/LIMITATIONS.md` says so under continuous
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

## What honesty actually costs

Roughly: an extra implementation of every metric, a vector set aimed at the
cases where implementations diverge, a refusal to answer questions the data does
not support, a published limitations page, and one comparison not made.

It also costs an ablation. The loop filter cannot be turned off in this format,
so its contribution is not on the charts — and rather than measure it with a
decoder that disagrees with the specification, the charts say plainly that this
one is unavailable and why.

That last one is the test of whether any of this is real. It is easy to be
rigorous about numbers that flatter you.

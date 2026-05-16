# Writeups

Six pieces about building Key Frame, written while building it. They are not
documentation — [`docs/bitstream.md`](../bitstream.md) is the contract and
[`docs/adr/`](../adr/) is the record of decisions. These are the arguments
behind those decisions, written for someone who wants to know why a codec is
shaped the way it is.

Read them in order or don't; each stands alone.

1. [A codec you can read](01-a-codec-you-can-read.md) — why legibility is a
   design constraint and not a documentation task.
2. [The hundred lines that terrify me](02-the-hundred-lines-that-terrify-me.md)
   — the range coder, and what it means to write code with no margin.
3. [Drift: the bug that eats codecs](03-drift.md) — the closed loop, and the
   failure mode that has no symptom until it has every symptom.
4. [Freezing a bitstream](04-freezing-a-bitstream.md) — what it takes to make a
   format a contract instead of a habit.
5. [Fuzzing my own decoder](05-fuzzing-my-own-decoder.md) — four campaigns, one
   real bug, and why the bug was in the decoder I trusted more.
6. [Honest rate–distortion](06-honest-rate-distortion.md) — how to publish
   compression numbers without lying, including to yourself.

# ADR-0017: Checked Arithmetic in Release Builds

## Context

Nearly every check in this repository runs in a debug profile, where Rust stops
on integer overflow. The exceptions are the ones that matter most.

The decoder campaigns, the natural-corpus bit-exactness proof, the average
bitrate measurements, the rate–distortion receipts, and the WebAssembly module
are all built `--release`, because they are the slow ones. Cargo's default for
that profile is `overflow-checks = false`. So the runs that push the most novel
data through the codec — the nightly job alone drives twenty million mutated
streams through both decoders — were the runs where an arithmetic overflow
wrapped quietly instead of stopping.

That inverts the intent. A campaign exists to find the input that breaks a
decoder. An overflow in the signal path is exactly such a break, and it is the
kind with no symptom: the addition wraps, a sample comes out wrong, the frame
decodes, and the campaign reports success. The independently written reference
decoder would probably disagree somewhere downstream and surface it as a
mismatch — but as an unexplained mismatch on a mutated stream, not as a panic
naming a file and a line.

An overflow anywhere on the path to a decoded pixel is a bit-exactness bug.
Bit-exactness is the whole claim.

## Decision

`[profile.release]` sets `overflow-checks = true` for the workspace.

`kf-fuzz` exposes `overflow_is_checked()`, which overflows a value the optimizer
cannot fold and reports whether the addition stopped. The campaign binary calls
it before its first iteration and refuses to run otherwise, with the reason
named. The setting is therefore proved by the artifact that depends on it rather
than trusted from a manifest line: `cargo test --release` and `cargo run
--release` select different profiles, so a test in some other binary would be
asking the wrong build.

## Consequences

The campaigns, the corpus proof, and the rate work now stop on an overflow with
a location instead of continuing with a wrong number.

Release builds get slower. The measured cost on the rate–distortion campaign is
within the noise of a run, which is the expected shape for code whose inner
loops are bounds-checked array arithmetic already. This is not a performance
project, and the README says so; there is no benchmark here that a few percent
would invalidate.

The WebAssembly module carries the checks too, and therefore the panic paths
that go with them. It grows, well inside the declared size budget, and the
budget was not touched. A host that hits one sees a trap rather than a wrong
picture, which is the correct order of bad outcomes for a decoder.

A future profile that turns the checks off fails the campaign rather than
quietly weakening it.

## Alternatives considered

- **A dedicated checked profile for the campaigns only.** Rejected. It would
  leave the corpus gate, the rate measurements, and the shipped module on
  unchecked arithmetic, and it would mean the campaign no longer tested the
  build anything else runs. The value of a campaign is that it exercises the
  real artifact.
- **Leave release alone and rely on the debug test suite.** Rejected. The debug
  suite runs committed vectors and a 256-iteration smoke campaign. It never sees
  the twenty-million-stream mutation space or a natural clip, which is precisely
  where an unexercised arithmetic edge would live.
- **Also enable `debug-assertions` in release.** Declined for now. The six
  `debug_assert!`s in the workspace state invariants the debug suite already
  exercises on the same code paths, so the addition would buy little and would
  change the shipped module's behaviour on an invariant that cannot be reached.
  Overflow is different in kind: it is data-dependent, and the release runs are
  the only ones that see the data.
- **Replace the arithmetic with explicit `checked_`/`saturating_` calls
  everywhere.** Rejected as a substitute. Where a bound is part of the design
  the code already names it; making every ordinary addition explicit would bury
  the deliberate ones among hundreds of ceremonial ones. The profile catches
  what the design did not anticipate, which is the only category that matters
  here.

## Supersedes / superseded by

None.

# Contributing

Key Frame is a correctness laboratory that happens to compress video. Keep
changes small, deterministic, and readable enough for someone learning
compression to follow.

## Before opening a change

- Run `cargo fmt --all -- --check`.
- Run `cargo clippy --workspace --all-targets -- -D warnings`.
- Run `./scripts/ci/forbidden-grep.sh`.
- Run `cargo test --workspace --all-targets` and `./scripts/preflight.sh`.
- Add a named regression test for every known failure mode the change touches.
- Update the public rustdoc for anything you changed.

## The bit-exactness rules

These are not style preferences; they are what makes a decoded frame mean the
same thing on every machine.

- Integer only. No `f32` or `f64` anywhere in the codec crates. All signal math
  is fixed-point with the bit width and rounding rule named per stage. Floats
  are allowed only in the tools' metrics and reporting and in the inspector's
  interface.
- Every arithmetic stage names its container width and rounding rule, with
  explicit casts at the named points. Implicit widening that differs between a
  naive and an optimized path is exactly how a silent mismatch is born.
- No platform-dependent operations: no `usize` in signal math, no truncating
  cast without a named rule, and wrapping or saturating behavior always
  explicit.
- The encoder is deterministic too. Same input and settings produce a
  byte-identical stream on every platform; all search orders are fixed.
- Codec crates read no ambient time, randomness, environment, filesystem, or
  threads, and depend on no allocator ordering.
- Panics are for broken invariants only, and each one names its invariant.
  Malformed input is always a `Result`.

## The bitstream is a contract

Anything decodable carries a version. The normative document is generated from
the frozen specification assets, never written by hand against the code, and
the implementation is never the sole source of truth. A syntax change bumps the
bitstream version and regenerates the conformance vectors.

The reference decoder exists to disagree with the fast one. It depends only on
the inert frame storage types, the specification assets, and the standard
library — it owns its byte reader, its checksum, its arithmetic, and its state
machine. A shared helper between the two decoders turns a verification story
into a common-mode failure, so the dependency-boundary check is not optional.

When the decoders disagree, write the clarifying specification sentence and its
vector *before* fixing the code. The disagreement means the contract was
ambiguous, and fixing only the code leaves the ambiguity in place.

## Decisions and formats

Numbered decision records in `docs/adr/` are append-only. If a change crosses a
crate boundary, touches the bitstream, or affects bit-exactness, write the
record before the implementation.

Commits, tags, and releases are the repository owner's to make.

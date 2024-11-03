## Summary

<!-- What changed and why? -->

## Verification

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `./scripts/ci/forbidden-grep.sh`
- [ ] `cargo test --workspace --all-targets`
- [ ] `./scripts/preflight.sh`

## Correctness and compatibility

- [ ] Every touched failure mode has a named regression test.
- [ ] No float entered a codec crate; every stage names its width and rounding.
- [ ] The reference decoder gained no dependency beyond frame storage, the
      specification assets, and the standard library.
- [ ] Bit-exactness holds: the double-run and cross-platform comparisons are
      unchanged or extended.
- [ ] A syntax change bumped the bitstream version, regenerated the oracle
      vectors, and was reviewed against both decoders.
- [ ] Any new hash is labelled staging or conformance correctly.
- [ ] Malformed input is a `Result`; every `panic!` names its invariant.

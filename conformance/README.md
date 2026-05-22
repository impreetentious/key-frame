# Conformance

The suite has three labeled origins. None may silently replace another.

- `oracle/` — streams emitted by the standard-library writer in `spec/oracle.py`
- `hand/` — streams constructed at the syntax layer and reviewed as literals
- `encoder/` — streams produced by the canonical encoder at pinned settings

`crashes/` is not an origin and is not generated. It holds the streams that
found real defects, kept so each fix stays fixed; every one is named by an entry
in [`docs/cutting-room/`](../docs/cutting-room/), and
`scripts/collect-cutting-room.mjs` refuses to build the site if a stream there
has no entry or an entry names a stream that is not there.

Decoded hashes include the integer deblocking filter: the filtered image is
what both decoders emit and what the encoder stores as LAST and GOLDEN.
Bitstream version is 1. Regenerate with:

```sh
cargo run --locked -p kf-tools --example generate_conformance
```

CI runs the same generator in `--check` mode on Linux and macOS and requires
both platforms to reproduce the committed stream bytes and decoded raw-YUV
hashes exactly.

`syntax-coverage.toml` maps every frozen context group and every payload
syntax element to at least one named oracle, hand, or encoder vector. The
entropy gate fails if that inventory drifts from `spec/v1/contexts.toml`.

# Conformance

`staging/` contains encoder streams produced before the reconstruction
pipeline is frozen. They are reproducibility receipts, not a stable
third-party interoperability suite. Decoded hashes include the integer
deblocking filter: the filtered image is what both decoders emit and what
the encoder stores as LAST and GOLDEN. Regenerate them with:

```sh
cargo run --locked -p kf-tools --example generate_staging
```

CI runs the same generator in `--check` mode on Linux and macOS and requires
both platforms to reproduce the committed stream bytes and decoded raw-YUV
hashes exactly.

`syntax-coverage.toml` maps every frozen context group and every payload
syntax element to at least one named oracle, hand, or encoder vector. The
entropy gate fails if that inventory drifts from `spec/v1/contexts.toml`.

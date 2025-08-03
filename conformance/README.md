# Conformance

`staging/` contains encoder streams produced before the reconstruction
pipeline is frozen. They are reproducibility receipts, not a stable
third-party interoperability suite. Regenerate them with:

```sh
cargo run --locked -p kf-tools --example generate_staging
```

CI runs the same generator in `--check` mode on Linux and macOS and requires
both platforms to reproduce the committed stream bytes and decoded raw-YUV
hashes exactly.

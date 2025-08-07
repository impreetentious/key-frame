# Key Frame specification assets

`v1/` is the decoder-normative, inert source for Key Frame bitstream version 1.
The files contain literal field layouts, context identifiers and initial
probabilities, transforms, scan orders, coding tables, search order, deblock
filter arithmetic, and worked vectors. They contain no executable codec
implementation.

`oracle.py` is an independent Python-standard-library implementation of CRC32C
and the range-coder vector path. It reads the inert field and constant assets
and emits `v1/vectors.json`. `check_assets.py` recomputes reviewed numeric
derivations and fails when any committed literal differs. `generate_docs.py`
renders [`../docs/bitstream.md`](../docs/bitstream.md) and checks it byte for
byte in preflight.

Run all specification checks with:

```sh
./scripts/ci/spec-check.sh
```

A normative change requires an ADR, an explicit bitstream-version decision,
new oracle vectors, regenerated documentation, and review against both decoder
implementations. An implementation may not generate or overwrite these tables
during a normal build.

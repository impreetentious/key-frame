# Key Frame

Video codecs combine prediction, transforms, quantization, entropy coding, and
stateful reconstruction behind an output that reveals almost none of those
decisions. That makes it difficult to learn how a codec works or verify why a
particular block looks the way it does.

## What it does

Key Frame is an original educational video codec under active construction. At
this version the repository provides the pinned Rust workspace, the first
frozen bitstream field/context/numeric assets, generated decoder-normative
documentation, independent worked vectors, inert frame storage, bit-level I/O,
fixed-point helpers, deterministic generator, and the adaptive context bank
that consumes the frozen Q16 entropy-cost assets. The canonical range encoder
now exposes renormalization and emission-time accounting without claiming
symbol-level byte ownership.
architecture decision record, and one-command verification harness that later
codec phases build on.

- Stores 4:2:0 frames with checked dimensions, strides, indexing, and crop
  behavior, without placing codec arithmetic in the shared frame crate.
- Carries all 144 adaptive binary contexts with normative floor-division
  updates and literal modeled-entropy lookup.
- Keeps signal-path crates free of floating point, ambient time, unordered
  iteration, filesystem access, and threads.
- Enforces the independent-decoder dependency boundary before either decoder
  is implemented.
- Checks version, specification, documentation, and license coherence in the same preflight
  command used by continuous integration.

The finished codec will ship an encoder, two independent decoders, a normative
bitstream specification with an independent oracle, command-line tools, and a
browser projection room driven by the real WebAssembly decoder. Those surfaces
are not represented as available before their end-to-end gates pass.

## What it is not

Key Frame is not an implementation of H.264, HEVC, VP9, AV1, or another
standardized format. Its bitstream is original and intentionally small enough
to explain completely. Version one does not include B-frames, 10-bit video,
SIMD, threads, containers, network streaming, or GPU acceleration.

## Limitations

The build constitution and core storage/utilities exist at this version; no
stream can be encoded or decoded yet. Linux x86_64 and macOS arm64 are the
native targets, WebAssembly will be added with the projection room, and Windows
is unsupported.

## Stack

Rust 1.87.0 · edition 2024 · Bash · Node.js for repository checks

## Project docs

- [`docs/bitstream.md`](docs/bitstream.md) is the generated decoder-normative v1 contract.
- [`docs/adr/`](docs/adr/) records architectural decisions and their tradeoffs.
- [`spec/`](spec/) contains inert assets, derivation checks, and the independent oracle.

## Run locally

Install Rust 1.87.0 (the repository pin selects it automatically) and a current
Node.js LTS release, then run:

```sh
./scripts/preflight.sh
```

## Verify

The same command runs every required local and CI gate:

```sh
./scripts/preflight.sh
```

## Build and deploy

The workspace currently builds as Rust libraries with `cargo build
--workspace`. The static inspector deployment arrives only after live native
and WebAssembly decodes are bit-identical.

## Status and contributing

Key Frame is pre-1.0 while its bitstream and conformance suite are being built.
The numbered records in [`docs/adr/`](docs/adr/) are append-only. Every change
must add tests for touched failure modes and leave preflight green. A syntax
change also bumps the bitstream version and regenerates independent vectors.

## License

[MIT](LICENSE) © 2024-2025 Sidakpreet Singh

---

**Version:** v0.3.2

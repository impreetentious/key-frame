# Key Frame

A video codec decides, for every block of every frame, how to predict it, how to
transform whatever the prediction missed, and how many bits that decision costs.
The compressed file records none of that reasoning. Anyone asking why a
particular block looks the way it does is left reading a production codebase
where the answer is spread across a million lines written for speed.

## What it does

Key Frame is an original video codec written to be read. It compresses 8-bit
4:2:0 video into a small, completely specified bitstream, then decodes that
bitstream twice — once with a fast production decoder and once with a
deliberately naive reference decoder written independently against the
specification. Every gate in the repository exists to prove the two agree.

- Encodes and decodes progressive 8-bit 4:2:0 `C420jpeg` Y4M at even dimensions
  from 64×64 to 4096×2304, padding internally to the superblock grid and
  cropping on output.
- Partitions each 64×64 superblock by quadtree down to 8×8, choosing splits and
  modes by closed-loop rate–distortion search over a frozen candidate order.
- Predicts key frames from eight intra modes, and predicts P-frames from LAST
  and GOLDEN references with quarter-pixel six-tap motion compensation, median
  motion-vector prediction, and reference-selecting skip.
- Codes the residual with literal 4/8/16/32 integer transforms and flat
  quantization across the full 64-step QP range, then deblocks the reconstructed
  frame before it becomes a reference.
- Entropy-codes every symbol through an adaptive binary range coder over a
  closed set of 144 contexts, with modeled-entropy costs the encoder reuses for
  its own decisions.
- States conformance coverage as a measurement rather than a claim: both
  decoders are instrumented, and the committed streams are required to code
  every context slot and every syntax element the format can reach, with at
  least one stream per element that the encoder did not author.
- Frames the stream in sync-marked packets carrying independent header and
  payload CRC32C, resynchronizes byte-by-byte after damage, and installs a
  decoded frame into the reference state only on complete success.
- Ships `kfenc`, `kfdec`, and `kfprobe`: encode, decode, and turn any stream
  into schema-validated per-block JSON with byte accounting that reconciles
  against a canonical replay of the payload.
- Derives its normative document, its numeric tables, and its test vectors from
  the same literal frozen assets, so the specification and the implementation
  cannot drift apart silently.
- Keeps the entire signal path integer-only: no floating point, no unordered
  iteration, no ambient clocks, no filesystem, no threads.
- Verifies itself with one command that runs every gate continuous integration
  runs, in the same order.

## What it is not

Key Frame is not an implementation of H.264, HEVC, VP9, AV1, or any other
standardized format, and it does not read or write their files. Its bitstream is
original and deliberately small enough to explain in full. It is not a
performance project: version one has no B-frames, no 10-bit support, no SIMD, no
threading, no container format, no network streaming, and no GPU path.

## Limitations

The bitstream is original, so nothing else decodes a `.kfv` file. Compression
efficiency is not competitive with production codecs and is never presented as
if it were; every published rate–distortion number is self-measured on a named
corpus with the encoder build that produced it. The encoder is scalar and
single-threaded, and it optimizes for a decision you can follow rather than for
speed; its average-bitrate mode is a single-pass leaky bucket that holds a
target across a clip, not a per-frame guarantee. Damage handling is limited to
detection, dependency invalidation, and resumption at the next valid keyframe: a
corrupt frame is never concealed, never partially applied, and never allowed into
later prediction, but nothing is reconstructed from it either, and the frames
between the damage and the next keyframe are reported as lost rather than
estimated. Twenty-eight of the 144 context slots are held in reserve: the frozen
context bank was sized for neighbor conditioning that version one declares but
does not use, so no stream can reach those slots, and coverage is reported
against the 116 the format can actually code rather than as a percentage of the
whole bank. Linux x86_64 and macOS arm64 are the supported native targets;
Windows is not supported.

## Stack

Rust 1.87.0 · edition 2024 · Bash · Node.js for repository checks

## Project docs

- [`docs/bitstream.md`](docs/bitstream.md) is the decoder-normative v1 contract,
  generated from the frozen specification assets.
- [`docs/adr/`](docs/adr/) records every architectural decision, the alternatives
  weighed, and the consequences accepted.
- [`spec/`](spec/) holds the literal assets, their derivation checks, and an
  independent standard-library oracle that authors test vectors without the
  Rust implementation.
- [`conformance/`](conformance/) holds committed streams with their decoded
  hashes, reproduced on both native targets in continuous integration.
- [`corpus/`](corpus/) pins the source clips used for measurement and for the
  bit-exactness proof on natural video; the clips themselves are fetched by
  checksum, never redistributed.

## Run locally

Install Rust 1.87.0 — the repository pin selects it automatically — and a
current Node.js LTS release. Encode, decode, and inspect an 8-bit 4:2:0 input:

```sh
cargo run --release -p kf-tools --bin kfenc -- \
  --input input.y4m --qp 32 --output output.kfv
# or: --bitrate 400000 in place of --qp
cargo run --release -p kf-tools --bin kfdec -- \
  output.kfv --output decoded.y4m
cargo run --release -p kf-tools --bin kfprobe -- output.kfv
```

## Verify

One command runs every gate, in the order continuous integration runs them:

```sh
./scripts/preflight.sh
```

It checks version, documentation, license, and specification coherence, then the
range-coder, transform, bitstream, syntax, reference-decoder, probe, intra,
conformance, inter, entropy, deblock, natural-corpus, rate-control,
decoder-campaign, error-matrix, and conformance-coverage gates, then formatting,
lints, the forbidden-API and decoder-boundary scan, the full test suite, and the
documentation build.

## Build and deploy

The workspace builds as Rust libraries and command-line binaries with `cargo
build --workspace`. Continuous integration additionally reproduces every
committed conformance stream and decoded hash on both supported native targets
and requires them to be byte-identical.

## Status and contributing

Key Frame is pre-1.0 and single-maintainer. The bitstream version is a separate
contract from the repository version: a syntax change requires an architectural
decision record, a bitstream-version bump, regenerated independent vectors, and
review of both decoder implementations. Records in [`docs/adr/`](docs/adr/) are
append-only. Every change adds tests for the failure modes it touches and leaves
preflight green.

## License

[MIT](LICENSE) © 2024-2025 Sidakpreet Singh

---

**Version:** v0.11.9

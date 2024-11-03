# Key Frame

Key Frame is a complete video codec built from scratch — encoder, decoder, and a
browser projection room where every bit in the stream explains itself. The codec
is the excuse; the fact that you can read it, check it, and watch it decide is
the point.

Video codecs are the densest artifacts in applied computing: signal processing,
information theory, perceptual modeling, and unforgiving systems discipline in
one bitstream. They are also almost always experienced as black boxes. This one
is built to be opened.

The core is integer-only and bit-exact — no float touches a path that reaches a
decoded pixel — and the bitstream is normative data before it is code: literal
tables and worked vectors live in `spec/`, `docs/bitstream.md` is generated from
them, and a standard-library-only Python oracle emits the header, checksum, and
coder vectors that both Rust decoders must satisfy. There are two decoders on
purpose. `kf-ref` is slow and obvious, depends only on inert frame storage, the
spec assets, and `std`, and owns its own reader, checksum, arithmetic, and state
machine; `kf-dec` is the fast one. They must agree bit for bit, always, and a
shared helper between them would turn the verification story into a common-mode
failure. What the suite proves, and what it does not, is spelled out in
[limitations](docs/LIMITATIONS.md).

![The projection room scrubbing a clip with the motion-vector overlay on, then
jumping to the worst block of the frame and showing the syntax elements that
coded it.](inspector/public/key-frame.gif)

The animation above is generated, not staged: `./scripts/record-gif.sh` drives
the real projection room with Playwright and encodes the captured frames.

**[Open the projection room →](https://itsmonarch04.github.io/key-frame/)** The
same decoder runs in your browser on a real `.kfv` stream: scrub, toggle the
partition, mode, motion-vector, entropy, byte-cost and residual overlays, click
any block to see its mode, quantizer, and the exact syntax elements behind it.
No toolchain required.

## Try the current workspace

```sh
cargo run --release -p kf-tools --bin kfenc -- --input corpus/foreman_cif.y4m --qp 32 --output out.kfv
cargo run --release -p kf-tools --bin kfdec -- out.kfv --output dec.y4m
cargo run --release -p kf-tools --bin kfmetric -- --ref corpus/foreman_cif.y4m --dis dec.y4m
cargo run --release -p kf-tools --bin kfprobe -- out.kfv --frame 30
./scripts/preflight.sh
./scripts/demo.sh
```

The demo closes by printing `sha256(dec.y4m)` and comparing it against the
checked-in conformance hash. Decode is bit-exact or the script fails; there is
no third outcome.

The repository contains:

- `kfenc` and `kfdec`, an original 8-bit 4:2:0 toolset with intra and inter
  prediction, integer transforms, adaptive binary range coding, deblocking, and
  fixed-point rate control;
- `kfprobe`, which runs the parse layer alone and prints a frame's partition
  tree with modeled entropy and emission-time byte accounting kept visibly
  separate, and `kfmetric`, which computes PSNR and SSIM without shelling out to
  anyone else's binary;
- a byte-oriented adaptive binary range coder with fixed-probability bypass,
  transactional context carry across valid inter frames, and an opaque delayed
  finalization tail specified by worked vector rather than by description;
- integer transforms from 4 to 32 with every stage's container width and
  rounding rule named, and extreme-input traps that drive worst-case values
  through each of them;
- a syntax layer over sync-framed packets with a literal checksum contract,
  authoritative flags, and recovery only at the next valid keyframe — a corrupt
  packet invalidates both references and the context state rather than being
  concealed;
- `kf-ref`, the independent reference decoder, and the dependency-boundary gate
  that keeps it independent;
- `spec/`, the inert normative assets plus the standard-library-only oracle, and
  `docs/bitstream.md` generated from them — complete enough that a third party
  could write a decoder from the document and the vectors alone;
- `conformance/`, multi-origin vectors with expected hashes and a coverage
  manifest proving every syntax element is exercised, and the crash corpus in
  which every stream the fuzzer ever broke the decoder with lives permanently;
- the projection room, the cutting-room catalogue of real bugs, and
  rate–distortion receipts on a fixed public corpus.

**This is not a standard codec.** Key Frame neither encodes nor decodes H.264,
HEVC, VP9, or AV1, and never will; its bitstream is its own. No B-frames, no
10-bit or HDR, no 4:4:4 or 4:2:2, no alpha, no screen-content tools, no film
grain. No SIMD and no threads in v1 — scalar, single-threaded, correct first.
Supported targets are Linux x86_64, macOS arm64, and WebAssembly; Windows is not
a supported CI target. The word "competitive" does not appear in this project's
documentation: the honest gap to a disclosed orientation baseline is the
content. See [limitations](docs/LIMITATIONS.md) before interpreting any number.

## IP posture

Key Frame is an original educational codec: its bitstream is its own, and it
implements no proprietary or standardized format. The techniques it uses — block
transforms, motion compensation, arithmetic coding, deblocking — are the
decades-old published foundations of the field. The project makes no patent
claims of its own and provides no warranty or legal opinion of any kind; it
exists to be read and learned from.

## Going deeper

The write-ups are the point of the project — each one names a failure mode and
shows the machinery that catches it.

- [`docs/LIMITATIONS.md`](docs/LIMITATIONS.md) — what this does and does not
  prove; read it before interpreting any number
- [`docs/writeups/`](docs/writeups/) — the six essays:
  [a codec you can read](docs/writeups/01-a-codec-you-can-read.md),
  [the hundred lines that terrify me](docs/writeups/02-range-coder.md),
  [drift: the bug that eats codecs](docs/writeups/03-drift.md),
  [freezing a bitstream](docs/writeups/04-freezing-a-bitstream.md),
  [fuzzing my own decoder](docs/writeups/05-fuzzing.md), and
  [honest rate–distortion](docs/writeups/06-honest-rd.md)
- [`docs/bitstream.md`](docs/bitstream.md) — the decoder-normative specification,
  generated from the inert assets, with complete pseudocode
- [`docs/cutting-room/`](docs/cutting-room/) — real bugs, minimized, with the
  stream that exposed them
- [`docs/adr/`](docs/adr/) — the numbered decisions, append-only
- [`spec/`](spec/) — the literal tables and the independent oracle. Code is never
  the source of truth
- [`conformance/README.md`](conformance/README.md) — the vectors, the coverage
  manifest, and how to test a decoder that is not this one
- [`inspector/README.md`](inspector/README.md) — the projection room

## Contributing

See [the contribution guide](docs/contributing.md), the numbered decisions in
[`docs/adr/`](docs/adr/), and the pull-request checklist. Every change needs
tests and a clean local preflight. Anything decodable carries a version: a
syntax change bumps it, regenerates the oracle vectors, and is reviewed against
both decoders.

## License

Copyright (c) 2024 Sidakpreet Singh.

Key Frame is released under the MIT License — a codec and its bitstream want
maximum reuse. The complete license text is in [LICENSE](LICENSE); the SPDX
identifier is `MIT`.

---

**Version:** v0.0.2

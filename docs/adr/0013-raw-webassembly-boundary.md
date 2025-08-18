# ADR-0013: Raw WebAssembly Boundary

## Context

The projection room needs the decoder in a browser, and the bit-exactness
constitution needs proof that the browser copy decodes to the same bytes as the
native one. The obvious route was `wasm-bindgen`, which the dependency policy
already permits.

Two things about that route did not survive contact with the requirement.

A `wasm-bindgen` module imports JavaScript functions. A module with imports
cannot be instantiated by a standalone runtime without supplying them, so the
equality check would have had to run a *differently built* module than the one
the application loads — and a gate that tests a different artifact proves
nothing about the shipped one.

The binding generator is also a version-locked pair: the `wasm-bindgen` crate
and its command-line tool must match exactly, and a mismatch fails at build
time with a message about a version this repository never chose. That is a
standing maintenance cost on a project whose whole claim is that its parts can
be read.

Against that, the surface it would generate is small. The application needs to
hand over a byte buffer, ask for a frame, and read back planes. That is an
allocate-write-call-read contract, not an object graph.

## Decision

`kf-wasm` exports a raw boundary over linear memory: `kf_alloc`, `kf_free`,
`kf_open`, `kf_decode_frame`, `kf_decode_all`, the stream-shape readers, and
paired pointer/length readers for output and error text. Status codes are
numbered and stable; error text is UTF-8 for humans, never parsed.

The module imports nothing, so the same file runs in a browser and under Node's
WebAssembly engine. `scripts/ci/wasm-gate.sh` builds it and decodes every
committed conformance stream inside it — once whole and once frame by frame
through random access — comparing both against the decoded-YUV hashes the
native decoders committed.

Offsets and lengths are pointer-width rather than `u32`. On `wasm32` that is
the same 32-bit value a host sees either way; natively it keeps the test build
honest instead of truncating a 64-bit address.

`kf-wasm` does not inherit the workspace lint table. The workspace forbids
`unsafe_code`, and a WebAssembly export cannot exist without it: the
`#[unsafe(no_mangle)]` attribute is itself what the lint names. The crate denies
`unsafe_code` instead and allows it on exactly one module, so the ban still
holds everywhere else. Every `unsafe` block in that module states the invariant
the host must uphold. The decode logic lives in a separate safe module that is
tested natively, so the unsafe surface is the boundary and nothing more.

## Consequences

The artifact under test is the artifact that ships. There is no glue file, no
generator version to pin, and nothing between the browser and the decoder that
the equality gate does not also exercise.

The application must do its own pointer arithmetic: allocate, write, call, then
read `kf_output_ptr` *after* the call, because an allocation inside the module
may have grown its memory and detached an earlier view. That is a real
obligation, and it is documented at the top of the boundary module.

Structured output — the probe's syntax JSON — will cross this boundary as UTF-8
bytes behind the same pointer/length pair, not as a JavaScript object. The
probe itself is not in `kf-wasm` yet: it lives in `kf-tools`, which the crate
graph does not allow this crate to depend on. Moving it is the projection room's
problem to solve, not this record's.

Adopting `wasm-bindgen` later remains possible and would not change the decoder;
it would change who writes the glue.

## Alternatives considered

- `wasm-bindgen` as originally planned: rejected because its imports make the
  shipped module unrunnable by the equality gate, which would have to test a
  second build instead.
- A WASI binary with a `main`: rejected because the browser is the target, and
  it would trade a JavaScript dependency for a system-interface one.
- Keeping `unsafe_code` forbidden and generating exports another way: there is
  no other way. A WebAssembly module without exports is unreachable.

## Supersedes / superseded by

None.

# kf-ref

`kf-ref` is the deliberately plain decoder used to arbitrate disagreements. It
depends only on inert frame storage, frozen specification bytes, and the Rust
standard library. It owns independent byte reading, CRC32C, range decoding,
context adaptation, inverse transform, prediction, and state commit code.

No production codec crate may become a dependency. The repository gate checks
both the manifest graph and source imports.

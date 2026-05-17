# Corpus

The repository does not redistribute source clips. `manifest.toml` pins the
initial two-clip QCIF subset of Xiph.Org's Derf test-media collection and the
checksums published by the same host. Run `scripts/fetch-corpus.sh`; downloaded
files live in ignored `corpus/clips/`.

`manifest.toml` is the only copy of that pinning. Everything that needs it —
the fetch script, the bit-exactness gate, the rate gate — reads it through
`scripts/corpus-manifest.sh`, and every field it declares is checked against
the clip that arrives: the checksum against the bytes, and the width, height,
and frame count against the Y4M header and the file's own length. A clip that
disagrees with any of them stops the build rather than being measured.

The address is the one field a cached clip cannot re-check, since the fetch
only reaches the network when the file is missing. It is exercised on every
machine that does not already have the corpus, which is every continuous
integration run.

The subset deliberately pairs the mostly static Akiyo sequence with the higher
motion Foreman sequence. It is a development gate, not the final rate–distortion
corpus or a claim that two clips characterize codec quality.

# Corpus

The repository does not redistribute source clips. `manifest.toml` pins the
initial two-clip QCIF subset of Xiph.Org's Derf test-media collection and the
checksums published by the same host. Run `scripts/fetch-corpus.sh`; downloaded
files live in ignored `corpus/clips/`.

`manifest.toml` is the only copy of that pinning. Everything that needs it
reads it from there: the fetch script, the bit-exactness gate, and the rate gate
through `scripts/corpus-manifest.sh`, and the rate–distortion campaign through
`kf_tools::pinned_clips`, because a Rust program that shelled out to bash to
learn its own clip list would be a worse arrangement than a second reader. Every
field the manifest declares is checked against the clip that arrives: the
checksum against the bytes, and the width, height, and frame count against the
Y4M header and the file's own length. A clip that disagrees with any of them
stops the build rather than being measured.

Two readers of one file only mean anything if something compares them.
`crates/kf-tools/tests/corpus_manifest.rs` runs both over the committed manifest
and over the shapes where they could plausibly part — a second table after the
last clip, indented keys, a trailing comment — and requires the same records
from each. They parted on all three, and the first was silent: a `[fetch]`
section after the last clip donated its `url` to that clip on the shell side, so
the fetch script would have downloaded a different file under the pinned name
with every declared field still looking present.

The address is the one field a cached clip cannot re-check, since the fetch
only reaches the network when the file is missing. It is exercised on every
machine that does not already have the corpus, which is every continuous
integration run.

The subset deliberately pairs the mostly static Akiyo sequence with the higher
motion Foreman sequence. It is a development gate, not the final rate–distortion
corpus or a claim that two clips characterize codec quality.

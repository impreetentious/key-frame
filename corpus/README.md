# Corpus

The repository does not redistribute source clips. `manifest.toml` pins the
initial two-clip QCIF subset of Xiph.Org's Derf test-media collection and the
checksums published by the same host. Run `scripts/fetch-corpus.sh`; downloaded
files live in ignored `corpus/clips/`.

The subset deliberately pairs the mostly static Akiyo sequence with the higher
motion Foreman sequence. It is a development gate, not the final rate–distortion
corpus or a claim that two clips characterize codec quality.

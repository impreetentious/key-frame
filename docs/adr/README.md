# Decision records

Every decision that constrains the format, the verification story, or the shape
of the repository, in the order it was taken. They are append-only: a record is
superseded by a later one rather than edited, because the reasoning at the time
is the part worth keeping.

[`0000-template.md`](0000-template.md) is the shape a new one takes. A normative
change needs one of these, an explicit bitstream-version decision, new oracle
vectors, regenerated documentation, and review against both decoder
implementations.

- [ADR-0001](0001-foundation-decisions.md) — Foundation decisions
- [ADR-0002](0002-worked-vector-augmentation.md) — Worked-vector augmentation
- [ADR-0003](0003-packet-vector-completion.md) — Packet vector completion
- [ADR-0004](0004-intra-oracle-stream.md) — Intra oracle stream
- [ADR-0005](0005-probe-schema-freeze.md) — Freeze the probe version-one wire format
- [ADR-0006](0006-complete-motion-compensation-phases.md) — Complete the version-one motion-compensation phases
- [ADR-0007](0007-pin-motion-search-centers.md) — Pin canonical motion-search centers
- [ADR-0008](0008-deblock-filter-vectors.md) — Deblock filter vectors
- [ADR-0009](0009-reconstruction-freeze.md) — Reconstruction freeze
- [ADR-0010](0010-rate-control-tables.md) — Rate-control tables
- [ADR-0011](0011-rate-bucket-initial-fill.md) — Initial Rate-Control Bucket Fill
- [ADR-0012](0012-reserved-context-slots.md) — Reserved Context Slots
- [ADR-0013](0013-raw-webassembly-boundary.md) — Raw WebAssembly Boundary
- [ADR-0014](0014-probe-crate-extraction.md) — Probe Crate Extraction
- [ADR-0015](0015-conformant-encoder-ablations.md) — Conformant Encoder Ablations
- [ADR-0016](0016-complexity-term-in-rate-control.md) — Complexity Term in Rate Control
- [ADR-0017](0017-checked-arithmetic-in-release.md) — Checked Arithmetic in Release Builds

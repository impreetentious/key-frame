# ADR-0009: Reconstruction freeze

## Context

The encoder closed loop and both decoders now run every version-one
reconstruction stage, including the integer deblocking filter whose output is
installed as LAST and GOLDEN. Streams that lived under `conformance/staging/`
were reproducibility receipts for an unfinished pipeline. Leaving them as the
public suite would present a pre-filter hash as an interoperability claim.

## Decision

Pin bitstream version at 1. Replace the staging receipts with a committed
three-origin suite under `conformance/`:

- `oracle/` — streams emitted by the standard-library writer in `spec/oracle.py`
- `hand/` — streams constructed at the syntax layer and reviewed as literals
- `encoder/` — streams produced by the canonical encoder at pinned settings

No origin may silently replace another. Regeneration is a scripted, reviewed
event. Syntax changes still require a new architectural decision, a version
bump, and regenerated vectors.

## Consequences

Continuous integration reproduces every origin's stream bytes and decoded
raw-YUV hashes on both supported native targets. The staging directory is
deleted rather than kept as a second source of truth.

## Alternatives considered

- Keep staging hashes and relabel them as frozen: rejected because the label
  would hide that they were collected before the filter was live.
- Encode the oracle and hand cases with `kfenc`: rejected because that would
  collapse three origins into one.

## Supersedes / superseded by

Extends [ADR-0001](0001-foundation-decisions.md) item 21 and the oracle-stream
decision of [ADR-0004](0004-intra-oracle-stream.md).

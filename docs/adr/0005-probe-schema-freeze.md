# ADR 0005: Freeze the probe version-one wire format

- Status: accepted
- Date: 2025-06-26
## Context

The projection room consumes `kfprobe` output, so field names and accounting
semantics must stabilize before its UI can be built independently. Range-coder
carry also makes emitted bytes a property of replay time, not a defensible owner
assignment to individual syntax elements or blocks.

## Decision

Freeze `spec/v1/probe.schema.json` as probe version 1. Reports separate modeled
Q16 entropy from canonical-shadow-encoder emission timing at coding-block,
superblock-structure, and frame-finalization boundaries. Their emitted-byte
buckets conserve the canonical replay length. They equal the input length only
when byte-for-byte replay matches; a valid noncanonical tail is reported as a
mismatch and receives no fabricated attribution.

The schema reserves an optional syntax trace with the same accounting labels.
The dependency-free validation gate executes `kfprobe` on the independent
oracle stream and validates its JSON against the committed schema.

## Consequences

Inspector fixtures can depend on a stable JSON contract. Changing the
shape requires a new `probe_version`; changing payload syntax remains governed
separately by the bitstream-version policy.

---
id: "000"
crate: kf-ref
found-by: fuzz
regression: conformance/crashes/false_sync_prefix.kfv
fixed-in: v0.11.1
---

# False sync prefix rejected by the reference decoder

## Symptom

A structurally valid stream with eight garbage bytes that happen to begin
`KFP1` between the sequence header and the first real packet decoded in the
fast decoder and failed in the reference decoder.

## Hunt

The deterministic structured-mutation campaign inserted a false sync word.
Replaying the 66-byte buffer showed the fast packet scanner advancing one byte
per failed header candidate, while the reference decoder required a valid
packet at the first byte after the sequence header.

## Root cause

The reference decoder parsed packets at a running offset and treated any
header failure as a stream error. The bitstream rule is that a failed
sync or header candidate advances one byte.

## Fix

The reference decoder now walks the packet region with its own cursor: failed
headers skip one byte, a payload-CRC failure still consumes the declared
extent, and only a fully validated packet is reconstructed.

## Lesson

Resynchronization has to be written twice, independently, or a shared skip
becomes a shared blind spot. A campaign that inserts `KFP1` into valid
streams is the cheapest way to notice the copy that forgot to skip.

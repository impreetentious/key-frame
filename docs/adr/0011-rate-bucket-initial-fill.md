# ADR-0011: Initial Rate-Control Bucket Fill

## Context

The leaky-bucket update and third-boundary QP steps are recorded in
`spec/v1/quant.toml`. The starting fill was unspecified. Starting at zero
makes every first frame look underfull, so QP falls by the maximum step until
a frame finally exceeds the budget, after which the clip overshoots.

## Decision

Initialize fill at half of bucket capacity. That value sits in the middle
third, so the first-frame QP holds until coded size actually leaves the
band. Bitstream version remains 1.

## Consequences

`RateController::new` starts in the hold band. Extreme-bitrate bound traps
still saturate to empty or full. Corpus ABR accuracy is measured against this
startup rule.

## Alternatives considered

- Start empty: rejected because it forces a QP collapse on every encode.
- Start full: rejected because it would raise QP before any frame is coded.

## Supersedes / superseded by

Extends [ADR-0010](0010-rate-control-tables.md).

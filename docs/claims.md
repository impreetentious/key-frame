# Claims and the gates that enforce them

Every substantive claim the README makes, and the check that would fail if it
stopped being true. Nothing on this page is enforced by this page: each row
names a script or a test that runs in `./scripts/preflight.sh`, and
`scripts/ci/claims-coherence.sh` refuses to pass if a row names something that
does not exist.

The point is not that the claims are documented. It is that none of them is an
assertion a reader has to take on trust — every one is a thing the build already
proves on every change, and this table is the index.

## The codec

| Claim | Enforced by |
| --- | --- |
| Progressive 8-bit 4:2:0 `C420jpeg` Y4M, even dimensions from 64×64 to 4096×2304 | `crates/kf-bitstream/tests/normative_limits.rs` drives the real constructor with the bounds `spec/v1/constants.toml` declares and requires everything outside them to be refused |
| Quadtree partition from 64×64 down to 8×8, chosen by closed-loop rate–distortion search | `scripts/ci/intra-gate.sh`, and `crates/kf-tools/tests/ablation_quality.rs` measures what the quadtree is worth |
| Eight intra modes; P-frames from LAST and GOLDEN with quarter-pixel six-tap motion compensation, median motion-vector prediction, and reference-selecting skip | `scripts/ci/inter-gate.sh` covers every P-frame branch; the six filter taps are frozen in `spec/v1/mc.toml` and replayed by `spec/oracle.py` |
| Literal 4/8/16/32 integer transforms, flat quantization across the full 64-step quantizer range | `scripts/ci/transform-gate.sh` replays 24 literal vectors at every size and saturates the extremes |
| Deblocking before a frame becomes a reference | `scripts/ci/deblock-gate.sh`, including `trap_loopfilter_ref_identity` |
| Each declared filter-activity skip rule has a vector that fails if the rule is dropped | `spec/check_assets.py` requires every declared skip condition to name a vector that leaves its samples untouched, and both decoders replay all seven through `scripts/ci/deblock-gate.sh`. Two of the three rules previously had none: removing either from both decoders left the deblock gate green and showed up only as an unexplained conformance hash drift |
| Sync-marked packets with independent header and payload CRC32C, byte-by-byte resynchronization after damage, and a frame installed into the reference state only on complete success | `scripts/ci/bitstream-gate.sh` covers the header, checksum, and resynchronization rules; `scripts/ci/error-matrix-gate.sh` covers what happens when they fire |
| Every syntax element round-trips, and the partition tree, prediction branches, and coefficient coding mean what the tables say | `scripts/ci/syntax-gate.sh` |
| Adaptive binary range coder over a closed set of 144 contexts | `scripts/ci/range-gate.sh` and `crates/kf-range/tests/normative_limits.rs`, which check the probability clamps, the adaptation rate, the context count, and the finalization tail against the frozen constants |
| Modeled-entropy costs the encoder reuses for its own decisions | `scripts/ci/entropy-gate.sh` |
| The declared per-phase interpolation blend table is the blend both decoders perform | `spec/check_assets.py` evaluates every declared luma and chroma phase sequence against the blend the codec derives from the phase index, comparing on sample values rather than symbolically because the table writes its fractions reduced |

## Verification

| Claim | Enforced by |
| --- | --- |
| Two decoders, written independently, agreeing bit-exactly | `scripts/ci/reference-gate.sh` and `scripts/ci/conformance-gate.sh`; `scripts/ci/forbidden-grep.sh` fails the build if `kf-ref` ever imports a production codec crate or declares a dependency beyond `kf-frame` and `kf-spec` |
| Conformance coverage stated as a measurement: every reachable syntax element and context slot, with at least one stream per element the encoder did not author | `scripts/ci/coverage-gate.sh`, which instruments both decoders and counts |
| Bit-exactness on natural video, not only synthetic sources | `scripts/ci/corpus-gate.sh`, both pinned clips at five quantizers in both decoders and the encoder's closed loop |
| Damage detected, contained, and never allowed into later prediction | `scripts/ci/error-matrix-gate.sh`, thirteen named traps in both decoders plus a single-byte wound sweep |
| Decoder campaigns with zero panics or hangs | `scripts/ci/fuzz-gate.sh`; the nightly budget is checked against the declared one by `crates/kf-fuzz/tests/campaign.rs` |
| Any frame seekable to the same image linear decoding produces | `scripts/ci/seek-gate.sh` |
| The specification and the implementation cannot drift apart silently | `scripts/ci/spec-check.sh`: the normative document is regenerated and diffed, `spec/check_assets.py` re-derives the tables, and `spec/mutation_check.py` mutates each asset and requires a rejection |
| No number or table the specification declares is decoration: every numeric scalar and every declared array in the frozen assets is reachable from something that runs | `scripts/ci/declared-scalar-use.sh`, which fails on any declared scalar or table no crate, specification script, CI script, or interface source names — the check that would have caught a coefficient-coding parameter table describing a scheme this codec never implemented. The generated document deliberately does not count as a reader — everything appears there, which is what makes it a document rather than a check — and the gate carries no exemption list, because a number nobody can justify checking is a number the specification should not declare |
| The header offsets, checksum spans, and flag masks a decoder trusts before it trusts anything else are derived from the declared field layout, not asserted beside it | `spec/check_assets.py` rebuilds both header layouts from their own field lists and requires every offset, length, and mask to fall out of them — including that the packet checksum begins after the sync marker, since resynchronization scans for that marker and cannot have it under the checksum |
| Where two assets state the same fact, they are made to agree | `spec/check_assets.py` reconciles the constant table against the component assets that restate it — the scene-cut bounds, the full-pixel motion range, the quantizer range, the coefficient cap, and the probability clamps — so an edit to either copy fails rather than leaving the normative document contradicting itself |
| Every declared scalar is load-bearing, not decorative normative text — the transform stage shifts and reconstruction clamp, and the interpolation taps, filter scale, rounding biases, phase denominators, and edge extension | `crates/kf-transform/tests/declared_shifts.rs` evaluates the shift formulas the asset declares and compares them to the shifts the code uses; for both transforms and motion compensation the production path, `kf-ref`, and `spec/check_assets.py` each read the same declarations through their own parser rather than restating the numbers, so no two of the three agree except by agreeing with the specification. Scalars the committed vectors cannot police — a rounding bias that rounds the same way either side, a phase denominator no committed motion vector is large enough to distinguish — are pinned by the relations that hold between them in `spec/check_assets.py`, and `spec/mutation_check.py` rejects a change to any of them |

## Integer-only signal path

| Claim | Enforced by |
| --- | --- |
| No floating point, unordered iteration, ambient clocks, filesystem, or threads in any codec crate | `scripts/ci/forbidden-grep.sh`, fail-closed: every crate under `crates/` is scanned unless it is named as a host crate, so a new codec crate is covered the day it appears |
| Floating point confined to the tools and the interface | the same scan, plus `disallowed_types = "deny"` in the workspace lint table |
| Every numeric cast in a codec crate either cannot lose anything or carries the rule that says what it discards | the same scan reads the comment block directly above each `as` cast and fails unless it begins `cast:`. Four survive the rule, and the load-bearing one is the range encoder's delayed carry in `crates/kf-range/src/encoder.rs`, where `try_from` would reject exactly the case the carry logic exists to handle. A cast is invisible to both the lint build and the checked-arithmetic release profile, because truncation is not an overflow — it is an answer |
| Release builds stop on an integer overflow rather than wrapping | `crates/kf-fuzz/tests/campaign.rs` asserts it, and the campaign binary asks `overflow_is_checked()` of itself before its first iteration and refuses to report a clean run from a build that would wrap. The artifact proves the setting, because `cargo test --release` and `cargo run --release` select different profiles and a test elsewhere would be asking the wrong build |

## The browser

| Claim | Enforced by |
| --- | --- |
| The WebAssembly module imports nothing | `scripts/ci/wasm-gate.sh` |
| An allocation the module cannot satisfy is reported to the host, not fatal to the instance | `crates/kf-wasm/src/abi.rs` allocates fallibly and returns offset zero, with `an_impossible_allocation_is_reported_rather_than_fatal` requiring the module to stay usable afterwards. Allocating infallibly would abort, which on this target traps and kills the instance, leaving the host's error path unreachable |
| The module decodes every committed stream identically to the native build | `scripts/ci/wasm-gate.sh`, pixels and syntax reports, whole-stream and per-frame |
| Nothing on the page is a pre-decoded picture | `scripts/ci/site-budget.mjs` fails on any `.yuv` or `.y4m` in the build, and the browser suite reads pixels out of the canvas rather than checking that an element rendered |
| The page loads, scrubs, draws overlays, answers a click with syntax, and shares a view as a link | `inspector/tests/smoke.spec.ts` via `scripts/ci/site-gate.sh` |
| Size budgets | `scripts/ci/site-budget.mjs`, with the budgets read from `spec/v1/constants.toml` rather than written twice |

## Measurement

| Claim | Enforced by |
| --- | --- |
| Quality metrics pinned to the last constant, checked against a second implementation | `scripts/ci/metric-gate.sh`: `bench/metric_oracle.py` derives the vectors by direct 2D convolution and Gauss–Legendre quadrature, and `crates/kf-tools/tests/metric_vectors.rs` replays them against the separable and closed-form Rust |
| Per-block JSON that validates against the frozen schema, with byte accounting that reconciles against a canonical replay | `scripts/ci/probe-gate.sh`, which also requires a valid noncanonical tail to report a mismatch without attributing it to any block |
| Every published figure regenerable, and a receipt that fails when a figure moves | `scripts/ci/metric-gate.sh` edits a figure and requires `kfmetric repro` to reject it |
| Rate–distortion and ablation curves reproduce from the checked-in encoder | `scripts/ci/rd-gate.sh` re-encodes both ends of every ladder; the nightly job re-derives all sixty points |
| No compression regression against the previous release beyond the allowance | `scripts/ci/rd-gate.sh` compares against `bench/results/rd-baseline.json` and plants a ten-percent-worse baseline every run to prove the check can fail |
| Average-bitrate targets reported separately and never fitted as a curve | `crates/kf-tools/tests/receipt_shape.rs` requires them to carry no rate/quality pair, so the interpolator refuses them by name |
| Average-bitrate accuracy | `scripts/ci/rate-gate.sh` at two disclosed operating points; the wider envelope is in [`LIMITATIONS.md`](LIMITATIONS.md) |

## Repository

| Claim | Enforced by |
| --- | --- |
| One version across every surface | `scripts/check-version-coherence.mjs` |
| One license, stated consistently wherever the repository states one | `scripts/ci/license-coherence.sh` |
| No broken local link in any document, and an unbroken architectural decision sequence | `scripts/ci/doc-coherence.sh` |
| One command runs every gate continuous integration runs, in the same order | `scripts/preflight.sh`, which the `check` workflow runs verbatim |

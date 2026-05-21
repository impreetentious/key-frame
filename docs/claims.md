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
| The declared motion-vector predictor candidates are the neighbours both decoders read, in the order and with the fallback the declaration names | `crates/kf-predict/tests/declared_predictor.rs` and the `declared_predictor` module in `crates/kf-ref/src/motion.rs`, each reading `spec/v1/mc.toml` through its own parser. A candidate name neither recognises is a failure rather than a skip, and the neighbourhood is built so that a bare majority carries one marker — a median ignores its extremes, so a field that merely gives each candidate a distinct vector passes while a candidate is read from the wrong cell |
| Where two assets state the same fact, or one asset states it twice, they are made to agree — including the coded plane list against the residual plane order | `spec/check_assets.py`, with `spec/mutation_check.py` requiring a rejection when either copy moves |
| The declared per-phase interpolation blend table is the blend both decoders perform | `spec/check_assets.py` evaluates every declared luma and chroma phase sequence against the blend the codec derives from the phase index, comparing on sample values rather than symbolically because the table writes its fractions reduced |

## Verification

| Claim | Enforced by |
| --- | --- |
| The packet bounds both decoders enforce are the ones the specification declares | `crates/kf-bitstream/tests/normative_limits.rs` drives the production constructor with the declared payload cap and initial read, and `crates/kf-ref/src/scan.rs` reads the same two declarations through its own parser rather than restating them — `the_payload_bounds_are_the_ones_the_specification_declares` refuses a header one byte past the cap and one byte under the minimum, which is where no committed vector reaches |
| Two decoders, written independently, agreeing bit-exactly | `scripts/ci/reference-gate.sh` and `scripts/ci/conformance-gate.sh`, which compares the committed suite against a fresh generation in both directions — a vector that changed or vanished, and a vector present that nothing authors, since a file left behind by a rename is decoded by the gates that glob these directories and described by no manifest; `scripts/ci/forbidden-grep.sh` fails the build if `kf-ref` ever imports a production codec crate or declares a dependency beyond `kf-frame` and `kf-spec` |
| Conformance coverage stated as a measurement: every reachable syntax element and context slot, with at least one stream per element the encoder did not author | `scripts/ci/coverage-gate.sh`, which instruments both decoders and counts |
| Bit-exactness on natural video, not only synthetic sources | `scripts/ci/corpus-gate.sh`, every pinned clip at five quantizers in both decoders and the encoder's closed loop |
| Every field the corpus manifest pins is checked against the clip that arrives | `scripts/fetch-corpus.sh` reads the address, checksum, dimensions, and frame count from `corpus/manifest.toml` through `scripts/corpus-manifest.sh` and refuses a clip that disagrees with any of them. `scripts/ci/corpus-gate.sh`, `scripts/ci/rate-gate.sh`, and the rate–distortion campaign take their clip list from the same place — the campaign through `pinned_clips` in `crates/kf-tools/src/corpus.rs` — so a clip the corpus gains is measured rather than merely declared, and a manifest that parses to nothing is a failure rather than a loop that runs zero times |
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
| No Rust dependency but the workspace's own crates | `scripts/ci/dependency-closure.sh` requires every package `Cargo.lock` resolves to be a member of this workspace, and the member list to be exactly the crates on disk — a crate directory no member line names is scanned by the forbidden-API gate and compiled by nothing |
| No floating point, unordered iteration, ambient clocks, filesystem, or threads in any codec crate, the WebAssembly module included | `scripts/ci/forbidden-grep.sh`, fail-closed: every crate under `crates/` is scanned unless it is named as a host crate, so a new codec crate is covered the day it appears. Two crates are named — the tools and the fuzz harness — and both allow `disallowed_types` and mean it. `kf-wasm` is scanned like any other: the addresses it hands a host are exposed and recovered explicitly rather than cast, so the artifact a visitor's browser runs is inside the perimeter rather than beside it |
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
| The page loads, scrubs, draws overlays, answers a click with syntax, and shares a view as a link | `inspector/tests/smoke.spec.ts` via `scripts/ci/site-gate.sh`, including a round trip of every overlay on its own: overlays travel as a string of initials, which is correct only while the initials stay distinct, and nothing in the types says they must |
| Size budgets | `scripts/ci/site-budget.mjs`, with the budgets read from `spec/v1/constants.toml` rather than written twice |

## Measurement

| Claim | Enforced by |
| --- | --- |
| Quality metrics pinned to the last constant, checked against a second implementation | `scripts/ci/metric-gate.sh`: `bench/metric_oracle.py` derives the vectors by direct 2D convolution and Gauss–Legendre quadrature, and `crates/kf-tools/tests/metric_vectors.rs` replays them against the separable and closed-form Rust |
| Every document the tools read is parsed rather than searched, and no file a caller chose can make one panic | `crates/kf-tools/tests/y4m.rs`: a header token whose first character is more than one byte wide, an unknown token, and dimensions outside the format are each a named error with a byte offset, where the reader used to split every token at byte one and abort the process |
| A written Y4M carries the displayed picture, whatever shape the buffer behind it has | `crates/kf-tools/tests/y4m.rs` writes a frame whose planes are wider in memory than in picture and requires the file to be the size the format says and every sample to survive the round trip |
| One Y4M reader in Rust and one in the browser accept the same clips | `inspector/tests/encoding.spec.ts` holds the page's reader to the codec's own bounds — one chroma siting, even dimensions from 64×64 to 4096×2304 — where it used to take three more sitings and any positive size, so the page would compare a stream against a clip `kfenc` could not have encoded |
| Every document the tools read is parsed rather than searched, and refuses malformed input with a byte offset instead of a crash | `crates/kf-tools/src/json.rs` and its tests: a key inside a note is not mistaken for the key, member order survives a round trip, and nesting is bounded — `nesting_is_bounded_and_the_bound_is_reachable` requires the bound to be both enforced and reachable, because unbounded recursion on a nested document is a stack overflow, and a stack overflow aborts the process rather than naming an offset |
| Nothing the page cannot draw reaches a canvas coordinate, and no document it fetches is rendered unchecked | `inspector/tests/encoding.spec.ts` drives the page's three pure readers directly: the probe reader refuses a non-finite position, motion vector, or mismatch offset rather than passing a `NaN` to an overlay; the catalogue reader refuses an entry missing any field that makes a finding reproducible; and every view state survives the round trip through a shared link while a malformed link falls back to the default |
| A damaged stream is walked to its end, and what a player would show is the timeline the corruption rules define | `crates/kf-tools/tests/decoder_tool.rs`: `kfdec --tolerate` writes one frame per accepted packet, repeating the last shown image across a corrupt frame and the dependency loss behind it and repeating nothing before the first image exists, while the same stream without `--tolerate` is refused and writes no file |
| Per-block JSON that validates against the frozen schema, with byte accounting that reconciles against a canonical replay, and a readable rendering of the same report that never adds the two figures | `scripts/ci/probe-gate.sh`, which also requires a valid noncanonical tail to report a mismatch without attributing it to any block; `crates/kf-tools/tests/probe.rs` drives `kfprobe --summary` and requires the partition, both accounting figures rendered from the report's own fixed point, and the sentence that keeps them apart |
| Every campaign and size budget the build enforces is the one the specification declares | `crates/kf-fuzz/tests/campaign.rs`: `the_campaign_budgets_are_the_ones_the_specification_declares` holds both compiled counts to `spec/v1/constants.toml` and settles their ordering at compile time; the nightly workflow names no count of its own, `scripts/ci/fuzz-gate.sh` and `scripts/ci/site-budget.mjs` and `scripts/ci/wasm-equality.mjs` each read the declaration, and `scripts/ci/declared-scalar-use.sh` fails the build if any script spells one out instead |
| Every published figure regenerable, and a receipt that fails when a figure moves | `scripts/ci/metric-gate.sh` edits a figure and requires `kfmetric repro` to reject it |
| Rate–distortion and ablation curves reproduce from the checked-in encoder | `scripts/ci/rd-gate.sh` re-encodes both ends of every ladder; the nightly job re-derives all sixty points |
| The projection room states measured figures and derives none of its own | `crates/kf-tools/tests/receipt_shape.rs`: `every_ablation_curve_states_its_bitrate_difference` requires each ablation curve to record exactly one bitrate difference and the baseline to record none; `crates/kf-tools/examples/rd_verify.rs` recomputes each one and compares it, refusal text included; the browser smoke suite fetches the receipt the page loaded and matches every rendered cell against it |
| No compression regression against the previous release beyond the allowance | `scripts/ci/rd-gate.sh` compares against `bench/results/rd-baseline.json` and plants a ten-percent-worse baseline every run to prove the check can fail |
| Average-bitrate targets reported separately and never fitted as a curve | `crates/kf-tools/tests/receipt_shape.rs` requires them to carry no rate/quality pair, so the interpolator refuses them by name |
| The pinned corpus is one file, read by two programs that are required to agree | `crates/kf-tools/tests/corpus_manifest.rs` runs `scripts/corpus-manifest.sh` and `kf_tools::pinned_clips` over the same text — the committed manifest and the shapes where they could part — and compares every field they share |
| Average-bitrate accuracy | `scripts/ci/rate-gate.sh` at two disclosed operating points; the wider envelope is in [`LIMITATIONS.md`](LIMITATIONS.md) |

## Repository

| Claim | Enforced by |
| --- | --- |
| One version across every surface | `scripts/check-version-coherence.mjs` |
| One license, stated consistently wherever the repository states one | `scripts/ci/license-coherence.sh` |
| No broken local link in any document, and an unbroken architectural decision sequence | `scripts/ci/doc-coherence.sh` |
| One command runs every gate continuous integration runs, in the same order | `scripts/preflight.sh`, which the `check` workflow runs verbatim |

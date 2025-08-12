# kf-fuzz

Deterministic decoder campaigns over arbitrary bytes, structured mutations of
valid streams, sequence-header damage, and packet deletion. Each iteration must
return a structured error or a pair of identical reconstructions; neither
decoder may panic, hang, or keep a reference after a failed frame.

The preflight smoke uses a few thousand iterations. Nightly continuous
integration runs twenty million iterations per target. Findings become
regression streams under `conformance/crashes/` and, when they teach something,
entries under `docs/cutting-room/`.

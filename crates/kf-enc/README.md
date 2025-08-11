# kf-enc

Deterministic closed-loop Key Frame encoder. The initial intra path performs
integer RDO over all eight prediction modes and records modeled entropy and
canonical range-emission timing as separate quantities.

After every superblock the encoder snapshots the 144-wide `p1` bank. Both
decoders replay the same checkpoints so a context drift is visible at the first
disagreeing superblock rather than only at the reconstructed frame. P-frames
start from the prior committed bank; a syntax error discards the in-flight
copy so a partial adaptation cannot become the next frame's state.

A Q16.16 leaky-bucket controller can choose per-frame QP from a target
bitrate. Fill saturates inside a bucket sized at twice the per-frame budget
times an eight-frame window; QP steps at most two from the bucket's thirds.
After a frame is fully reconstructed, the integer deblocking filter runs
before LAST or GOLDEN is updated. The filtered image is the reference; the
unfiltered reconstruction is discarded.

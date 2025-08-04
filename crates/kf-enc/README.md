# kf-enc

Deterministic closed-loop Key Frame encoder. The initial intra path performs
integer RDO over all eight prediction modes and records modeled entropy and
canonical range-emission timing as separate quantities.

After every superblock the encoder snapshots the 144-wide `p1` bank. Both
decoders replay the same checkpoints so a context drift is visible at the first
disagreeing superblock rather than only at the reconstructed frame.

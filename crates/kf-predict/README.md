# kf-predict

Scalar, integer-only intra and inter prediction for Key Frame v1. The crate
reads only already reconstructed samples from `kf-frame`; codec syntax stays
outside this layer so encoder and fast decoder can share identical
reconstruction math.

Deblocking lives here for the same reason: the encoder closed loop and the
fast decoder must apply the identical integer filter before a frame enters a
reference slot.

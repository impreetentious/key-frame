# kf-predict

Scalar, integer-only intra and inter prediction for Key Frame v1. The crate
reads only already reconstructed samples from `kf-frame`; codec syntax stays
outside this layer so encoder and fast decoder can share identical
reconstruction math.

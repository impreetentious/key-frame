# kf-dec

Fast scalar Key Frame decoder. The implementation consumes the production
syntax layer and shared reconstruction primitives; it remains deliberately
independent from the readability-first `kf-ref` decoder.

A traced decode records the committed context bank after every superblock. The
copy used while a frame is in flight is discarded on any syntax or
reconstruction failure, so a partial adaptation never becomes the next
frame's starting state.

The same integer deblocking filter the encoder uses runs after reconstruction
and before a frame is installed as LAST or GOLDEN.

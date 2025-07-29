# kf-transform

`kf-transform` applies the literal version-one matrices embedded by `kf-spec`.
Every multiply/accumulate is i64. Stage shifts are explicit, the first inverse
stage narrows only after rounding, and the second stage clamps to the signed
16-bit residual domain before prediction adds and 8-bit clipping occur in the
decoder.

The inverse is decoder-normative. The forward path is encoder-side and may be
replaced only when quantization and the closed reconstruction loop remain
bit-identical.

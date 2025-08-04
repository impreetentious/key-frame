# kf-range

`kf-range` implements the version-one adaptive binary range coder against the
literal vectors in `spec/v1/vectors.json`. `p1` always means the probability of
symbol one on a 4096-point scale. Context updates use signed floor division;
bypass bins use a fixed half probability and never adapt a context.

The decoder initializes from five bytes, bounds every renormalization read by
the validated payload slice, and leaves any unread arithmetic-finalization tail
opaque. No invented tail-length limit is applied.

Instrumentation deliberately separates modeled Q16 entropy from temporal byte
emission. Delayed carry can cause bytes to appear after later bins or during
finalization, so an emission event records when bytes became observable, never
which symbol owns them. The sum of all events must equal the canonical payload
length.

A coverage counter records which of the 144 frozen context ids were coded. It
is generated against that closed id range and rejects any id outside it. A
`p1` snapshot of the whole bank exists so encoder and decoder states can be
compared at superblock and frame boundaries without sharing live storage.

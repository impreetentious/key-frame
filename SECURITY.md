# Security policy

Key Frame is an educational codec, not production media infrastructure. It
provides no sandboxing of any kind and makes no warranty about the streams it
produces or consumes.

The security-relevant surface is the decoder. It parses untrusted `.kfv`
bitstreams and untrusted Y4M input with hand-rolled parsers, and it is expected
to reject malformed, truncated, or deliberately corrupted input with a
structured error inside documented allocation and iteration caps. A panic, a
hang, an unbounded allocation, an out-of-bounds access, or a decode that
proceeds past a corrupt packet without waiting for a valid keyframe is a bug
worth reporting even when the input is obvious garbage. The same applies to the
WebAssembly build, which decodes whatever a page hands it.

A crashing stream is the most useful bug report this project can receive: it
goes into the regression corpus permanently.

Please do not disclose a suspected vulnerability in a public issue before I
have had a chance to assess it. Use my private security contact and include the
affected build, the exact command, and the smallest reproducing stream.
Reproducing streams may be shared publicly after triage.

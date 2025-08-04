# kf-bitstream

`kf-bitstream` owns the production syntax boundary for `.kfv`. It validates
fixed headers before allocation or entropy decoding, reports the byte offset
and field for malformed input, and uses the bitwise CRC32C implementation in
`kf-core` checked against the independent oracle.

The bitstream version is independent of the repository semver. This crate
currently accepts version 1 only.

Packet headers are structurally validated before a payload is exposed. Failed
sync/header candidates advance one byte; a header-valid packet with a bad
payload checksum consumes its declared capped extent. The scanner tracks
strictly increasing frame indices without treating nominal GOP intervals as a
decoder schedule.

Syntax readers and writers record every frozen context id they code. Bypass
bins never appear in that set. The resulting coverage bitmap is compared
across encode and decode of the same payload so a silent id mismatch cannot
hide behind a matching reconstruction.

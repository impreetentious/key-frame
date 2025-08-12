# Cutting room

A catalogue of real decoder and encoder defects found by campaigns,
differential tests, or conformance. Entries are not invented to fill the
shelf.

Each finding is a `NNN-slug.md` file with:

- id, crate, found-by, regression stream path, fixed-in
- Symptom, Hunt, Root cause, Fix, Lesson

Regression streams live under `conformance/crashes/` and are decoded by the
ordinary test suite after the fix lands.

- [000-false-sync-prefix](000-false-sync-prefix.md) — false `KFP1` between the
  sequence header and the first packet.

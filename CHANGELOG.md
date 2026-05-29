# Changelog

## v0.2.0 — 2026-05-29

`daily-receipt` now emits a cadence record on every print. After computing
the byte-stable ESC/POS stream (unchanged), it shells out to
`cadence record daily --produced-by daily-receipt` so each day's strip is
registered as a canonical `daily` artifact the weekly composer can consume.

- New flag `--no-cadence-record` (default: record) preserves byte-stable,
  side-effect-free runs.
- New flag `--cadence-summary <s>` (default: derived from day-type + payload
  counts) sets the record's summary.

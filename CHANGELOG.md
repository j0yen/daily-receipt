# Changelog

## v0.3.0 — 2026-05-29

Add `archive` subcommand to `daily-receipt`: once per year (or on demand),
produce a PDF scroll of all daily strips for a given year, sourced from
the cadence substrate.  Layout: cover page + 12 month pages, each with a
7×5 day grid of strip thumbnails decoded from ESC/POS bytes.  Optional
`--include-scans` interleaves monthly phone-photographs.  After render,
emits a `yearly` cadence record for full substrate lineage.

- New `daily-receipt archive <YYYY>` — render annual PDF
- New `daily-receipt archive ls` — list rendered scrolls
- ESC/POS → grayscale PNG decoder (AC4 byte-identical determinism)
- Unknown ESC/POS bytes skipped with deduped stderr warning (AC5)
- Strip PNG cache with mtime idempotency (AC7)

## v0.2.0 — 2026-05-29

`daily-receipt` now emits a cadence record on every print. After computing
the byte-stable ESC/POS stream (unchanged), it shells out to
`cadence record daily --produced-by daily-receipt` so each day's strip is
registered as a canonical `daily` artifact the weekly composer can consume.

- New flag `--no-cadence-record` (default: record) preserves byte-stable,
  side-effect-free runs.
- New flag `--cadence-summary <s>` (default: derived from day-type + payload
  counts) sets the record's summary.

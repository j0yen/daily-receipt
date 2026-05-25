# daily-receipt

> Produce a deterministic, falsifiable Rust core for the Daily Receipt art project: given a day's structured summary plus a chosen day-type (workday | quiet | special) plus the content payload supplied by upstream (a haiku triple, a glyph seed, or a stamp id), emit a byte-stable ESC/POS command stream for one ~3-8cm thermal strip.

## Install

### One-liner

```sh
curl -fsSL https://raw.githubusercontent.com/j0yen/daily-receipt/main/install.sh | bash
```

### Manual

```sh
git clone --depth 1 https://github.com/j0yen/daily-receipt.git
cd daily-receipt
./install.sh
```

Installs the `daily-receipt` binary via `cargo install --path . --locked`. Requires `cargo` / `rustc 1.85+` and `git`. Built binary lands in `~/.cargo/bin/`.

## Why

Produce a deterministic, falsifiable Rust core for the Daily Receipt art project: given a day's structured summary plus a chosen day-type (workday | quiet | special) plus the content payload supplied by upstream (a haiku triple, a glyph seed, or a stamp id), emit a byte-stable ESC/POS command stream for one ~3-8cm thermal strip. Physical printing, scheduling, and Claude-API haiku generation are EXPLICITLY out of scope; the testable core is the encoder + day-type classifier + glyph renderer that downstream cron/printer wrappers consume. This isolates the failure-prone surface (ESC/POS byte sequences, classifier heuristics, glyph determinism) from the unfalsifiable surface (does this haiku spark joy).

## Build

```sh
cargo build --release
```

Produces `target/release/daily-receipt`. Symlink into `~/.local/bin/` if you want it on `$PATH`.

## Usage

```sh
daily-receipt --help
```

## Audience

the author on Arch Linux runs `daily-receipt render --summary day.json --content content.json --out strip.escpos`. Downstream a tiny shell wrapper pipes the file to /dev/usb/lp0 or saves it for the day. The Rust core never touches the printer; downstream wrappers and a future `daily-receipt-printer` crate own that.

## Acceptance criteria

This project was scaffolded from a PRD via the `autobuilder` pipeline. The MUST-level acceptance criteria are:

- **AC1**: `daily-receipt render --summary <day.json> --content <content.json> --out <path>` writes a non-empty ESC/POS byte stream to <path> for a workday haiku input and exits 0.
- **AC2**: Output bytes begin with the ESC/POS init sequence ESC '@' (0x1B 0x40) and end with a feed-and-cut sequence (GS V 0x42 0x00 = 0x1D 0x56 0x42 0x00). This is the printer-handshake contract every strip must honor.
- **AC3**: Rendering is deterministic: two `render` invocations with byte-identical summary+content JSON produce byte-identical ESC/POS output (no timestamps, no RNG, no map iteration order leaking in).
- **AC4**: Day-type classifier `classify` returns `workday` when summary has >=3 distinct repos OR >=10 commits, `special` when summary.special_stamp_id is set, and `quiet` otherwise. Exhaustive on the three variants; no fourth bucket.
- **AC5**: Haiku content must be exactly three lines, each line 1..=40 visible chars. `render` returns an error (exit code 3) when the content payload has the wrong shape for the day-type. No silent truncation, no panic.
- **AC6**: Glyph renderer for a quiet day emits a 24x24 monochrome bitmap encoded as the GS '*' raster image command, deterministic from the input seed (u64). Same seed -> identical bitmap bytes; different seeds -> different bitmaps (collision prob...
- **AC7**: Every strip embeds the ISO 8601 date (YYYY-MM-DD) from the summary in the printed bytes verbatim. `render --summary` with date='2026-05-23' produces output that contains the literal ASCII bytes '2026-05-23'.

Each AC has a matching integration test under `tests/acceptance_ac<n>.rs`.

## Provenance

Built via the [`autobuilder`](https://github.com/j0yen/autobuilder) pipeline (PRD intake -> intent-card -> scaffold -> iterate-and-prove). Originally consolidated as a subdir of the [`wintermute`](https://github.com/j0yen/wintermute) monorepo; this standalone repo is a fresh-init snapshot for easier consumption and distribution.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

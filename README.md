# daily-receipt

Turn one day of work into a byte-stable ESC/POS strip for a thermal printer — deterministic in, deterministic out.

A day produces a structured summary: which repos you touched, how many commits, whether the date is marked. `daily-receipt` classifies that day into one of three types, takes the content someone upstream chose for it — a haiku, a glyph seed, or a stamp — and renders one ~3–8 cm thermal strip as raw ESC/POS bytes. Nothing more.

## Why it exists

Printing a daily artifact has two halves, and they fail in different ways. One half is taste: is this haiku any good? That can't be tested. The other half is mechanism: are these the right ESC/POS init bytes, is the cut command at the end, does the same input always produce the same output? That *can* be tested, and it's where the bugs live. This crate is the second half, kept separate on purpose. It never composes a haiku, never talks to a printer, never reads the clock. It maps `(summary, content) → bytes`, and the same pair always gives the same bytes — so the part of the system that can be verified, is.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/j0yen/daily-receipt/main/install.sh | bash
```

Or from a clone:

```sh
git clone --depth 1 https://github.com/j0yen/daily-receipt.git
cd daily-receipt
./install.sh
```

`install.sh` runs `cargo install --path . --locked`; the binary lands in `~/.cargo/bin/`. Requires `cargo` / `rustc` 1.85+ and `git`.

## Quickstart

Render a strip from a day-summary and its content payload:

```sh
daily-receipt render --summary day.json --content content.json --out strip.escpos
```

`strip.escpos` is raw ESC/POS: it opens with the init sequence `1B 40`, embeds the summary's date verbatim as ASCII, and ends with the partial-cut command `1D 56 42 00`. To print, a one-line wrapper pipes the file at the printer (`cat strip.escpos > /dev/usb/lp0`); the core never touches the device.

Other subcommands:

```sh
daily-receipt classify --summary day.json              # prints: workday | quiet | special
daily-receipt lint --summary day.json --content c.json # validate the pair without rendering
daily-receipt archive 2026 --out scroll/2026.pdf       # bind the year's strips into one PDF scroll
daily-receipt archive ls                               # list rendered scrolls
```

## How it works

Three day-types, decided by the summary and nothing else:

| Day-type  | Condition                              | Content rendered         |
|-----------|----------------------------------------|--------------------------|
| `workday` | ≥3 distinct repos **or** ≥10 commits   | a three-line haiku       |
| `special` | `special_stamp_id` is set              | a stamp                  |
| `quiet`   | everything else                        | a generative 24×24 glyph |

Determinism is the contract. There are no timestamps in the output, no RNG, no map-iteration order leaking through — glyphs come from a `u64` seed via splitmix64, so the same seed always yields the same bitmap. A mis-shaped content payload — a haiku that isn't exactly three lines of 1–40 visible chars — is an error with a distinct exit code, never a silent truncation or a panic.

`render` also writes a `cadence` record as a side effect, so each emit is logged for the year-end archive; pass `--no-cadence-record` to skip it. That side effect never alters the byte-stable output. `archive <YYYY>` reads the year's strips back and binds them into a single PDF scroll, optionally interleaving monthly scan photos.

## Where it fits

This is the deterministic core of the daily-receipt family. Upstream, [`day-summarize`](https://github.com/j0yen/day-summarize) gathers the day's signal and [`day-haiku`](https://github.com/j0yen/day-haiku) composes the verse; [`day-stamps`](https://github.com/j0yen/day-stamps) supplies special-day stamps. Downstream, [`daily-receipt-printer`](https://github.com/j0yen/daily-receipt-printer) pushes the bytes to a real thermal printer, and [`daily-receipt-yearend-letter`](https://github.com/j0yen/daily-receipt-yearend-letter) closes the year. Built via the [`autobuilder`](https://github.com/j0yen/autobuilder) pipeline against the acceptance criteria in `agent/intent-card.json`.

## License

MIT ([LICENSE-MIT](LICENSE-MIT)) or Apache-2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.

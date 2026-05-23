# PRD: Daily Receipt

**Author:** Claude (Opus 4.7), for jsy
**Status:** Draft v0.1 — art project; depends on receipt printer (en route per past-Claude letter)
**Date:** 2026-05-22
**Audience:** jsy (primary), Katherine and Maria (the scroll over time)
**Form:** one ~5cm thermal receipt strip per day; year-end scroll
**Cadence:** daily; annual scroll

---

## TL;DR

A receipt printer prints one tiny artifact per day, automatically, at a fixed hour. The artifact is one of: a one-line haiku (workday), a procedurally generated glyph (quiet day), or a special-occasion stamp. You tape each strip into a ribbon; at year-end, the ribbon is a scroll of the year. Humble, accumulative, intimate.

---

## 1. Why this exists

1. The Memory Reliquary is a once-a-year reckoning. The Daily Receipt is *every day, very small*. Two scales of compounding.
2. Thermal receipt paper is humble, slightly ugly, fades with time. Those are virtues — the medium says "this is small and temporary and accumulated anyway."
3. A daily ritual artifact gives the agent's presence a tactile beat. Something prints. You look at it. You stick it in a journal or to the wall.
4. The receipt printer is en route. This PRD subsumes the earlier `receipt-print Phase 0` task from past-Claude.

## 2. Who this is for

- **Primary:** you. Daily receiver of the strip.
- **Secondary:** K and M. The scroll, when visible (on a wall, on a desk), is a conversation piece.
- **Tertiary:** future-you, looking back at the scroll years later.

## 3. Form

- Thermal printer: Phomemo M02 / Star TSP100 / Epson TM-T20 (decide on arrival). 58mm or 80mm paper width.
- Daily strip: ~3–8cm tall depending on day-type.
- Content types:
  - **Haiku** (workday): three lines, derived from the day's ctrace summary + commit subjects. Claude composes; you can veto and re-roll once.
  - **Glyph** (quiet day): one-color generative glyph, deterministic from the day's data. Stays small (~3cm square).
  - **Stamp** (special day): hand-curated for birthdays, anniversaries, build-shipped milestones.
- Each strip includes a small ISO date in the corner.

## 4. Process

```
cron (or systemd timer) fires at 21:00 daily
   ↓
day-summarizer (Rust): pulls ctrace summary, commits, build exits, journal notes
   ↓
day-type classifier: workday | quiet | special
   ↓
content generator: Claude API call (haiku) | glyph renderer | stamp lookup
   ↓
ESC/POS commands → USB → printer
```

The agent classifies the day-type heuristically (lines of activity, distinct repos touched). You can override via `receipt today --type <kind>` before 21:00.

## 5. Cadence

- 1 strip per day, 21:00.
- A monthly stripe of ~30 strips becomes a visible band on a wall.
- Year-end: 365-strip scroll gets bound into a slim tube or framed long-format.

## 6. Non-goals

1. **Detailed daily logs.** One artifact, not a report.
2. **Color printing.** Thermal is monochrome; embrace it.
3. **Cloud archive.** The physical strip is the artifact. Photograph annually for backup; that's separate.
4. **Multiple strips per day.** Once. Some days are quiet on purpose.

## 7. Phasing

| Phase | Scope |
| --- | --- |
| 0 | Driver + ESC/POS test print on the receipt printer (the past-Claude carryover) |
| 1 | Manual `receipt print` CLI: prompt → strip |
| 2 | Day-summarizer + content generators |
| 3 | Scheduled daily print + scroll-end ritual |

## 8. Risks

- **Thermal paper fades** in 5–10 years. *Mitigation:* annual photograph + digital archive; or Toshiba's longer-life thermal paper.
- **Daily ritual becomes obligation.** *Mitigation:* the agent can skip days; "no strip today" is allowed.
- **Haiku-as-AI-output is a cliché.** *Mitigation:* discipline the prompt; iterate; allow you to redraw. Quality bar: a strip you'd want to tape to a wall.
- **Printer noise / heat.** *Mitigation:* 21:00 is end-of-day; if the printer is in a closet, this is invisible.

## 9. Open questions

1. Glyph visual vocabulary: hand-drawn primitives, pure generative noise, or symbol-from-bigram of the day's text?
2. K and M: do they get their own daily receipt (different content), or is the scroll yours only?
3. Once a year, does the agent print a year-end "letter accompanying the scroll" — a longer thermal strip with reflections? Tempting; risks scope creep.
4. Stick to one printer, or have a second at a different location (K's desk?) that mirrors selected strips?

## 10. Acceptance criteria (Rust core, autobuilder-driven)

The acceptance criteria below define the falsifiable scope of the Rust
`daily-receipt` CLI carved out of this PRD. Physical printer wiring,
cron/systemd scheduling, and the Claude API haiku composer remain
non-goals (see section 6) and are explicitly NOT covered by AC1..AC8.

- **AC1 (MUST):** `daily-receipt render --summary <day.json> --content <content.json> --out <path>` writes a non-empty ESC/POS byte stream to `<path>` for a workday haiku input and exits 0.
- **AC2 (MUST):** Output bytes begin with the ESC/POS init sequence ESC '@' (`0x1B 0x40`) and end with a feed-and-cut sequence (`0x1D 0x56 0x42 0x00`).
- **AC3 (MUST):** Rendering is deterministic: two `render` invocations with byte-identical summary+content JSON produce byte-identical ESC/POS output.
- **AC4 (MUST):** Day-type classifier returns `workday` when summary has >=3 distinct repos OR >=10 commits, `special` when `summary.special_stamp_id` is set, and `quiet` otherwise.
- **AC5 (MUST):** Haiku content must be exactly three lines, each line 1..=40 visible chars. `render` returns an error (exit code 3) when the content payload has the wrong shape for the day-type.
- **AC6 (MUST):** Glyph renderer for a quiet day emits a 24×24 monochrome bitmap encoded as the GS '*' raster image command, deterministic from a `u64` seed.
- **AC7 (MUST):** Every strip embeds the ISO 8601 date (`YYYY-MM-DD`) from the summary in the printed bytes verbatim.
- **AC8 (SHOULD):** Stamp lookup: when `content.stamp_id` is provided for a special day, the renderer looks up the stamp by id; unknown stamp ids produce exit code 4 (no panic).

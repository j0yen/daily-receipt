//! daily-receipt — deterministic ESC/POS renderer for the Daily Receipt
//! art project.
//!
//! The crate exposes a CLI plus a small library surface. The CLI is the
//! contract; the library exists so the same logic can be tested without
//! shelling out and so a future Rust-side scheduler can embed it.
//!
//! See `agent/intent-card.json` for the eight acceptance criteria this
//! crate is built against. The short version:
//!
//! * `classify(summary)` returns one of three [`DayType`]s.
//! * [`render`] produces a byte-stable ESC/POS stream for a strip.
//! * The stream always opens with the ESC/POS init (`0x1B 0x40`) and
//!   ends with the partial-cut command (`0x1D 0x56 0x42 0x00`).
//! * Glyphs are deterministic 24x24 bitmaps generated from a `u64`
//!   seed via splitmix64; same seed → identical bytes.
//! * The summary's date is embedded as ASCII bytes verbatim.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod archive;

use serde::{Deserialize, Serialize};

/// The shape of the daily summary the renderer consumes.
///
/// `commits` lists commit subjects across all repos touched in the day.
/// `repos` is the distinct set of repository slugs that produced those
/// commits. Both are independent because a single-repo day with 20 commits
/// should still classify as a workday.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaySummary {
    /// ISO 8601 calendar date (`YYYY-MM-DD`). Embedded verbatim in output.
    pub date: String,
    /// Distinct repository slugs touched today.
    pub repos: Vec<String>,
    /// Commit subjects, one per commit.
    pub commits: Vec<String>,
    /// Optional special stamp identifier; if `Some`, the day is special.
    #[serde(default)]
    pub special_stamp_id: Option<String>,
    /// Optional free-form journal note. Currently unused by the renderer
    /// but reserved for future content generators.
    #[serde(default)]
    pub journal_note: Option<String>,
}

/// The three day-types the renderer knows how to print.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DayType {
    /// High-activity day — print a haiku strip.
    Workday,
    /// Low-activity day — print a generative glyph strip.
    Quiet,
    /// Marked day (birthday, milestone) — print a stamp strip.
    Special,
}

/// Content payload supplied alongside the summary. Exactly one variant
/// matches each `DayType`; mismatches produce a [`RenderError::ContentShape`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Content {
    /// Three-line haiku. Each line must be 1..=40 visible chars.
    Haiku {
        /// The three lines of the haiku, in order.
        lines: [String; 3],
    },
    /// Glyph seed for a deterministic 24x24 bitmap.
    Glyph {
        /// PRNG seed; the bitmap is a pure function of this value.
        seed: u64,
    },
    /// Stamp identifier to look up in the embedded stamp table.
    Stamp {
        /// Identifier of the stamp to print.
        id: String,
    },
}

/// Errors the renderer can return. Each maps to a distinct CLI exit code.
#[derive(Debug)]
pub enum RenderError {
    /// Content variant does not match the day-type. Exit code 3.
    ContentShape(String),
    /// Haiku line failed the 1..=40 visible-char constraint. Exit code 3.
    HaikuShape(String),
    /// Stamp id was not found in the embedded table. Exit code 4.
    UnknownStamp(String),
    /// Summary date was not a 10-char `YYYY-MM-DD`. Exit code 5.
    DateShape(String),
}

impl core::fmt::Display for RenderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ContentShape(s) => write!(f, "content shape: {s}"),
            Self::HaikuShape(s) => write!(f, "haiku shape: {s}"),
            Self::UnknownStamp(s) => write!(f, "unknown stamp: {s}"),
            Self::DateShape(s) => write!(f, "date shape: {s}"),
        }
    }
}

impl std::error::Error for RenderError {}

impl RenderError {
    /// Exit code mapping for the CLI.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::ContentShape(_) | Self::HaikuShape(_) => 3,
            Self::UnknownStamp(_) => 4,
            Self::DateShape(_) => 5,
        }
    }
}

/// Classify a day as workday / quiet / special by inspecting the summary.
///
/// Rules (see AC4):
/// * `special_stamp_id` set → [`DayType::Special`].
/// * Distinct repos >= 3 OR commits >= 10 → [`DayType::Workday`].
/// * Otherwise → [`DayType::Quiet`].
#[must_use]
pub fn classify(summary: &DaySummary) -> DayType {
    if summary.special_stamp_id.is_some() {
        return DayType::Special;
    }
    let distinct_repos = {
        let mut sorted: Vec<&str> = summary.repos.iter().map(String::as_str).collect();
        sorted.sort_unstable();
        sorted.dedup();
        sorted.len()
    };
    if distinct_repos >= 3 || summary.commits.len() >= 10 {
        return DayType::Workday;
    }
    DayType::Quiet
}

// --- ESC/POS byte primitives ---------------------------------------------
//
// All ESC/POS commands we emit are explicit byte arrays. No vendor-specific
// extensions. The subset chosen overlaps Phomemo M02 / Star TSP100 / Epson
// TM-T20; see intent-card ambiguities_resolved.

const ESC_INIT: [u8; 2] = [0x1B, 0x40]; // ESC '@' — reset to default state.
const LF: u8 = 0x0A;
const FEED_AND_CUT: [u8; 4] = [0x1D, 0x56, 0x42, 0x00]; // GS V B 0 — partial cut after feed.
const ESC_ALIGN_CENTER: [u8; 3] = [0x1B, 0x61, 0x01]; // ESC a 1.
const ESC_ALIGN_LEFT: [u8; 3] = [0x1B, 0x61, 0x00];

const HAIKU_LINE_MIN: usize = 1;
const HAIKU_LINE_MAX: usize = 40;
const GLYPH_SIZE: usize = 24; // 24x24 — one byte-row is 24/8 = 3 bytes wide.
const GLYPH_ROW_BYTES: usize = GLYPH_SIZE / 8;

/// Render a strip's ESC/POS bytes from a summary + content.
///
/// The output is deterministic in its inputs. No clock, no RNG except
/// the splitmix64 seeded from `Content::Glyph::seed`.
///
/// # Errors
///
/// Returns [`RenderError`] if the content shape does not match the day's
/// classification, if a haiku line is empty / overlong, if a stamp id is
/// unknown, or if the date is not a 10-char `YYYY-MM-DD`.
pub fn render(summary: &DaySummary, content: &Content) -> Result<Vec<u8>, RenderError> {
    validate_date(&summary.date)?;

    let day_type = classify(summary);

    let mut out = Vec::with_capacity(256);
    out.extend_from_slice(&ESC_INIT);

    match (day_type, content) {
        (DayType::Workday, Content::Haiku { lines }) => {
            write_haiku(&mut out, lines)?;
        }
        (DayType::Quiet, Content::Glyph { seed }) => {
            write_glyph(&mut out, *seed);
        }
        (DayType::Special, Content::Stamp { id }) => {
            write_stamp(&mut out, id)?;
        }
        (day_type, content) => {
            return Err(RenderError::ContentShape(format!(
                "day-type {day_type:?} does not accept content {}",
                content_kind(content)
            )));
        }
    }

    // Date footer — always present, ASCII-verbatim (AC7).
    out.extend_from_slice(&ESC_ALIGN_LEFT);
    out.push(LF);
    out.extend_from_slice(summary.date.as_bytes());
    out.push(LF);

    // Feed-and-cut tail (AC2).
    out.extend_from_slice(&FEED_AND_CUT);
    Ok(out)
}

const fn content_kind(c: &Content) -> &'static str {
    match c {
        Content::Haiku { .. } => "haiku",
        Content::Glyph { .. } => "glyph",
        Content::Stamp { .. } => "stamp",
    }
}

fn validate_date(date: &str) -> Result<(), RenderError> {
    if date.len() != 10 {
        return Err(RenderError::DateShape(format!(
            "date must be 10 chars (YYYY-MM-DD), got {}",
            date.len()
        )));
    }
    let bytes = date.as_bytes();
    let is_digit = |i: usize| bytes.get(i).is_some_and(u8::is_ascii_digit);
    let is_dash = |i: usize| bytes.get(i) == Some(&b'-');
    if !(is_digit(0)
        && is_digit(1)
        && is_digit(2)
        && is_digit(3)
        && is_dash(4)
        && is_digit(5)
        && is_digit(6)
        && is_dash(7)
        && is_digit(8)
        && is_digit(9))
    {
        return Err(RenderError::DateShape(format!(
            "date '{date}' is not YYYY-MM-DD"
        )));
    }
    Ok(())
}

fn write_haiku(out: &mut Vec<u8>, lines: &[String; 3]) -> Result<(), RenderError> {
    for (i, line) in lines.iter().enumerate() {
        let visible = line.chars().count();
        if !(HAIKU_LINE_MIN..=HAIKU_LINE_MAX).contains(&visible) {
            return Err(RenderError::HaikuShape(format!(
                "haiku line {i} has {visible} chars; allowed {HAIKU_LINE_MIN}..={HAIKU_LINE_MAX}"
            )));
        }
    }
    out.extend_from_slice(&ESC_ALIGN_LEFT);
    for line in lines {
        out.extend_from_slice(line.as_bytes());
        out.push(LF);
    }
    Ok(())
}

fn write_glyph(out: &mut Vec<u8>, seed: u64) {
    // Header: GS '*' n m where n = width-in-bytes (= GLYPH_SIZE / 8 = 3)
    // and m = height-in-8-dot-units (= GLYPH_SIZE / 8 = 3). Payload then
    // follows: n * m * 8 = 72 bytes. We pick exactly this form and stick
    // to it forever; determinism (AC6) requires only that we never
    // silently switch between raster commands.
    let bitmap = generate_glyph_bitmap(seed);
    out.extend_from_slice(&ESC_ALIGN_CENTER);

    // GLYPH_ROW_BYTES and GLYPH_SIZE/8 are both compile-time 3 — fits u8.
    let nx: u8 = u8::try_from(GLYPH_ROW_BYTES).unwrap_or(3);
    let ny: u8 = u8::try_from(GLYPH_SIZE / 8).unwrap_or(3);

    out.extend_from_slice(&[0x1D, 0x2A, nx, ny]);
    out.extend_from_slice(&bitmap);
    out.push(LF);
}

/// Generate the deterministic 24×24 glyph bitmap. Returns 72 bytes
/// (24 rows × 3 bytes wide), MSB-left within each byte.
fn generate_glyph_bitmap(seed: u64) -> [u8; GLYPH_SIZE * GLYPH_ROW_BYTES] {
    let mut state = seed;
    let mut bytes = [0u8; GLYPH_SIZE * GLYPH_ROW_BYTES];
    let mut chunks = bytes.chunks_exact_mut(GLYPH_ROW_BYTES);

    // Splitmix64 — small, deterministic, no external deps.
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };

    // To keep the bitmap visually balanced, mirror horizontally: only
    // generate the left 12 columns and mirror to the right. Folk-symbol
    // feel, still deterministic.
    for row_slot in chunks.by_ref() {
        let r = next();
        // Low 12 bits become the left-half row pattern.
        let left12: u32 = u32::try_from(r & 0x0FFF).unwrap_or(0);
        // Mirror to the right half (also 12 bits).
        let mut mirrored_right: u32 = 0;
        for i in 0..12u32 {
            if left12 & (1u32 << i) != 0 {
                mirrored_right |= 1u32 << (11 - i);
            }
        }
        let full24 = (left12 << 12) | mirrored_right;
        // Pack into 3 bytes, MSB-left (bit 23 → byte 0 MSB).
        let b0 = u8::try_from((full24 >> 16) & 0xFF).unwrap_or(0);
        let b1 = u8::try_from((full24 >> 8) & 0xFF).unwrap_or(0);
        let b2 = u8::try_from(full24 & 0xFF).unwrap_or(0);
        if let [s0, s1, s2] = row_slot {
            *s0 = b0;
            *s1 = b1;
            *s2 = b2;
        }
    }
    bytes
}

fn write_stamp(out: &mut Vec<u8>, id: &str) -> Result<(), RenderError> {
    let stamp = lookup_stamp(id).ok_or_else(|| RenderError::UnknownStamp(id.to_owned()))?;
    out.extend_from_slice(&ESC_ALIGN_CENTER);
    for line in stamp {
        out.extend_from_slice(line.as_bytes());
        out.push(LF);
    }
    Ok(())
}

/// The embedded stamp table. Each entry is rendered as a small block of
/// centered text — punchy and physically distinct from haiku strips.
const STAMPS: &[(&str, &[&str])] = &[
    (
        "birthday",
        &[
            "*** HAPPY BIRTHDAY ***",
            "one more loop around the sun",
        ],
    ),
    (
        "anniversary",
        &[
            "~~~ ANNIVERSARY ~~~",
            "another quiet year, together",
        ],
    ),
    (
        "build-shipped",
        &[
            "### SHIPPED ###",
            "something exists today that did not yesterday",
        ],
    ),
    (
        "new-year",
        &[
            "*** NEW YEAR ***",
            "the scroll begins again",
        ],
    ),
];

fn lookup_stamp(id: &str) -> Option<&'static [&'static str]> {
    STAMPS
        .iter()
        .find_map(|(k, v)| if *k == id { Some(*v) } else { None })
}

/// Convenience: list known stamp ids. Used by the CLI's `lint` and
/// human-friendly error messages.
#[must_use]
pub fn known_stamp_ids() -> Vec<&'static str> {
    STAMPS.iter().map(|(k, _)| *k).collect()
}

/// Validate that a `(summary, content)` pair would render cleanly.
/// Returns `Ok(day_type)` on success.
///
/// # Errors
///
/// Same conditions as [`render`].
pub fn lint(summary: &DaySummary, content: &Content) -> Result<DayType, RenderError> {
    validate_date(&summary.date)?;
    let day_type = classify(summary);
    match (day_type, content) {
        (DayType::Workday, Content::Haiku { lines }) => {
            for (i, line) in lines.iter().enumerate() {
                let visible = line.chars().count();
                if !(HAIKU_LINE_MIN..=HAIKU_LINE_MAX).contains(&visible) {
                    return Err(RenderError::HaikuShape(format!(
                        "haiku line {i} has {visible} chars; allowed {HAIKU_LINE_MIN}..={HAIKU_LINE_MAX}"
                    )));
                }
            }
            Ok(day_type)
        }
        (DayType::Quiet, Content::Glyph { .. }) => Ok(day_type),
        (DayType::Special, Content::Stamp { id }) => {
            if lookup_stamp(id).is_some() {
                Ok(day_type)
            } else {
                Err(RenderError::UnknownStamp(id.clone()))
            }
        }
        (dt, c) => Err(RenderError::ContentShape(format!(
            "day-type {dt:?} does not accept content {}",
            content_kind(c)
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_special_when_stamp_set() {
        let s = DaySummary {
            date: "2026-05-23".into(),
            repos: vec![],
            commits: vec![],
            special_stamp_id: Some("birthday".into()),
            journal_note: None,
        };
        assert_eq!(classify(&s), DayType::Special);
    }

    #[test]
    fn classify_workday_on_repo_breadth() {
        let s = DaySummary {
            date: "2026-05-23".into(),
            repos: vec!["a".into(), "b".into(), "c".into()],
            commits: vec!["x".into()],
            special_stamp_id: None,
            journal_note: None,
        };
        assert_eq!(classify(&s), DayType::Workday);
    }

    #[test]
    fn classify_workday_on_commit_depth() {
        let s = DaySummary {
            date: "2026-05-23".into(),
            repos: vec!["a".into()],
            commits: (0..10).map(|i| format!("c{i}")).collect(),
            special_stamp_id: None,
            journal_note: None,
        };
        assert_eq!(classify(&s), DayType::Workday);
    }

    #[test]
    fn classify_quiet_otherwise() {
        let s = DaySummary {
            date: "2026-05-23".into(),
            repos: vec!["a".into()],
            commits: vec!["c1".into()],
            special_stamp_id: None,
            journal_note: None,
        };
        assert_eq!(classify(&s), DayType::Quiet);
    }

    #[test]
    fn classify_repo_dedup() {
        // 4 repo entries, but only 2 distinct. Should NOT be a workday.
        let s = DaySummary {
            date: "2026-05-23".into(),
            repos: vec!["a".into(), "a".into(), "b".into(), "b".into()],
            commits: vec!["c1".into()],
            special_stamp_id: None,
            journal_note: None,
        };
        assert_eq!(classify(&s), DayType::Quiet);
    }
}

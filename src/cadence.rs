//! Cadence substrate binding.
//!
//! On every successful `render` emit, register the produced artifact as a
//! `daily` record in the cadence substrate by shelling out to the `cadence`
//! CLI (`cadence record daily --produced-by daily-receipt --path <out>
//! --summary <derived>`). Decoupled by design: if `cadence` is not on
//! `$PATH`, emit one warning to stderr and proceed, leaving the byte-stable
//! render output untouched.

// `pub(crate)` is required for the crate-root binary to call into this
// private module; clippy's redundant_pub_crate and rustc's unreachable_pub
// otherwise pull in opposite directions for a binary submodule.
#![allow(clippy::redundant_pub_crate)]

use std::io::Write as _;
use std::path::Path;
use std::process::Command;

use daily_receipt::{DaySummary, DayType};

/// Human-readable lowercase name for a [`DayType`].
const fn day_type_str(day_type: DayType) -> &'static str {
    match day_type {
        DayType::Workday => "workday",
        DayType::Quiet => "quiet",
        DayType::Special => "special",
    }
}

/// Derive a default cadence-record summary from the day summary and its
/// classified day-type. Always begins with the day-type (AC3).
pub(crate) fn derive_summary(summary: &DaySummary, day_type: DayType) -> String {
    let distinct_repos = {
        let mut sorted: Vec<&str> = summary.repos.iter().map(String::as_str).collect();
        sorted.sort_unstable();
        sorted.dedup();
        sorted.len()
    };
    format!(
        "{} {}: {} repo(s), {} commit(s)",
        day_type_str(day_type),
        summary.date,
        distinct_repos,
        summary.commits.len()
    )
}

/// Shell out to `cadence record daily` for the just-emitted artifact.
///
/// Never fails the caller: a missing `cadence` binary or a non-zero exit
/// is reported as a single stderr warning and otherwise ignored, so the
/// byte-stable receipt emit is never blocked by the substrate side effect.
pub(crate) fn record_emit(out: &Path, summary_text: &str) {
    let result = Command::new("cadence")
        .arg("record")
        .arg("daily")
        .arg("--produced-by")
        .arg("daily-receipt")
        .arg("--path")
        .arg(out.as_os_str())
        .arg("--summary")
        .arg(summary_text)
        .status();
    match result {
        Ok(status) if status.success() => {}
        Ok(status) => warn(&format!(
            "cadence record exited {status}; receipt emitted anyway"
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn("cadence not on PATH; skipped cadence record (receipt emitted)");
        }
        Err(e) => warn(&format!(
            "cadence record failed to spawn ({e}); receipt emitted anyway"
        )),
    }
}

/// Emit a single one-line warning to stderr.
fn warn(msg: &str) {
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "daily-receipt: {msg}");
}

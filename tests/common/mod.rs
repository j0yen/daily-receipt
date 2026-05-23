//! Shared helpers for acceptance tests.
//!
//! Lives at tests/common/mod.rs so cargo's integration-test discovery
//! does not pick it up as its own test binary.

#![allow(dead_code)]
#![allow(unreachable_pub)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::missing_const_for_fn,
    clippy::manual_string_new
)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use daily_receipt::{Content, DaySummary};
use serde::Serialize;

/// Path to the freshly built CLI binary.
pub fn cli_bin() -> PathBuf {
    // CARGO_BIN_EXE_<name> is set by cargo for integration tests.
    PathBuf::from(env!("CARGO_BIN_EXE_daily-receipt"))
}

/// Make a unique temporary dir inside CARGO_TARGET_TMPDIR (preferred) or
/// std::env::temp_dir() as a fallback.
pub fn tmp_dir(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let base = option_env!("CARGO_TARGET_TMPDIR")
        .map_or_else(std::env::temp_dir, PathBuf::from);
    let path = base.join(format!("daily-receipt-{prefix}-{pid}-{id}"));
    std::fs::create_dir_all(&path).expect("mkdir tmp");
    path
}

pub fn write_json<T: Serialize>(dir: &Path, name: &str, value: &T) -> PathBuf {
    let path = dir.join(name);
    let s = serde_json::to_string_pretty(value).expect("serialize");
    std::fs::write(&path, s).expect("write json");
    path
}

pub fn workday_summary() -> DaySummary {
    DaySummary {
        date: "2026-05-23".into(),
        repos: vec!["wintermute".into(), "autobuilder".into(), "session-index".into()],
        commits: vec![
            "feat: add classifier".into(),
            "fix: byte order".into(),
            "test: ac3 determinism".into(),
        ],
        special_stamp_id: None,
        journal_note: Some("good day".into()),
    }
}

pub fn quiet_summary() -> DaySummary {
    DaySummary {
        date: "2026-05-23".into(),
        repos: vec!["wintermute".into()],
        commits: vec!["chore: tidy".into()],
        special_stamp_id: None,
        journal_note: None,
    }
}

pub fn special_summary() -> DaySummary {
    DaySummary {
        date: "2026-05-23".into(),
        repos: vec!["wintermute".into()],
        commits: vec![],
        special_stamp_id: Some("birthday".into()),
        journal_note: None,
    }
}

pub fn haiku_content() -> Content {
    Content::Haiku {
        lines: [
            "scrolls of receipts".into(),
            "the printer hums its small song".into(),
            "another day prints".into(),
        ],
    }
}

pub fn glyph_content(seed: u64) -> Content {
    Content::Glyph { seed }
}

pub fn stamp_content(id: &str) -> Content {
    Content::Stamp { id: id.into() }
}

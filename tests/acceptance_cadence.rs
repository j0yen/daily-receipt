//! Acceptance tests for the cadence-bind extension
//! (PRD-cadence-bind-daily-receipt).
//!
//! AC2/AC3/AC4 are hermetic: each points `CADENCE_HOME` at a throwaway
//! dir and drives the real `cadence` CLI, so they neither read nor mutate
//! the user's live substrate. They are environment-gated on `cadence`
//! being on `$PATH` (it is on this machine, where the proof gate runs);
//! absent `cadence` they early-return rather than fail spuriously.
//!
//! AC5 (missing-cadence degrade) is fully self-contained: it spawns the
//! CLI with an empty `$PATH` so the `cadence` lookup fails by construction.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::manual_string_new,
    clippy::missing_panics_doc
)]

mod common;

use std::process::Command;

/// True when the `cadence` CLI is reachable on `$PATH`.
fn cadence_available() -> bool {
    Command::new("cadence")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// AC2 + AC3: a normal render registers exactly one `daily` cadence record
/// whose `path` is the receipt and whose `summary` is non-empty and names
/// the day-type.
#[test]
fn render_emits_cadence_record() {
    if !cadence_available() {
        return; // env-gated: requires `cadence` on PATH
    }
    let dir = common::tmp_dir("cad-rec");
    let cadence_home = dir.join("cadence-home");
    std::fs::create_dir_all(&cadence_home).unwrap();

    let summary = common::workday_summary();
    let content = common::haiku_content();
    let summary_path = common::write_json(&dir, "summary.json", &summary);
    let content_path = common::write_json(&dir, "content.json", &content);
    let out_path = dir.join("strip.escpos");

    let status = Command::new(common::cli_bin())
        .env("CADENCE_HOME", &cadence_home)
        .arg("render")
        .arg("--summary")
        .arg(&summary_path)
        .arg("--content")
        .arg(&content_path)
        .arg("--out")
        .arg(&out_path)
        .status()
        .expect("spawn cli");
    assert!(status.success(), "render exit, got {status:?}");

    let listing = Command::new("cadence")
        .env("CADENCE_HOME", &cadence_home)
        .arg("list")
        .arg("daily")
        .arg("--produced-by")
        .arg("daily-receipt")
        .arg("--since")
        .arg("1h")
        .arg("--json")
        .output()
        .expect("spawn cadence list");
    assert!(listing.status.success(), "cadence list exit");

    let json: serde_json::Value = serde_json::from_slice(&listing.stdout).expect("parse json");
    let arr = json.as_array().expect("expected JSON array");
    assert_eq!(arr.len(), 1, "expected exactly one record, got {arr:?}");

    // AC2: path matches the emitted receipt.
    let recorded_path = arr[0]["path"].as_str().expect("path field");
    assert_eq!(recorded_path, out_path.to_str().unwrap());

    // AC3: summary non-empty and names the day-type (workday for this input).
    let recorded_summary = arr[0]["summary"].as_str().expect("summary field");
    assert!(!recorded_summary.is_empty(), "summary must be non-empty");
    assert!(
        recorded_summary.contains("workday"),
        "summary must name the day-type, got {recorded_summary:?}"
    );
}

/// AC4: `--no-cadence-record` emits the receipt without adding any record.
#[test]
fn no_cadence_record_flag_adds_no_record() {
    if !cadence_available() {
        return; // env-gated
    }
    let dir = common::tmp_dir("cad-norec");
    let cadence_home = dir.join("cadence-home");
    std::fs::create_dir_all(&cadence_home).unwrap();

    let summary = common::workday_summary();
    let content = common::haiku_content();
    let summary_path = common::write_json(&dir, "summary.json", &summary);
    let content_path = common::write_json(&dir, "content.json", &content);
    let out_path = dir.join("strip.escpos");

    let status = Command::new(common::cli_bin())
        .env("CADENCE_HOME", &cadence_home)
        .arg("render")
        .arg("--no-cadence-record")
        .arg("--summary")
        .arg(&summary_path)
        .arg("--content")
        .arg(&content_path)
        .arg("--out")
        .arg(&out_path)
        .status()
        .expect("spawn cli");
    assert!(status.success());
    assert!(!std::fs::read(&out_path).unwrap().is_empty());

    let listing = Command::new("cadence")
        .env("CADENCE_HOME", &cadence_home)
        .arg("list")
        .arg("daily")
        .arg("--produced-by")
        .arg("daily-receipt")
        .arg("--since")
        .arg("1h")
        .arg("--json")
        .output()
        .expect("spawn cadence list");
    assert!(listing.status.success());
    let json: serde_json::Value = serde_json::from_slice(&listing.stdout).expect("parse json");
    assert!(
        json.as_array().expect("array").is_empty(),
        "expected no records with --no-cadence-record, got {json:?}"
    );
}

/// AC1 spirit: the cadence side effect does not alter the byte-stable
/// output — render with and without `--no-cadence-record` is byte-equal.
#[test]
fn cadence_side_effect_is_byte_stable() {
    let dir = common::tmp_dir("cad-bytes");
    let summary = common::workday_summary();
    let content = common::haiku_content();
    let summary_path = common::write_json(&dir, "summary.json", &summary);
    let content_path = common::write_json(&dir, "content.json", &content);

    let out_plain = dir.join("plain.escpos");
    let out_norec = dir.join("norec.escpos");
    let cadence_home = dir.join("cadence-home");
    std::fs::create_dir_all(&cadence_home).unwrap();

    let s1 = Command::new(common::cli_bin())
        .env("CADENCE_HOME", &cadence_home)
        .args(["render", "--summary"])
        .arg(&summary_path)
        .arg("--content")
        .arg(&content_path)
        .arg("--out")
        .arg(&out_plain)
        .status()
        .expect("spawn cli");
    assert!(s1.success());

    let s2 = Command::new(common::cli_bin())
        .args(["render", "--no-cadence-record", "--summary"])
        .arg(&summary_path)
        .arg("--content")
        .arg(&content_path)
        .arg("--out")
        .arg(&out_norec)
        .status()
        .expect("spawn cli");
    assert!(s2.success());

    assert_eq!(
        std::fs::read(&out_plain).unwrap(),
        std::fs::read(&out_norec).unwrap(),
        "cadence record must not change the ESC/POS bytes"
    );
}

/// AC5: with `cadence` unreachable (empty `$PATH`), the render still emits
/// bytes, exits 0, and logs one warning to stderr — no crash.
#[test]
fn missing_cadence_degrades_gracefully() {
    let dir = common::tmp_dir("cad-missing");
    let summary = common::workday_summary();
    let content = common::haiku_content();
    let summary_path = common::write_json(&dir, "summary.json", &summary);
    let content_path = common::write_json(&dir, "content.json", &content);
    let out_path = dir.join("strip.escpos");

    let output = Command::new(common::cli_bin())
        .env("PATH", "") // make `cadence` unresolvable
        .arg("render")
        .arg("--summary")
        .arg(&summary_path)
        .arg("--content")
        .arg(&content_path)
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("spawn cli");

    assert!(output.status.success(), "must still exit 0, got {:?}", output.status);
    assert!(
        !std::fs::read(&out_path).unwrap().is_empty(),
        "bytes must still be emitted"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cadence"),
        "expected a cadence warning on stderr, got {stderr:?}"
    );
}

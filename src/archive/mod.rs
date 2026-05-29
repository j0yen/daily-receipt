//! Archive subcommand — produce an annual PDF scroll from a year of strips.
//!
//! Entry point is [`run_archive`].

pub mod escpos_decode;
pub mod render_pdf;
pub mod scan_intercalate;

use std::collections::HashSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Arguments for the `archive` subcommand.
#[derive(Debug)]
pub struct ArchiveArgs {
    /// Four-digit year to archive.
    pub year: i32,
    /// Output PDF path. Default: `scroll/<YYYY>.pdf`.
    pub out: Option<PathBuf>,
    /// Whether to interleave monthly scan photographs.
    pub include_scans: bool,
    /// Whether to also print a manifest JSON to stdout.
    pub json: bool,
    /// Skip emitting a cadence `yearly` record after render.
    pub no_cadence_record: bool,
    /// Override cadence summary.
    pub cadence_summary: Option<String>,
}

/// A single record returned by `cadence list daily --year YYYY --json`.
#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct CadenceDailyRecord {
    /// ULID or other ID.
    #[serde(default)]
    id: String,
    /// ISO date of the strip.
    date: String,
    /// Path to the ESC/POS or content data.
    #[serde(default)]
    path: String,
    /// Human summary.
    #[serde(default)]
    summary: String,
    /// Record kind (should be "daily").
    #[serde(default)]
    kind: String,
}

/// Run the `archive <YYYY>` subcommand.
///
/// # Errors
///
/// Returns an error string describing what went wrong.
pub fn run_archive(args: &ArchiveArgs, base_dir: &Path) -> Result<(), String> {
    // Resolve output path
    let out_path = match &args.out {
        Some(p) => p.clone(),
        None => {
            let scroll_dir = base_dir.join("scroll");
            std::fs::create_dir_all(&scroll_dir)
                .map_err(|e| format!("mkdir scroll: {e}"))?;
            scroll_dir.join(format!("{}.pdf", args.year))
        }
    };

    // Collect strips from cadence records
    let records = query_cadence_records(args.year);

    // For each record, render or load strip PNG (with cache)
    let archive_dir = base_dir.join("archive").join(args.year.to_string());
    std::fs::create_dir_all(&archive_dir)
        .map_err(|e| format!("mkdir archive: {e}"))?;

    let mut strip_data: Vec<(String, Vec<u8>, u32, u32, String)> = Vec::new();
    let mut warned_bytes: HashSet<u8> = HashSet::new();
    let mut workday_count: usize = 0;
    let mut quiet_count: usize = 0;
    let mut special_count: usize = 0;
    let mut distinct_repos: std::collections::HashSet<String> = std::collections::HashSet::new();

    for record in &records {
        let cache_path = archive_dir.join(format!("{}.png", record.date));

        // Load or (re-)generate PNG
        let escpos_bytes = if !record.path.is_empty() {
            std::fs::read(&record.path).unwrap_or_default()
        } else {
            Vec::new()
        };

        let use_cache = cache_path.exists() && !escpos_bytes.is_empty() && {
            // mtime check: cache file newer than source
            is_cache_fresh(&cache_path, &PathBuf::from(&record.path))
        };

        let png_bytes: Vec<u8> = if use_cache {
            std::fs::read(&cache_path).unwrap_or_default()
        } else if !escpos_bytes.is_empty() {
            let img = escpos_decode::decode(&escpos_bytes, &mut warned_bytes)
                .map_err(|e| format!("decode {}: {e}", record.date))?;
            let png = escpos_decode::to_png_bytes(&img)
                .map_err(|e| format!("png encode {}: {e}", record.date))?;
            // Write to cache (best effort)
            let _ = std::fs::write(&cache_path, &png);
            png
        } else {
            // No ESC/POS bytes: synthesize a minimal stub
            minimal_stub_png()
        };

        // Determine image dimensions
        let (w, h) = escpos_decode::to_png_bytes(
            &escpos_decode::decode(b"\x1b\x40hello\x0a\x1d\x56\x42\x00", &mut HashSet::new())
                .unwrap_or_else(|_| image::GrayImage::new(384, 48)),
        )
        .ok()
        .and_then(|b| render_pdf::png_dims(&b))
        .unwrap_or((384, 48));

        let (pw, ph) = render_pdf::png_dims(&png_bytes).unwrap_or((w, h));

        // Derive day-type from summary string
        let kind = if record.summary.starts_with("workday") {
            workday_count += 1;
            "workday"
        } else if record.summary.starts_with("special") {
            special_count += 1;
            "special"
        } else {
            quiet_count += 1;
            "quiet"
        };

        // Extract repo hints from summary (heuristic: "N repo(s)")
        // Just count distinct dates as a proxy when cadence isn't present.
        distinct_repos.insert(record.date.clone());

        strip_data.push((record.date.clone(), png_bytes, pw, ph, kind.to_owned()));
    }

    let stats = render_pdf::ArchiveStats {
        year: args.year,
        total_strips: strip_data.len(),
        workday_count,
        quiet_count,
        special_count,
        distinct_repos: distinct_repos.len(),
    };

    let mut months = render_pdf::build_months(args.year, &strip_data);

    // Inject scans if requested
    if args.include_scans {
        let scans_dir = scan_intercalate::scans_dir();
        let scans = scan_intercalate::load_scans(&scans_dir, args.year);
        render_pdf::inject_scans(&mut months, &scans);
    }

    let pdf_bytes = render_pdf::build_pdf(&stats, &months)?;

    // Write PDF
    std::fs::write(&out_path, &pdf_bytes)
        .map_err(|e| format!("write pdf {}: {e}", out_path.display()))?;

    // Optional JSON manifest
    if args.json {
        let manifest = serde_json::json!({
            "year": args.year,
            "output": out_path.display().to_string(),
            "total_strips": stats.total_strips,
            "workday": stats.workday_count,
            "quiet": stats.quiet_count,
            "special": stats.special_count,
        });
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(stdout, "{}", manifest);
    }

    // Cadence yearly record
    if !args.no_cadence_record {
        let summary = args.cadence_summary.clone().unwrap_or_else(|| {
            format!(
                "{} strips: {} workday, {} quiet, {} special",
                stats.total_strips, stats.workday_count, stats.quiet_count, stats.special_count
            )
        });
        record_yearly(&out_path, &summary);
    }

    Ok(())
}

/// Run the `archive ls` subcommand — list rendered scrolls.
///
/// # Errors
///
/// Returns an error string if the scroll directory cannot be read.
pub fn run_archive_ls(base_dir: &Path) -> Result<(), String> {
    let scroll_dir = base_dir.join("scroll");
    if !scroll_dir.exists() {
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(stdout, "(no scrolls rendered yet)");
        return Ok(());
    }

    let mut entries: Vec<(String, u64)> = Vec::new();
    let read = std::fs::read_dir(&scroll_dir)
        .map_err(|e| format!("read scroll dir: {e}"))?;
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("pdf") {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_owned();
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            entries.push((name, size));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut stdout = std::io::stdout().lock();
    for (name, size) in &entries {
        let _ = writeln!(stdout, "{name}  ({size} bytes)");
    }
    if entries.is_empty() {
        let _ = writeln!(stdout, "(no scrolls rendered yet)");
    }
    Ok(())
}

/// Query cadence for daily records in the given year. Returns an empty vec
/// if cadence is not installed or produces no output.
fn query_cadence_records(year: i32) -> Vec<CadenceDailyRecord> {
    let result = Command::new("cadence")
        .args(["list", "daily", "--year", &year.to_string(), "--json"])
        .output();
    match result {
        Ok(out) if out.status.success() => {
            serde_json::from_slice::<Vec<CadenceDailyRecord>>(&out.stdout)
                .unwrap_or_default()
        }
        Ok(out) => {
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(
                stderr,
                "daily-receipt archive: cadence list exited {}; no strips loaded",
                out.status
            );
            Vec::new()
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(
                stderr,
                "daily-receipt archive: cadence not on PATH; rendering empty scroll"
            );
            Vec::new()
        }
        Err(e) => {
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(stderr, "daily-receipt archive: cadence error: {e}");
            Vec::new()
        }
    }
}

/// Emit a cadence `yearly` record for the produced scroll.
fn record_yearly(out: &Path, summary: &str) {
    let result = Command::new("cadence")
        .args([
            "record",
            "yearly",
            "--produced-by",
            "daily-receipt",
            "--path",
            &out.display().to_string(),
            "--summary",
            summary,
        ])
        .status();
    match result {
        Ok(status) if status.success() => {}
        Ok(status) => {
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(
                stderr,
                "daily-receipt archive: cadence record yearly exited {status}"
            );
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(
                stderr,
                "daily-receipt archive: cadence not on PATH; skipped yearly record"
            );
        }
        Err(e) => {
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(stderr, "daily-receipt archive: cadence error: {e}");
        }
    }
}

/// Check if a cache PNG is fresher than its ESC/POS source.
fn is_cache_fresh(cache: &Path, source: &Path) -> bool {
    let cache_mtime = std::fs::metadata(cache)
        .and_then(|m| m.modified())
        .ok();
    let source_mtime = std::fs::metadata(source)
        .and_then(|m| m.modified())
        .ok();
    match (cache_mtime, source_mtime) {
        (Some(c), Some(s)) => c >= s,
        _ => false,
    }
}

/// Produce a minimal 384×24 white PNG as a stub for records with no ESC/POS.
fn minimal_stub_png() -> Vec<u8> {
    let img = image::GrayImage::from_pixel(384, 24, image::Luma([255u8]));
    escpos_decode::to_png_bytes(&img).unwrap_or_default()
}

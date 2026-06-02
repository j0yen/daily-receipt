//! Acceptance tests for the `archive` extension
//! (PRD-daily-receipt-archive.md). These are the paired tests the prior
//! /build ticks were missing: archive ACs 1-9 previously had zero coverage.
//!
//! Design notes — these tests deliberately avoid tautological fixtures:
//!
//! * The ESC/POS decoder is fed hand-authored byte streams (not bytes
//!   echoed back from the encoder under test) and the resulting PNGs are
//!   inspected for the *behavioral* properties each AC names (page count,
//!   image-object count, determinism, unknown-byte tolerance).
//! * The PDF is parsed structurally (counting `/Type /Page` and
//!   `/Subtype /Image` objects in the serialized bytes) rather than
//!   compared to a golden blob, so a regression in layout — not just a
//!   byte diff — is what trips the assertion.
//! * CLI-level ACs drive the real binary in a throwaway
//!   `DAILY_RECEIPT_BASE_DIR` so they neither read nor mutate the user's
//!   live scroll/ and archive/ trees, and pass `--no-cadence-record` so
//!   they do not depend on `cadence` being on `$PATH`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::manual_string_new,
    clippy::missing_panics_doc
)]

mod common;

use std::collections::HashSet;
use std::process::Command;

use daily_receipt::archive::escpos_decode::{decode, to_png_bytes};
use daily_receipt::archive::render_pdf::{
    build_months, build_pdf, inject_scans, png_dims, ArchiveStats, StripTuple,
};

/// Count occurrences of a byte subsequence — used to count PDF object kinds
/// in the serialized output. Independent of the writer's internal API.
fn count_subslices(hay: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || hay.len() < needle.len() {
        return 0;
    }
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

/// Build a small grayscale strip PNG from a hand-written ESC/POS stream so
/// the strip data fed to the PDF builder is *real decoder output*, not a
/// canned blob.
fn strip_png(text: &str) -> (Vec<u8>, u32, u32) {
    let mut bytes = vec![0x1B, 0x40]; // ESC @ init
    bytes.extend_from_slice(text.as_bytes());
    bytes.push(0x0A); // LF
    bytes.extend_from_slice(&[0x1D, 0x56, 0x42, 0x00]); // GS V B 0 cut
    let mut warned = HashSet::new();
    let img = decode(&bytes, &mut warned);
    let png = to_png_bytes(&img).expect("encode strip png");
    let (w, h) = png_dims(&png).expect("png dims");
    (png, w, h)
}

/// AC2: regardless of how many days emitted, the PDF has exactly
/// 12 month pages + 1 cover page = 13 pages. Empty months still render.
///
/// Independent-input proof: we feed ZERO strips and assert 13 page objects;
/// then feed strips into only two months and assert it is STILL 13. A
/// tautological test would just trust the builder's claimed count — here we
/// recount `/Type /Page` (not `/Type /Pages`, the tree node) in the bytes.
#[test]
fn ac2_thirteen_pages_regardless_of_strip_count() {
    let stats = ArchiveStats { year: 2026, ..ArchiveStats::default() };

    // Empty year.
    let months = build_months(2026, &[]);
    let pdf = build_pdf(&stats, &months).expect("build empty pdf");
    // "/Type /Page\n" with a trailing delimiter distinguishes a leaf Page
    // object from the "/Type /Pages" tree node. pdf-writer emits
    // "/Type /Page" then a newline/space; "/Type /Pages" has an 's' next.
    let page_objs = count_subslices(&pdf, b"/Type /Page\n")
        + count_subslices(&pdf, b"/Type /Page ")
        + count_subslices(&pdf, b"/Type /Page/")
        + count_subslices(&pdf, b"/Type /Page>");
    assert_eq!(
        page_objs, 13,
        "empty year must still produce 1 cover + 12 month pages = 13, got {page_objs}"
    );

    // Two months populated; the rest empty — still 13 pages.
    let (png, w, h) = strip_png("hello march");
    let (png2, w2, h2) = strip_png("hello july");
    let strips: Vec<StripTuple> = vec![
        ("2026-03-15".into(), png, w, h, "workday".into()),
        ("2026-07-04".into(), png2, w2, h2, "special".into()),
    ];
    let months = build_months(2026, &strips);
    let pdf = build_pdf(&stats, &months).expect("build populated pdf");
    let page_objs = count_subslices(&pdf, b"/Type /Page\n")
        + count_subslices(&pdf, b"/Type /Page ")
        + count_subslices(&pdf, b"/Type /Page/")
        + count_subslices(&pdf, b"/Type /Page>");
    assert_eq!(
        page_objs, 13,
        "two-month year must still produce 13 pages, got {page_objs}"
    );
}

/// AC3: each emitted day's cell contains a strip image. Injecting 3 known
/// records must surface >= 3 image XObjects in the PDF.
///
/// Independent-input proof: three *distinct* strips across three different
/// months; we count `/Subtype /Image` occurrences in the serialized PDF.
#[test]
fn ac3_emitted_days_embed_strip_images() {
    let stats = ArchiveStats {
        year: 2026,
        total_strips: 3,
        workday_count: 2,
        quiet_count: 1,
        special_count: 0,
        distinct_repos: 3,
    };
    let (p1, w1, h1) = strip_png("alpha strip");
    let (p2, w2, h2) = strip_png("beta strip is longer text here");
    let (p3, w3, h3) = strip_png("g");
    let strips: Vec<StripTuple> = vec![
        ("2026-01-10".into(), p1, w1, h1, "workday".into()),
        ("2026-06-20".into(), p2, w2, h2, "workday".into()),
        ("2026-11-30".into(), p3, w3, h3, "quiet".into()),
    ];
    let months = build_months(2026, &strips);
    let pdf = build_pdf(&stats, &months).expect("build pdf");
    let image_objs = count_subslices(&pdf, b"/Subtype /Image");
    assert!(
        image_objs >= 3,
        "expected >= 3 image XObjects for 3 emitted days, got {image_objs}"
    );
}

/// AC4: the ESC/POS decoder is deterministic — byte-identical inputs yield
/// byte-identical PNGs. And, as a non-tautological counterpart, *different*
/// text yields a *different* PNG (so the test would catch a decoder that
/// ignored its input).
#[test]
fn ac4_decoder_is_deterministic_and_input_sensitive() {
    let stream = b"\x1b\x40the quick brown fox\x0a\x1d\x56\x42\x00";

    let mut w1 = HashSet::new();
    let mut w2 = HashSet::new();
    let png_a = to_png_bytes(&decode(stream, &mut w1)).expect("a");
    let png_b = to_png_bytes(&decode(stream, &mut w2)).expect("b");
    assert_eq!(png_a, png_b, "decoder must be byte-deterministic (AC4)");

    // Different alignment command must change the rendered bytes — proves
    // the decoder actually consumes its input rather than emitting a
    // constant. ESC a 2 (right align) vs default left.
    let right = b"\x1b\x40\x1b\x61\x02the quick brown fox\x0a\x1d\x56\x42\x00";
    let mut w3 = HashSet::new();
    let png_c = to_png_bytes(&decode(right, &mut w3)).expect("c");
    assert_ne!(
        png_a, png_c,
        "right-aligned text must differ from left-aligned (decoder ignores input?)"
    );

    // Empty-ish input must still produce a valid, smaller-or-equal PNG and
    // not panic.
    let mut w4 = HashSet::new();
    let png_empty = to_png_bytes(&decode(b"\x1b\x40\x1d\x56\x42\x00", &mut w4)).expect("empty");
    assert!(png_empty.starts_with(b"\x89PNG"), "decoder output must be a PNG");
}

/// AC5: unsupported ESC/POS bytes are skipped with a single deduped stderr
/// warning per unique unknown byte, and the decode does NOT panic.
///
/// Independent-input proof: inject 0x1F (unsupported) twice plus a second
/// unknown 0x07; assert the `warned` set ends up holding exactly those two
/// unique bytes, that valid text around them still renders, and that the
/// stream did not panic. The dedup contract is what the `HashSet` size
/// verifies — feeding the same unknown byte twice must register once.
#[test]
fn ac5_unknown_bytes_skipped_and_deduped_no_panic() {
    // 0x1F appears twice (must dedup to one warning), 0x07 once.
    let stream = b"\x1b\x40ok\x1ftext\x1fmore\x07end\x0a\x1d\x56\x42\x00";
    let mut warned: HashSet<u8> = HashSet::new();
    let img = decode(stream, &mut warned);

    assert!(
        warned.contains(&0x1F),
        "0x1F should be recorded as an unknown byte"
    );
    assert!(
        warned.contains(&0x07),
        "0x07 should be recorded as an unknown byte"
    );
    assert_eq!(
        warned.len(),
        2,
        "exactly two UNIQUE unknown bytes expected (0x1F deduped), got {warned:?}"
    );

    // The valid text around the unknown bytes still rendered: the canvas
    // must contain at least one dark pixel (text was drawn).
    let dark = img.pixels().any(|p| p.0[0] < 128);
    assert!(dark, "valid text should still render despite unknown bytes");

    // And the resulting image still PNG-encodes (no panic, valid output).
    let png = to_png_bytes(&img).expect("png after unknown bytes");
    assert!(png.starts_with(b"\x89PNG"));
}

/// AC6: `inject_scans` interleaves scan images into the matching month, and
/// months with no scan are left untouched. Missing scans must not error.
///
/// Independent-input proof: build 12 empty months, hand a scans map with an
/// entry only for month 5, and assert month 5 (and only month 5) gains a
/// scan page — verified by counting page objects (13 base + 1 scan = 14)
/// and confirming the scan-bearing month is May.
#[test]
fn ac6_inject_scans_interleaves_only_matching_months() {
    let mut months = build_months(2026, &[]);

    // A real (tiny) PNG, produced via the decoder so it is decodable.
    let (scan_png, _w, _h) = strip_png("MAY SCAN");
    let mut scans = std::collections::HashMap::new();
    scans.insert(5u8, scan_png);
    inject_scans(&mut months, &scans);

    let with_scan: Vec<u8> = months
        .iter()
        .filter(|m| m.scan_png.is_some())
        .map(|m| m.month)
        .collect();
    assert_eq!(with_scan, vec![5u8], "only May should have a scan page");

    // Empty scans map: must not error and must add no scan pages.
    let mut months2 = build_months(2026, &[]);
    let empty: std::collections::HashMap<u8, Vec<u8>> = std::collections::HashMap::new();
    inject_scans(&mut months2, &empty);
    assert!(
        months2.iter().all(|m| m.scan_png.is_none()),
        "no scans must mean no scan pages"
    );

    // The scan page surfaces in the rendered PDF: 13 base pages + 1 scan.
    let stats = ArchiveStats { year: 2026, ..ArchiveStats::default() };
    let pdf = build_pdf(&stats, &months).expect("pdf with scan");
    let page_objs = count_subslices(&pdf, b"/Type /Page\n")
        + count_subslices(&pdf, b"/Type /Page ")
        + count_subslices(&pdf, b"/Type /Page/")
        + count_subslices(&pdf, b"/Type /Page>");
    assert_eq!(page_objs, 14, "13 base + 1 scan page expected, got {page_objs}");
}

/// AC1 (CLI): `daily-receipt archive 2026 --out <path>` writes a file whose
/// magic bytes mark it a PDF document (what `file` reports).
///
/// Hermetic: throwaway base dir; `--no-cadence-record` so no `cadence`
/// dependency. The cadence source is empty here, so this also exercises the
/// "empty scroll still renders 13 pages" degrade path end-to-end.
#[test]
fn ac1_cli_writes_pdf_document() {
    let dir = common::tmp_dir("arch-ac1");
    let out = dir.join("2026.pdf");

    let output = Command::new(common::cli_bin())
        .env("PATH", "") // make `cadence` unresolvable: empty year, still a PDF
        .env("DAILY_RECEIPT_BASE_DIR", &dir)
        .arg("archive")
        .arg("2026")
        .arg("--no-cadence-record")
        .arg("--out")
        .arg(&out)
        .output()
        .expect("spawn cli");

    assert!(
        output.status.success(),
        "archive must exit 0, got {:?}; stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes = std::fs::read(&out).expect("read pdf");
    assert!(
        bytes.starts_with(b"%PDF-"),
        "output must begin with the %PDF- magic (file would report 'PDF document')"
    );
    assert!(bytes.len() > 100, "PDF should be non-trivially sized");
}

/// AC9 (CLI): `archive ls` lists rendered scrolls under <base>/scroll/ with
/// year and byte-size. We plant two real PDFs and assert both names and
/// their exact byte sizes appear in stdout — independent of how `ls`
/// computes them (we stat the files ourselves and look for the number).
#[test]
fn ac9_cli_archive_ls_lists_scrolls_with_sizes() {
    let dir = common::tmp_dir("arch-ac9");
    let scroll_dir = dir.join("scroll");
    std::fs::create_dir_all(&scroll_dir).unwrap();

    let p2025 = scroll_dir.join("2025.pdf");
    let p2026 = scroll_dir.join("2026.pdf");
    std::fs::write(&p2025, b"%PDF-1.7\n2025 scroll placeholder\n").unwrap();
    std::fs::write(&p2026, b"%PDF-1.7\n2026 scroll placeholder body longer\n").unwrap();
    let size_2025 = std::fs::metadata(&p2025).unwrap().len();
    let size_2026 = std::fs::metadata(&p2026).unwrap().len();

    let output = Command::new(common::cli_bin())
        .env("DAILY_RECEIPT_BASE_DIR", &dir)
        .arg("archive")
        .arg("ls")
        .output()
        .expect("spawn cli");
    assert!(output.status.success(), "archive ls must exit 0, got {:?}", output.status);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("2025.pdf"), "ls must list 2025.pdf, got: {stdout}");
    assert!(stdout.contains("2026.pdf"), "ls must list 2026.pdf, got: {stdout}");
    assert!(
        stdout.contains(&size_2025.to_string()),
        "ls must report 2025.pdf byte size {size_2025}, got: {stdout}"
    );
    assert!(
        stdout.contains(&size_2026.to_string()),
        "ls must report 2026.pdf byte size {size_2026}, got: {stdout}"
    );
}

/// AC9 edge: `archive ls` against a base dir with no scroll/ directory must
/// not error — it reports an empty listing.
#[test]
fn ac9_cli_archive_ls_empty_is_not_an_error() {
    let dir = common::tmp_dir("arch-ac9-empty");
    let output = Command::new(common::cli_bin())
        .env("DAILY_RECEIPT_BASE_DIR", &dir)
        .arg("archive")
        .arg("ls")
        .output()
        .expect("spawn cli");
    assert!(output.status.success(), "empty ls must exit 0, got {:?}", output.status);
}

/// AC7 (idempotence, library-level surrogate): `build_pdf` over the same
/// stats + months is byte-stable. The full CLI idempotence (skipping cached
/// strip PNGs by mtime) needs cadence-backed records; here we prove the
/// deterministic core the CLI relies on. Re-running the *render* must yield
/// byte-equal PDFs.
#[test]
fn ac7_pdf_build_is_byte_stable() {
    let stats = ArchiveStats {
        year: 2026,
        total_strips: 2,
        workday_count: 1,
        quiet_count: 1,
        special_count: 0,
        distinct_repos: 2,
    };
    let (p1, w1, h1) = strip_png("idempotent one");
    let (p2, w2, h2) = strip_png("idempotent two");
    let strips: Vec<StripTuple> = vec![
        ("2026-02-14".into(), p1, w1, h1, "workday".into()),
        ("2026-08-09".into(), p2, w2, h2, "quiet".into()),
    ];

    let months_a = build_months(2026, &strips);
    let months_b = build_months(2026, &strips);
    let pdf_a = build_pdf(&stats, &months_a).expect("a");
    let pdf_b = build_pdf(&stats, &months_b).expect("b");
    assert_eq!(pdf_a, pdf_b, "re-rendering the same archive must be byte-equal (AC7)");
}

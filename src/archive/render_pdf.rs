//! PDF renderer — produces a portrait A4 PDF from a list of strip PNGs.
//!
//! Layout:
//! * One cover page: year, date range, total counts.
//! * One page per calendar month (12 pages), each with a 7×5 day grid.
//! * An optional scan page interleaved at end of each month (--include-scans).
//!
//! Uses `pdf-writer` directly for a low-dependency, no-unsafe, no-network
//! path. Output is a valid PDF 1.7 document.

use std::collections::HashMap;

use pdf_writer::{Content, Name, Pdf, Rect, Ref};

/// A4 page dimensions in PDF points (1 pt = 1/72 inch).
const PAGE_W: f32 = 595.28;
const PAGE_H: f32 = 841.89;

/// Margins.
const MARGIN: f32 = 36.0;

/// Day-cell dimensions.
#[allow(clippy::float_arithmetic)]
const CELL_W: f32 = (PAGE_W - 2.0 * MARGIN) / 7.0;
#[allow(clippy::float_arithmetic)]
const CELL_H: f32 = (PAGE_H - 120.0 - 2.0 * MARGIN) / 5.0;

/// Strip thumbnail max width inside a cell.
#[allow(clippy::float_arithmetic)]
const THUMB_W: f32 = CELL_W - 4.0;
#[allow(clippy::float_arithmetic)]
const THUMB_H: f32 = CELL_H - 14.0;

/// Default fallback page width in pixels for scan images.
const PAGE_W_PX: u32 = 595;
/// Default fallback page height in pixels for scan images.
const PAGE_H_PX: u32 = 842;

/// Pixel-based PNG source width expected from the decoder (reserved for
/// future validation of incoming strip images).
#[allow(dead_code)]
const PNG_SOURCE_W: u32 = super::escpos_decode::CANVAS_WIDTH;

/// Strip entry type alias to avoid repeating the tuple signature.
pub type StripTuple = (String, Vec<u8>, u32, u32, String);

/// One strip entry passed to the renderer.
#[derive(Debug, Clone)]
pub struct StripEntry {
    /// Calendar day (1-31).
    pub day: u8,
    /// PNG bytes of the decoded strip.
    pub png_bytes: Vec<u8>,
    /// PNG image width in pixels.
    pub width: u32,
    /// PNG image height in pixels.
    pub height: u32,
}

/// One month's worth of strips, plus optional scan image.
#[derive(Debug, Default)]
pub struct MonthData {
    /// 1-based month number.
    pub month: u8,
    /// Year.
    pub year: i32,
    /// Strips with their decoded PNG bytes.
    pub strips: Vec<StripEntry>,
    /// Optional scan PNG bytes (for --include-scans).
    pub scan_png: Option<Vec<u8>>,
}

/// Summary statistics for the cover page.
#[derive(Debug, Default)]
pub struct ArchiveStats {
    /// Four-digit year this archive covers.
    pub year: i32,
    /// Total number of strips rendered.
    pub total_strips: usize,
    /// Number of workday strips.
    pub workday_count: usize,
    /// Number of quiet strips.
    pub quiet_count: usize,
    /// Number of special strips.
    pub special_count: usize,
    /// Number of distinct repositories that contributed.
    pub distinct_repos: usize,
}

/// Build a PDF from cover + 12 month pages.
///
/// # Errors
///
/// Returns an error string if any internal allocation fails (practically
/// unreachable with the current pdf-writer API, but we propagate).
#[allow(clippy::too_many_lines)] // PDF layout requires a wide dispatch
pub fn build_pdf(stats: &ArchiveStats, months: &[MonthData]) -> Result<Vec<u8>, String> {
    let mut alloc = Ref::new(1);
    let catalog_id = alloc.bump();
    let page_tree_id = alloc.bump();

    let mut pdf = Pdf::new();

    // Collect all page IDs upfront: cover + up to 12 month pages + optional scan pages
    let mut page_ids: Vec<Ref> = Vec::new();
    let cover_id = alloc.bump();
    page_ids.push(cover_id);

    // For each month: month-page + optional scan-page
    let mut month_page_ids: Vec<Ref> = Vec::new();
    let mut scan_page_ids: Vec<Option<Ref>> = Vec::new();
    for m in months {
        let mpid = alloc.bump();
        month_page_ids.push(mpid);
        page_ids.push(mpid);
        if m.scan_png.is_some() {
            let spid = alloc.bump();
            scan_page_ids.push(Some(spid));
            page_ids.push(spid);
        } else {
            scan_page_ids.push(None);
        }
    }

    // Catalog
    pdf.catalog(catalog_id).pages(page_tree_id);

    // Page tree
    {
        let mut tree = pdf.pages(page_tree_id);
        tree.kids(page_ids.iter().copied());
        let count = i32::try_from(page_ids.len()).unwrap_or(i32::MAX);
        tree.count(count);
    }

    // Pre-allocate font ref
    let font_id = alloc.bump();

    // --- Cover page ---
    let cover_content_id = alloc.bump();
    {
        let mut page = pdf.page(cover_id);
        let a4 = Rect::new(0.0, 0.0, PAGE_W, PAGE_H);
        page.media_box(a4);
        page.parent(page_tree_id);
        page.contents(cover_content_id);
        let mut resources = page.resources();
        resources.fonts().pair(Name(b"F1"), font_id);
    }
    {
        let mut content = Content::new();
        content.begin_text();
        content.set_font(Name(b"F1"), 28.0);
        #[allow(clippy::float_arithmetic)]
        let title_y = PAGE_H - MARGIN - 30.0;
        content.next_line(MARGIN, title_y);
        let title = format!("Year {} — Daily Receipt Archive", stats.year);
        content.show(pdf_writer::Str(title.as_bytes()));
        content.set_font(Name(b"F1"), 14.0);
        content.next_line(0.0, -40.0);
        let subtitle = format!("{} strips total", stats.total_strips);
        content.show(pdf_writer::Str(subtitle.as_bytes()));
        content.next_line(0.0, -20.0);
        let counts = format!(
            "Workday: {}  Quiet: {}  Special: {}",
            stats.workday_count, stats.quiet_count, stats.special_count
        );
        content.show(pdf_writer::Str(counts.as_bytes()));
        content.next_line(0.0, -20.0);
        let repos = format!("Distinct repos: {}", stats.distinct_repos);
        content.show(pdf_writer::Str(repos.as_bytes()));
        content.next_line(0.0, -40.0);
        let placeholder = b"[Year-end letter placeholder]";
        content.show(pdf_writer::Str(placeholder));
        content.end_text();
        pdf.stream(cover_content_id, &content.finish());
    }

    // --- Month pages ---
    for (mi, month) in months.iter().enumerate() {
        let Some(&mpid) = month_page_ids.get(mi) else { continue };
        let mc_id = alloc.bump();

        // Collect image refs for this month's strips
        let mut strip_img_ids: Vec<Ref> = Vec::new();
        let mut strip_img_allocs: Vec<(Ref, &StripEntry)> = Vec::new();
        for strip in &month.strips {
            let img_id = alloc.bump();
            strip_img_ids.push(img_id);
            strip_img_allocs.push((img_id, strip));
        }

        // Scan image ref
        let scan_img_id: Option<Ref> = if month.scan_png.is_some() {
            Some(alloc.bump())
        } else {
            None
        };

        // Page
        {
            let mut page = pdf.page(mpid);
            page.media_box(Rect::new(0.0, 0.0, PAGE_W, PAGE_H));
            page.parent(page_tree_id);
            page.contents(mc_id);
            let mut resources = page.resources();
            resources.fonts().pair(Name(b"F1"), font_id);
            let mut xobjects = resources.x_objects();
            for (idx, img_id) in strip_img_ids.iter().enumerate() {
                let key = format!("Im{mi}_{idx}");
                xobjects.pair(Name(key.as_bytes()), *img_id);
            }
        }

        // Page content stream
        {
            let month_name = month_name(month.month);
            let header = format!("{} {}", month_name, month.year);

            let mut content = Content::new();
            content.begin_text();
            content.set_font(Name(b"F1"), 16.0);
            #[allow(clippy::float_arithmetic)]
            let header_y = PAGE_H - MARGIN - 20.0;
            content.next_line(MARGIN, header_y);
            content.show(pdf_writer::Str(header.as_bytes()));

            // Day-of-week header
            content.set_font(Name(b"F1"), 8.0);
            let days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
            for (d, &day_name) in days.iter().enumerate() {
                content.next_line(if d == 0 { 0.0 } else { CELL_W }, if d == 0 { -30.0 } else { 0.0 });
                content.show(pdf_writer::Str(day_name.as_bytes()));
            }
            content.end_text();

            // Day cells with date numbers
            content.begin_text();
            content.set_font(Name(b"F1"), 7.0);
            let days_in_month = days_in_month(month.year, month.month);
            for day in 1u8..=days_in_month {
                let (col, row) = day_cell_position(month.year, month.month, day);
                #[allow(clippy::float_arithmetic, clippy::cast_precision_loss)]
                let x = f32::from(col as u8).mul_add(CELL_W, MARGIN) + 2.0;
                #[allow(clippy::float_arithmetic, clippy::cast_precision_loss)]
                let y = (f32::from(row as u8) + 1.0).mul_add(-CELL_H, PAGE_H - MARGIN - 80.0) + 2.0;
                content.next_line(x, y);
                content.show(pdf_writer::Str(day.to_string().as_bytes()));
            }
            content.end_text();

            // Thumbnail images
            let mut strip_by_day: HashMap<u8, usize> = HashMap::new();
            for (idx, strip) in month.strips.iter().enumerate() {
                strip_by_day.insert(strip.day, idx);
            }
            for day in 1u8..=days_in_month {
                if let Some(&idx) = strip_by_day.get(&day) {
                    if let Some(strip) = month.strips.get(idx) {
                        let (col, row) = day_cell_position(month.year, month.month, day);
                        #[allow(clippy::float_arithmetic, clippy::cast_precision_loss)]
                        let x = f32::from(col as u8).mul_add(CELL_W, MARGIN) + 2.0;
                        #[allow(clippy::float_arithmetic, clippy::cast_precision_loss)]
                        let y = (f32::from(row as u8) + 1.0).mul_add(-CELL_H, PAGE_H - MARGIN - 80.0) + 10.0;
                        // Scale to fit within THUMB_W × THUMB_H
                        #[allow(clippy::float_arithmetic, clippy::cast_precision_loss)]
                        let aspect = strip.height as f32 / strip.width.max(1) as f32;
                        let tw = THUMB_W;
                        #[allow(clippy::float_arithmetic)]
                        let th = (tw * aspect).min(THUMB_H);
                        let key = format!("Im{mi}_{idx}");
                        content.transform([tw, 0.0, 0.0, th, x, y]);
                        content.x_object(Name(key.as_bytes()));
                        // Reset transform (inverse)
                        #[allow(clippy::float_arithmetic)]
                        let inv_tw = 1.0 / tw.max(f32::EPSILON);
                        #[allow(clippy::float_arithmetic, clippy::similar_names)]
                        let inv_th = 1.0 / th.max(f32::EPSILON);
                        #[allow(clippy::float_arithmetic)]
                        content.transform([inv_tw, 0.0, 0.0, inv_th, -x * inv_tw, -y * inv_th]);
                    }
                }
            }

            // Footer stats
            content.begin_text();
            content.set_font(Name(b"F1"), 7.0);
            content.next_line(MARGIN, MARGIN);
            let footer = format!("{} strips", month.strips.len());
            content.show(pdf_writer::Str(footer.as_bytes()));
            content.end_text();

            pdf.stream(mc_id, &content.finish());
        }

        // Write strip image XObjects
        for (img_id, strip) in &strip_img_allocs {
            write_png_xobject(&mut pdf, *img_id, &strip.png_bytes, strip.width, strip.height);
        }

        // Scan page (if any)
        if let (Some(spid), Some(scan_bytes)) = (scan_page_ids.get(mi).copied().flatten(), &month.scan_png) {
            let sc_id = alloc.bump();
            let scan_img_ref = scan_img_id.unwrap_or_else(|| alloc.bump());
            {
                let mut page = pdf.page(spid);
                page.media_box(Rect::new(0.0, 0.0, PAGE_W, PAGE_H));
                page.parent(page_tree_id);
                page.contents(sc_id);
                let mut resources = page.resources();
                resources.fonts().pair(Name(b"F1"), font_id);
                let key = format!("Scan{mi}");
                resources.x_objects().pair(Name(key.as_bytes()), scan_img_ref);
            }
            {
                let mut content = Content::new();
                let key = format!("Scan{mi}");
                content.transform([PAGE_W, 0.0, 0.0, PAGE_H, 0.0, 0.0]);
                content.x_object(Name(key.as_bytes()));
                pdf.stream(sc_id, &content.finish());
            }
            // Parse scan PNG dimensions
            let (sw, sh) = png_dimensions(scan_bytes).unwrap_or((PAGE_W_PX, PAGE_H_PX));
            write_png_xobject(&mut pdf, scan_img_ref, scan_bytes, sw, sh);
        }
    }

    // Font resource (standard PDF font — no embedding needed)
    pdf.type1_font(font_id).base_font(Name(b"Helvetica"));

    Ok(pdf.finish())
}

/// Write a grayscale PNG as a PDF image `XObject`.
fn write_png_xobject(
    pdf: &mut Pdf,
    img_id: Ref,
    png_bytes: &[u8],
    width: u32,
    height: u32,
) {
    // For grayscale PNGs we embed raw pixel data decoded from the PNG.
    // pdf-writer needs raw samples; we use the `image` crate to decode.
    use image::ImageDecoder as _;

    let raw_data: Vec<u8> =
        image::codecs::png::PngDecoder::new(std::io::Cursor::new(png_bytes)).map_or_else(
            |_| {
                let len = usize::try_from(width.saturating_mul(height)).unwrap_or(usize::MAX);
                vec![128u8; len]
            },
            |dec| {
                let total =
                    usize::try_from(dec.total_bytes()).unwrap_or(usize::MAX);
                let mut buf = vec![0u8; total];
                if dec.read_image(&mut buf).is_ok() {
                    buf
                } else {
                    let len =
                        usize::try_from(width.saturating_mul(height)).unwrap_or(usize::MAX);
                    vec![128u8; len]
                }
            },
        );

    let mut image = pdf.image_xobject(img_id, &raw_data);
    image.width(i32::try_from(width).unwrap_or(i32::MAX));
    image.height(i32::try_from(height).unwrap_or(i32::MAX));
    image.color_space().device_gray();
    image.bits_per_component(8);
}

/// Parse width/height from PNG header bytes (bytes 16-23 of a valid PNG).
/// Public so the archive module can call it without duplication.
#[must_use]
pub fn png_dims(png_bytes: &[u8]) -> Option<(u32, u32)> {
    png_dimensions(png_bytes)
}

/// Parse width/height from PNG header bytes (bytes 16-23 of a valid PNG).
const fn png_dimensions(png_bytes: &[u8]) -> Option<(u32, u32)> {
    // PNG signature is 8 bytes, then IHDR chunk: 4-byte length, 4-byte "IHDR",
    // then 4-byte width, 4-byte height.
    if png_bytes.len() < 24 {
        return None;
    }
    // Safe: length checked above; indexing into fixed PNG header positions.
    let w = u32::from_be_bytes([png_bytes[16], png_bytes[17], png_bytes[18], png_bytes[19]]);
    let h = u32::from_be_bytes([png_bytes[20], png_bytes[21], png_bytes[22], png_bytes[23]]);
    Some((w, h))
}

/// Return the (col, row) cell position (0-based) for a given day.
/// col is weekday (0=Sun..6=Sat), row is week-of-month (0-4).
const fn day_cell_position(year: i32, month: u8, day: u8) -> (u32, u32) {
    let weekday = weekday_of(year, month, 1);
    // `as u32` safe: day is 1..=31, fits in u32
    #[allow(clippy::as_conversions)]
    let offset = (day as u32).saturating_sub(1) + weekday;
    (offset % 7, offset / 7)
}

/// Compute day of week for the 1st of a month. 0=Sun, 6=Sat.
/// Uses Tomohiko Sakamoto's algorithm.
const fn weekday_of(year: i32, month: u8, day: u8) -> u32 {
    let t: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut y = year;
    // `as i32` safe: month is 1..=12, day is 1..=31, both fit in i32
    #[allow(clippy::as_conversions)]
    let m = month as i32;
    if m < 3 {
        y -= 1;
    }
    #[allow(clippy::as_conversions)]
    let d = day as i32;
    // t[(m-1)] is safe: m is 1..=12, so m-1 is 0..=11, all valid indices.
    #[allow(clippy::indexing_slicing)]
    let result = (y + y / 4 - y / 100 + y / 400 + t[(m - 1) as usize] + d) % 7;
    // rem_euclid on i32 and then cast is safe: result is in 0..=6
    #[allow(clippy::cast_sign_loss, clippy::as_conversions)]
    { result.rem_euclid(7) as u32 }
}

/// Number of days in a given month.
const fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        2 => {
            if is_leap(year) { 29 } else { 28 }
        }
        // 4 | 6 | 9 | 11 and any invalid month both return 30
        _ => 30,
    }
}

/// Gregorian leap year check.
const fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// English month name.
const fn month_name(month: u8) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "Unknown",
    }
}

/// Return all 12 [`MonthData`] stubs for a year, populated with strips.
#[must_use]
pub fn build_months(year: i32, strips: &[StripTuple]) -> Vec<MonthData> {
    let mut months: Vec<MonthData> = (1u8..=12)
        .map(|m| MonthData { month: m, year, strips: Vec::new(), scan_png: None })
        .collect();

    for (date, png_bytes, w, h, _kind) in strips {
        // date is YYYY-MM-DD
        let parts: Vec<&str> = date.splitn(3, '-').collect();
        if parts.len() < 3 {
            continue;
        }
        let Some(m_str) = parts.get(1) else { continue };
        let Some(d_str) = parts.get(2) else { continue };
        let Ok(m) = m_str.parse::<u8>() else { continue };
        let Ok(d) = d_str.parse::<u8>() else { continue };
        if !(1..=12).contains(&m) {
            continue;
        }
        let mi = usize::from(m - 1);
        if let Some(month) = months.get_mut(mi) {
            month.strips.push(StripEntry {
                day: d,
                png_bytes: png_bytes.clone(),
                width: *w,
                height: *h,
            });
        }
    }
    months
}

/// Inject scan PNGs into the months vector.
pub fn inject_scans<S: std::hash::BuildHasher>(
    months: &mut [MonthData],
    scans: &HashMap<u8, Vec<u8>, S>,
) {
    for month in months.iter_mut() {
        if let Some(png) = scans.get(&month.month) {
            month.scan_png = Some(png.clone());
        }
    }
}

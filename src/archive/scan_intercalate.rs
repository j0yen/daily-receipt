//! Scan-intercalation helper — loads monthly phone-photograph PNGs from a
//! configured scan directory.
//!
//! Directory convention: `$DAILY_RECEIPT_SCANS_DIR/<YYYY>/<MM>.{jpg,png}`.
//! Missing months are silently skipped (AC6).

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Load all available scan images for a given year.
///
/// Returns a map from 1-based month number to raw PNG bytes.
/// JPEGs and PNGs are both accepted; JPEGs are converted to PNG in-memory.
/// Missing months or any I/O errors are skipped with a single warning.
#[must_use]
pub fn load_scans(scans_dir: &Path, year: i32) -> HashMap<u8, Vec<u8>> {
    let year_dir = scans_dir.join(year.to_string());
    let mut result: HashMap<u8, Vec<u8>> = HashMap::new();

    for month in 1u8..=12 {
        let mm = format!("{month:02}");
        let candidates: &[&str] = &[".png", ".jpg", ".jpeg"];
        for &ext in candidates {
            let path = year_dir.join(format!("{mm}{ext}"));
            if path.exists() {
                match load_as_png(&path) {
                    Ok(bytes) => {
                        result.insert(month, bytes);
                        break;
                    }
                    Err(e) => {
                        warn(&format!("scan {}: {e}", path.display()));
                    }
                }
            }
        }
    }
    result
}

/// Load any supported image file and return it as PNG bytes.
fn load_as_png(path: &Path) -> Result<Vec<u8>, String> {
    use image::ImageEncoder as _;

    let img = image::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let gray = img.to_luma8();
    let mut buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut buf);
    encoder
        .write_image(gray.as_raw(), gray.width(), gray.height(), image::ExtendedColorType::L8)
        .map_err(|e| format!("encode png {}: {e}", path.display()))?;
    Ok(buf)
}

/// Emit a single one-line warning to stderr.
fn warn(msg: &str) {
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "daily-receipt archive: {msg}");
}

/// Resolve the scans directory from env var or default.
pub fn scans_dir() -> PathBuf {
    std::env::var("DAILY_RECEIPT_SCANS_DIR")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join("wintermute/daily-receipt/scans")
        })
}

//! Minimal ESC/POS decoder — converts the byte stream daily-receipt v0.1
//! emits into a grayscale PNG bitmap.
//!
//! Supported commands:
//! * `ESC @` (0x1B 0x40) — init, reset state.
//! * `ESC a n` (0x1B 0x61 n) — alignment: 0=left, 1=center, 2=right.
//! * `ESC E n` (0x1B 0x45 n) — bold on/off.
//! * `GS * n m` (0x1D 0x2A n m) — define 24×24 raster image.
//! * `GS V B 0` (0x1D 0x56 0x42 0x00) — cut (end of strip).
//! * `LF` (0x0A) — line feed (advance Y by font height).
//! * Printable ASCII bytes (0x20..=0x7E) — rendered as text.
//!
//! Unknown bytes are skipped; each unique unknown byte produces one stderr
//! warning (AC5).

use std::collections::HashSet;
use std::io::Write as _;

use image::{GenericImage as _, GrayImage, Luma};

/// Width of the thermal-paper canvas in pixels (58 mm × 203 DPI ≈ 384 px).
pub const CANVAS_WIDTH: u32 = 384;

/// Height of each text line in pixels (12 px at 203 DPI).
const LINE_HEIGHT: u32 = 12;

/// Width of each character cell in pixels.
const CHAR_WIDTH: u32 = 6;

/// Width of the 24×24 glyph.
const GLYPH_SIZE: u32 = 24;

/// Decoded state built up while walking ESC/POS bytes.
struct Decoder<'a> {
    /// Pixel canvas — white (255) background.
    canvas: GrayImage,
    /// Current cursor Y position (top of current line).
    y: u32,
    /// Current alignment: 0=left, 1=center, 2=right.
    align: u8,
    /// Bold mode on/off.
    bold: bool,
    /// Text accumulator for the current line.
    line_buf: String,
    /// Unknown bytes already warned about (deduped).
    warned: &'a mut HashSet<u8>,
}

impl<'a> Decoder<'a> {
    fn new(warned: &'a mut HashSet<u8>) -> Self {
        let canvas = GrayImage::from_pixel(CANVAS_WIDTH, 4096, Luma([255u8]));
        Self { canvas, y: 0, align: 0, bold: false, line_buf: String::new(), warned }
    }

    /// Flush the current line buffer to the canvas, then advance Y.
    fn flush_line(&mut self) {
        if self.line_buf.is_empty() {
            self.y = self.y.saturating_add(LINE_HEIGHT);
            return;
        }
        let text = std::mem::take(&mut self.line_buf);
        let text_len_u32 =
            u32::try_from(text.len()).unwrap_or(u32::MAX / CHAR_WIDTH);
        let text_width = text_len_u32.saturating_mul(CHAR_WIDTH);
        let x_start = match self.align {
            1 => CANVAS_WIDTH.saturating_sub(text_width) / 2,
            2 => CANVAS_WIDTH.saturating_sub(text_width),
            _ => 0,
        };
        self.render_text(x_start, self.y, &text);
        self.y = self.y.saturating_add(LINE_HEIGHT);
    }

    /// Render ASCII text at pixel position (x, y).
    fn render_text(&mut self, x_start: u32, y: u32, text: &str) {
        let thickness: u32 = if self.bold { 2 } else { 1 };
        let mut x = x_start;
        for ch in text.chars() {
            if x >= CANVAS_WIDTH {
                break;
            }
            if ch == ' ' {
                x = x.saturating_add(CHAR_WIDTH);
                continue;
            }
            // Simple 5×7 font stencil: draw a dark rectangle for each char.
            // Not a real font — just a placeholder visual that distinguishes
            // filled from empty cells, which is all we need for PDF thumbnails.
            let cw = CHAR_WIDTH.min(CANVAS_WIDTH - x);
            let ch_h = LINE_HEIGHT.min(2000);
            for dy in 1..ch_h.saturating_sub(1) {
                for dx in 0..cw.saturating_sub(1) {
                    if dx < thickness || dy < thickness || dy > ch_h - 2 || dx > cw - 2 {
                        let px = x.saturating_add(dx);
                        let py = y.saturating_add(dy);
                        if px < CANVAS_WIDTH && py < self.canvas.height() {
                            self.canvas.put_pixel(px, py, Luma([0u8]));
                        }
                    }
                }
            }
            x = x.saturating_add(CHAR_WIDTH);
        }
    }

    /// Place a 24×24 GS raster bitmap at the current Y position.
    fn place_glyph(&mut self, nx: u8, ny: u8, data: &[u8]) {
        let row_bytes = u32::from(nx);
        let dot_rows = u32::from(ny) * 8;
        let pixel_width = row_bytes * 8;

        let x_start = match self.align {
            1 => CANVAS_WIDTH.saturating_sub(pixel_width) / 2,
            2 => CANVAS_WIDTH.saturating_sub(pixel_width),
            _ => 0,
        };

        for row in 0..dot_rows {
            for byte_idx in 0..row_bytes {
                let data_idx = usize::try_from(row * row_bytes + byte_idx)
                    .unwrap_or(usize::MAX);
                let byte = data.get(data_idx).copied().unwrap_or(0);
                for bit in 0..8u32 {
                    let is_dark = (byte >> (7 - bit)) & 1 == 1;
                    if is_dark {
                        let px = x_start + byte_idx * 8 + bit;
                        let py = self.y + row;
                        if px < CANVAS_WIDTH && py < self.canvas.height() {
                            self.canvas.put_pixel(px, py, Luma([0u8]));
                        }
                    }
                }
            }
        }
        self.y = self.y.saturating_add(dot_rows);
    }

    /// Warn once per unique unknown byte (AC5).
    fn warn_unknown(&mut self, b: u8) {
        if self.warned.insert(b) {
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(stderr, "daily-receipt: escpos_decode: unknown byte 0x{b:02X} — skipped");
        }
    }

    /// Crop the canvas to actual content height and return it.
    fn finish(mut self) -> GrayImage {
        let content_h = self.y.saturating_add(LINE_HEIGHT).max(GLYPH_SIZE);
        let h = content_h.min(self.canvas.height());
        self.canvas.sub_image(0, 0, CANVAS_WIDTH, h).to_image()
    }
}

/// Decode an ESC/POS byte slice emitted by daily-receipt into a grayscale image.
///
/// Returns the decoded image. `cut_found` semantics: the GS V B 0 partial-cut
/// command is consumed but not separately signalled; callers treat the returned
/// image as a complete strip.
///
/// Unknown ESC/POS bytes are skipped with a single stderr warning per unique
/// unknown byte (AC5).
#[allow(clippy::too_many_lines)] // dispatch table is intentionally wide
pub fn decode(bytes: &[u8], warned: &mut HashSet<u8>) -> GrayImage {
    let mut dec = Decoder::new(warned);
    let mut i = 0;
    while i < bytes.len() {
        let Some(&b) = bytes.get(i) else { break };
        match b {
            // LF — flush current line
            0x0A => {
                dec.flush_line();
                i += 1;
            }
            // ESC — look at next byte
            0x1B => {
                let cmd = bytes.get(i + 1).copied();
                match cmd {
                    // ESC @ — init
                    Some(0x40) => {
                        dec.flush_line();
                        i += 2;
                    }
                    // ESC a n — alignment
                    Some(0x61) => {
                        if let Some(&n) = bytes.get(i + 2) {
                            dec.align = n;
                            i += 3;
                        } else {
                            i += 2;
                        }
                    }
                    // ESC E n — bold
                    Some(0x45) => {
                        if let Some(&n) = bytes.get(i + 2) {
                            dec.bold = n != 0;
                            i += 3;
                        } else {
                            i += 2;
                        }
                    }
                    _ => {
                        dec.warn_unknown(b);
                        i += 1;
                    }
                }
            }
            // GS — look at next byte
            0x1D => {
                let cmd = bytes.get(i + 1).copied();
                match cmd {
                    // GS * nx ny data — raster image definition
                    Some(0x2A) => {
                        let nx = bytes.get(i + 2).copied().unwrap_or(0);
                        let ny = bytes.get(i + 3).copied().unwrap_or(0);
                        let data_len = usize::from(nx) * usize::from(ny) * 8;
                        let data_start = i + 4;
                        let data_end = data_start + data_len;
                        let data = if data_end <= bytes.len() {
                            bytes.get(data_start..data_end).unwrap_or(&[])
                        } else {
                            bytes.get(data_start.min(bytes.len())..).unwrap_or(&[])
                        };
                        dec.flush_line();
                        dec.place_glyph(nx, ny, data);
                        i = data_end;
                    }
                    // GS V B 0 — partial cut (strip end)
                    Some(0x56) => {
                        let b2 = bytes.get(i + 2).copied();
                        let b3 = bytes.get(i + 3).copied();
                        if b2 == Some(0x42) && b3 == Some(0x00) {
                            i += 4;
                        } else {
                            dec.warn_unknown(b);
                            i += 1;
                        }
                    }
                    // GS ! n — char size (pass-through, consume 3 bytes)
                    Some(0x21) => {
                        i += 3;
                    }
                    _ => {
                        dec.warn_unknown(b);
                        i += 1;
                    }
                }
            }
            // Printable ASCII
            0x20..=0x7E => {
                dec.line_buf.push(char::from(b));
                i += 1;
            }
            // Everything else
            _ => {
                dec.warn_unknown(b);
                i += 1;
            }
        }
    }
    // Flush any remaining text
    if !dec.line_buf.is_empty() {
        dec.flush_line();
    }
    dec.finish()
}

/// Encode a `GrayImage` to PNG bytes (in-memory).
///
/// # Errors
///
/// Returns an error if PNG encoding fails.
pub fn to_png_bytes(img: &GrayImage) -> Result<Vec<u8>, image::ImageError> {
    use image::ImageEncoder as _;
    let mut buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut buf);
    encoder.write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::L8,
    )?;
    Ok(buf)
}

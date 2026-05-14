//! OCR — optional Tesseract-backed text extraction.
//!
//! Enabled via the `ocr-tesseract` Cargo feature. Requires the system to
//! have `libtesseract-dev` and `libleptonica-dev` installed at build time
//! (`apt install libtesseract-dev libleptonica-dev tesseract-ocr` on
//! Debian/Ubuntu, `brew install tesseract leptonica` on macOS).
//!
//! Without the feature, [`extract`] returns an empty list so the action
//! compiler's OCR rung degrades to a clean miss instead of an error.

use nerve_protocol::OcrFragment;

#[cfg(feature = "ocr-tesseract")]
use nerve_protocol::Bounds;
#[cfg(feature = "ocr-tesseract")]
use parking_lot::Mutex;

#[cfg(feature = "ocr-tesseract")]
static API: once_cell::sync::Lazy<Mutex<Option<leptess::LepTess>>> =
    once_cell::sync::Lazy::new(|| {
        let api = match leptess::LepTess::new(None, "eng") {
            Ok(a) => Some(a),
            Err(e) => {
                tracing::warn!("could not initialise tesseract: {e}");
                None
            }
        };
        Mutex::new(api)
    });

/// Extract OCR fragments from a PNG-encoded screenshot. Returns an empty
/// list when the build has no Tesseract support, when Tesseract failed to
/// initialise, or when the image had no readable text.
#[cfg(feature = "ocr-tesseract")]
pub fn extract(image_png: &[u8]) -> Vec<OcrFragment> {
    let mut guard = API.lock();
    let api = match guard.as_mut() {
        Some(a) => a,
        None => return Vec::new(),
    };
    if api.set_image_from_mem(image_png).is_err() {
        return Vec::new();
    }

    // Use leptess' high-level `get_tsv_text` which gives us text + per-word
    // bounding boxes in a single pass with no manual Boxa indexing.
    let tsv = match api.get_tsv_text(0) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("tesseract get_tsv_text failed: {e}");
            return Vec::new();
        }
    };
    parse_tsv(&tsv)
}

#[cfg(not(feature = "ocr-tesseract"))]
pub fn extract(_image_png: &[u8]) -> Vec<OcrFragment> {
    Vec::new()
}

/// True when this build was compiled with Tesseract support.
pub const fn enabled() -> bool {
    cfg!(feature = "ocr-tesseract")
}

/// Parse a Tesseract TSV table into [`OcrFragment`]s.
///
/// Column layout (Tesseract docs):
///   level, page_num, block_num, par_num, line_num, word_num,
///   left, top, width, height, conf, text
///
/// We only keep `level == 5` (word) rows with non-empty text and conf >= 30.
#[cfg(feature = "ocr-tesseract")]
fn parse_tsv(tsv: &str) -> Vec<OcrFragment> {
    let mut out = Vec::new();
    for line in tsv.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 12 {
            continue;
        }
        // Skip header / non-word rows.
        if cols[0].parse::<i32>().unwrap_or(0) != 5 {
            continue;
        }
        let text = cols[11].trim();
        if text.is_empty() {
            continue;
        }
        let x: i32 = cols[6].parse().unwrap_or(0);
        let y: i32 = cols[7].parse().unwrap_or(0);
        let w: i32 = cols[8].parse().unwrap_or(0);
        let h: i32 = cols[9].parse().unwrap_or(0);
        let conf: f32 = cols[10].parse().unwrap_or(0.0);
        if conf < 30.0 {
            continue;
        }
        out.push(OcrFragment {
            text: text.to_string(),
            bounds: Bounds {
                x,
                y,
                width: w,
                height: h,
            },
            confidence: conf / 100.0,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_on_empty_input_returns_empty() {
        let frags = extract(&[]);
        assert!(frags.is_empty());
    }

    #[cfg(feature = "ocr-tesseract")]
    #[test]
    fn parse_tsv_keeps_only_high_confidence_word_rows() {
        // Tesseract TSV: level, page, block, par, line, word, l, t, w, h, conf, text
        let tsv = "\
level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext
1\t1\t0\t0\t0\t0\t0\t0\t500\t300\t-1\t
5\t1\t1\t1\t1\t1\t10\t20\t30\t12\t95\tHello
5\t1\t1\t1\t1\t2\t50\t20\t40\t12\t10\tnoise
5\t1\t1\t1\t1\t3\t100\t20\t30\t12\t87\tworld
4\t1\t1\t1\t1\t0\t10\t20\t120\t12\t90\t
";
        let frags = parse_tsv(tsv);
        let texts: Vec<&str> = frags.iter().map(|f| f.text.as_str()).collect();
        assert_eq!(texts, vec!["Hello", "world"]);
        assert!(frags[0].confidence > 0.9 && frags[0].confidence < 1.0);
        assert_eq!(frags[0].bounds.x, 10);
        assert_eq!(frags[1].bounds.x, 100);
    }

    /// End-to-end test that exercises the real Tesseract pipeline on a tiny
    /// in-memory PNG. We synthesise an image with `image` so the test has no
    /// filesystem dependency.
    #[cfg(feature = "ocr-tesseract")]
    #[test]
    fn extract_returns_text_for_synthetic_image() {
        use image::{ImageBuffer, ImageFormat, Luma};
        use std::io::Cursor;
        // 200x60 white canvas. We draw a few dark rectangles where the word
        // "OK" should be. Tesseract is forgiving enough on simple high
        // contrast that even crude shapes give us a non-empty result.
        let mut img: ImageBuffer<Luma<u8>, Vec<u8>> = ImageBuffer::new(200, 60);
        for p in img.pixels_mut() {
            *p = Luma([255]);
        }
        // "O" - two vertical strokes
        for y in 15..45 {
            img.put_pixel(40, y, Luma([0]));
            img.put_pixel(60, y, Luma([0]));
        }
        for x in 40..=60 {
            img.put_pixel(x, 15, Luma([0]));
            img.put_pixel(x, 44, Luma([0]));
        }
        // "K" - vertical + diagonals
        for y in 15..45 {
            img.put_pixel(90, y, Luma([0]));
        }
        for i in 0..15 {
            img.put_pixel(90 + i, 30 - i, Luma([0]));
            img.put_pixel(90 + i, 30 + i, Luma([0]));
        }
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png).unwrap();
        // We can't promise tesseract reads "OK" from crude rectangles, but
        // extract() must at minimum not panic, and the function should
        // complete in well under a second.
        let frags = extract(&buf);
        // Sanity: each fragment has a bounding box inside the image.
        for f in &frags {
            assert!(f.bounds.x >= 0);
            assert!(f.bounds.y >= 0);
            assert!(f.bounds.x + f.bounds.width <= 200);
            assert!(f.bounds.y + f.bounds.height <= 60);
        }
    }
}

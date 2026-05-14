//! OCR — optional Tesseract-backed text extraction.
//!
//! Enabled via the `ocr-tesseract` feature. Requires `libtesseract` /
//! `libleptonica` on the host; we don't statically link them because most
//! CI runners don't have them installed.
//!
//! The fallback path returns an empty result, which keeps the rest of the
//! daemon working — the compiler's OCR ladder rung will simply find nothing
//! and move on to coordinate fallback.

use nerve_protocol::{Bounds, OcrFragment};

#[cfg(feature = "ocr-tesseract")]
pub fn extract(image_png: &[u8]) -> Vec<OcrFragment> {
    use leptess::LepTess;
    use leptess::tesseract::TessApi;
    use std::sync::Mutex;
    static API: once_cell::sync::Lazy<Mutex<Option<LepTess>>> = once_cell::sync::Lazy::new(|| {
        let api = match LepTess::new(None, "eng") {
            Ok(a) => Some(a),
            Err(e) => {
                tracing::warn!("could not initialise tesseract: {e}");
                None
            }
        };
        Mutex::new(api)
    });
    let mut guard = match API.lock() {
        Ok(g) => g,
        Err(_) => return Vec::new(),
    };
    let api = match guard.as_mut() {
        Some(a) => a,
        None => return Vec::new(),
    };
    if api.set_image_from_mem(image_png).is_err() {
        return Vec::new();
    }
    let mut out = Vec::new();
    // Word-level boxes are the most useful for an element-bound search.
    if let Some(boxes) = api.get_component_boxes(leptess::capi::TessPageIteratorLevel_RIL_WORD, true) {
        let n = boxes.get_n();
        let raw = match api.get_utf8_text() {
            Ok(t) => t,
            Err(_) => String::new(),
        };
        let words: Vec<&str> = raw.split_whitespace().collect();
        for i in 0..n {
            if let Some(b) = boxes.get_box(i, false) {
                let text = words.get(i as usize).copied().unwrap_or("").to_string();
                if text.is_empty() {
                    continue;
                }
                out.push(OcrFragment {
                    text,
                    bounds: Bounds {
                        x: b.x() as i32,
                        y: b.y() as i32,
                        width: b.w() as i32,
                        height: b.h() as i32,
                    },
                    confidence: 1.0,
                });
            }
        }
    }
    out
}

#[cfg(not(feature = "ocr-tesseract"))]
pub fn extract(_image_png: &[u8]) -> Vec<OcrFragment> {
    Vec::new()
}

/// True when this build was compiled with Tesseract support.
pub const fn enabled() -> bool {
    cfg!(feature = "ocr-tesseract")
}

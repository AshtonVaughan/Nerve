//! Browser-DOM adapter that the action compiler can consult when the active
//! window belongs to a Chromium / Firefox process.
//!
//! Today this is a *capability stub*: it owns the data model and the Compiler
//! integration point. The actual CDP connection is left for a follow-up, but
//! the rest of the runtime is already wired so that, when this module reports
//! `is_browser(app)` and `query_element(...)` returns a hit, the compiler
//! lowers the semantic click to a `js_click` rather than a coordinate.
//!
//! Wiring the real adapter:
//!
//! 1. Discover the browser's `--remote-debugging-port` (Chromium) or open the
//!    Marionette socket (Firefox).
//! 2. Connect a WebSocket to `ws://127.0.0.1:<port>/devtools/browser/<id>`.
//! 3. Call `Target.getTargets` to enumerate tabs.
//! 4. For each tab, `Runtime.evaluate({ expression: ..., returnByValue: true })`
//!    to locate the element via `document.querySelector` or text content.
//! 5. `Input.dispatchMouseEvent` (mousePressed + mouseReleased) for clicks,
//!    `Input.insertText` for typing.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserElement {
    pub frame_url: String,
    pub backend_node_id: i64,
    pub bounds: nerve_protocol::Bounds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserBackend {
    None,
    ChromiumCdp,
    FirefoxMarionette,
    SafariBridge,
}

impl BrowserBackend {
    pub fn detect_from_app(app: &str) -> Self {
        let lower = app.to_ascii_lowercase();
        if lower.contains("chrome") || lower.contains("chromium") || lower.contains("edge") {
            BrowserBackend::ChromiumCdp
        } else if lower.contains("firefox") {
            BrowserBackend::FirefoxMarionette
        } else if lower.contains("safari") {
            BrowserBackend::SafariBridge
        } else {
            BrowserBackend::None
        }
    }
}

/// Front-door for the compiler. The MVP returns `None` so the compiler keeps
/// walking the ladder. Wiring the real adapter is a 100% additive change.
pub async fn query_element(
    _app: &str,
    _selector_or_text: &str,
) -> Option<BrowserElement> {
    None
}

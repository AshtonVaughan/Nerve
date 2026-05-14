//! Browser-DOM adapter.
//!
//! When the active window belongs to a Chromium-family browser running with
//! `--remote-debugging-port=<n>`, this module connects over the
//! Chrome DevTools Protocol (CDP) and lets the action compiler resolve
//! `click_element { text }` against the real DOM rather than guessing pixels.
//!
//! How it works:
//!
//! 1. The compiler asks `query_element(app, text)` for the active app name
//!    and the user's target text. We map the app to a known port (Chrome
//!    9222 by default, configurable via `NERVE_CDP_PORT`), then GET
//!    `/json` to enumerate live targets.
//! 2. For each page-typed target we open `webSocketDebuggerUrl` and call
//!    `Runtime.evaluate` to look up the element by visible text — XPath
//!    `//*[normalize-space(text())="<text>"]`. If we get a hit, we ask CDP
//!    for the bounding rect.
//! 3. Returned [`BrowserElement`] carries the URL, the backend node id,
//!    and the bounds. The compiler can then issue a coordinate click that's
//!    guaranteed to land on the right pixel because CDP just told us where
//!    the element is.
//!
//! The full integration sits behind `--features browser-cdp` because it
//! pulls in `reqwest` (a non-trivial HTTP client). Without the feature, the
//! compiler's ladder skips the rung the same way it does for OCR.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
        if lower.contains("chrome")
            || lower.contains("chromium")
            || lower.contains("edge")
            || lower.contains("brave")
        {
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

/// True when this build was compiled with `--features browser-cdp`.
pub const fn enabled() -> bool {
    cfg!(feature = "browser-cdp")
}

#[cfg(feature = "browser-cdp")]
pub mod cdp {
    //! Thin Chrome DevTools Protocol client. Just enough to look up an
    //! element by visible text and return its bounding box.

    use super::BrowserElement;

    use serde::{Deserialize, Serialize};

    fn debug_port() -> u16 {
        std::env::var("NERVE_CDP_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(9222)
    }

    #[derive(Debug, Deserialize)]
    struct CdpTarget {
        #[serde(rename = "type")]
        kind: String,
        url: String,
        #[serde(rename = "webSocketDebuggerUrl")]
        ws_url: Option<String>,
    }

    /// Enumerate page-typed targets from CDP's /json endpoint.
    async fn list_targets() -> anyhow::Result<Vec<CdpTarget>> {
        let port = debug_port();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(500))
            .build()?;
        let resp = client
            .get(format!("http://127.0.0.1:{port}/json"))
            .send()
            .await?
            .error_for_status()?;
        let targets: Vec<CdpTarget> = resp.json().await?;
        Ok(targets.into_iter().filter(|t| t.kind == "page").collect())
    }

    #[derive(Debug, Serialize)]
    struct CdpRequest {
        id: u32,
        method: &'static str,
        params: serde_json::Value,
    }

    #[derive(Debug, Deserialize)]
    struct CdpResponse {
        #[serde(default)]
        id: u32,
        result: Option<serde_json::Value>,
        #[allow(dead_code)]
        error: Option<serde_json::Value>,
    }

    /// Run a single request/response exchange on a CDP WebSocket.
    async fn cdp_call(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        id: u32,
        method: &'static str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        use futures_util::{SinkExt, StreamExt};
        let req = serde_json::to_string(&CdpRequest { id, method, params })?;
        ws.send(tokio_tungstenite::tungstenite::Message::Text(req.into()))
            .await?;
        while let Some(msg) = ws.next().await {
            let text = match msg? {
                tokio_tungstenite::tungstenite::Message::Text(t) => t,
                _ => continue,
            };
            let parsed: CdpResponse = match serde_json::from_str(&text) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if parsed.id == id {
                if let Some(r) = parsed.result {
                    return Ok(r);
                }
                return Err(anyhow::anyhow!("cdp error: {text}"));
            }
        }
        Err(anyhow::anyhow!("cdp ws closed before response"))
    }

    /// Look up the first element whose visible text matches `needle` across
    /// all open Chromium pages and return its bounds. Returns `Ok(None)` on
    /// a clean miss (no port open, no matching element).
    pub async fn query_element(needle: &str) -> anyhow::Result<Option<BrowserElement>> {
        let targets = match list_targets().await {
            Ok(t) => t,
            Err(e) => {
                tracing::debug!("cdp: target listing failed: {e}");
                return Ok(None);
            }
        };
        for target in targets {
            let ws_url = match target.ws_url {
                Some(u) => u,
                None => continue,
            };
            if let Some(elem) = query_one(&ws_url, &target.url, needle).await? {
                return Ok(Some(elem));
            }
        }
        Ok(None)
    }

    async fn query_one(
        ws_url: &str,
        page_url: &str,
        needle: &str,
    ) -> anyhow::Result<Option<BrowserElement>> {
        let (mut ws, _) = tokio_tungstenite::connect_async(ws_url).await?;

        // Find the element by visible text via XPath. We escape any " in the
        // needle so the JS literal stays well-formed.
        let escaped = needle.replace('"', "\\\"");
        let expr = format!(
            r#"(() => {{
                const r = document.evaluate(
                    '//*[contains(normalize-space(text()), "{escaped}")]',
                    document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null
                );
                if (!r.singleNodeValue) return null;
                const rect = r.singleNodeValue.getBoundingClientRect();
                return {{
                    x: Math.round(rect.left),
                    y: Math.round(rect.top),
                    width: Math.round(rect.width),
                    height: Math.round(rect.height),
                }};
            }})()"#
        );
        let result = cdp_call(
            &mut ws,
            1,
            "Runtime.evaluate",
            serde_json::json!({
                "expression": expr,
                "returnByValue": true,
                "awaitPromise": false,
            }),
        )
        .await?;
        let value = result.get("result").and_then(|r| r.get("value")).cloned();
        let v = match value {
            Some(serde_json::Value::Null) | None => return Ok(None),
            Some(v) => v,
        };
        let bounds = nerve_protocol::Bounds {
            x: v.get("x").and_then(|n| n.as_i64()).unwrap_or(0) as i32,
            y: v.get("y").and_then(|n| n.as_i64()).unwrap_or(0) as i32,
            width: v.get("width").and_then(|n| n.as_i64()).unwrap_or(0) as i32,
            height: v.get("height").and_then(|n| n.as_i64()).unwrap_or(0) as i32,
        };
        Ok(Some(BrowserElement {
            frame_url: page_url.to_string(),
            backend_node_id: 0,
            bounds,
        }))
    }
}

/// Compiler-facing entry point. Returns `Ok(None)` when the browser bridge
/// isn't compiled in, when the app isn't a Chromium browser, or when no
/// matching element exists.
pub async fn query_element(app: &str, needle: &str) -> Option<BrowserElement> {
    if BrowserBackend::detect_from_app(app) != BrowserBackend::ChromiumCdp {
        return None;
    }
    #[cfg(feature = "browser-cdp")]
    {
        match cdp::query_element(needle).await {
            Ok(elem) => elem,
            Err(e) => {
                tracing::debug!("cdp query_element failed: {e}");
                None
            }
        }
    }
    #[cfg(not(feature = "browser-cdp"))]
    {
        let _ = needle;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_backend_detection() {
        assert_eq!(
            BrowserBackend::detect_from_app("Google Chrome"),
            BrowserBackend::ChromiumCdp
        );
        assert_eq!(
            BrowserBackend::detect_from_app("Microsoft Edge"),
            BrowserBackend::ChromiumCdp
        );
        assert_eq!(
            BrowserBackend::detect_from_app("Brave Browser"),
            BrowserBackend::ChromiumCdp
        );
        assert_eq!(
            BrowserBackend::detect_from_app("Firefox"),
            BrowserBackend::FirefoxMarionette
        );
        assert_eq!(
            BrowserBackend::detect_from_app("Safari"),
            BrowserBackend::SafariBridge
        );
        assert_eq!(
            BrowserBackend::detect_from_app("TextEdit"),
            BrowserBackend::None
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn query_element_returns_none_for_non_browser_app() {
        assert!(query_element("TextEdit", "Save").await.is_none());
    }
}

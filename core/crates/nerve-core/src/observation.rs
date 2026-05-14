//! Observation gathering.
//!
//! Combines screen capture, cursor position, active window, and UI tree into a
//! single [`Observation`]. Each component is optional so a slow backend (e.g.
//! Wayland without portals) still produces a usable observation.

use std::sync::Arc;

use chrono::Utc;
use sha2::Digest;
use tracing::warn;

use nerve_protocol::{Observation, Platform, SafetyState, Screen};

use crate::platform::PlatformBackend;
use crate::safety::SafetyEngine;

#[derive(Debug, Clone, Copy, Default)]
pub struct ObserveOpts {
    pub include_screenshot: bool,
    pub include_ui_tree: bool,
    /// Run OCR over the captured screenshot and populate
    /// `Observation.ocr`. Has no effect when the build was compiled without
    /// `--features ocr-tesseract`.
    pub include_ocr: bool,
}

impl ObserveOpts {
    pub const ALL: Self = Self {
        include_screenshot: true,
        include_ui_tree: true,
        include_ocr: true,
    };
    pub const FAST: Self = Self {
        include_screenshot: false,
        include_ui_tree: false,
        include_ocr: false,
    };
}

pub async fn observe(
    backend: &Arc<dyn PlatformBackend>,
    safety: &Arc<SafetyEngine>,
    session_id: &str,
    opts: ObserveOpts,
) -> Observation {
    let mut screen = Screen {
        width: 0,
        height: 0,
        scale_factor: 1.0,
        screenshot_base64: None,
        screenshot_format: "png".to_string(),
        screenshot_hash: None,
        monitors: Vec::new(),
    };

    let capture_timeout = std::time::Duration::from_millis(2500);
    let cap_result = tokio::time::timeout(capture_timeout, backend.capture_primary_screen()).await;
    // Stash the raw PNG bytes if any consumer of this observation (OCR,
    // compiler) needs them. We only keep them in-memory while populating
    // the Observation; they aren't serialised when `include_screenshot=false`.
    let mut raw_png: Option<Vec<u8>> = None;
    if opts.include_screenshot {
        match cap_result {
            Ok(Ok(captured)) => {
                screen.width = captured.width;
                screen.height = captured.height;
                screen.scale_factor = captured.scale_factor;
                let hash = sha256_hex(&captured.png_bytes);
                screen.screenshot_hash = Some(hash);
                use base64::Engine;
                screen.screenshot_base64 =
                    Some(base64::engine::general_purpose::STANDARD.encode(&captured.png_bytes));
                raw_png = Some(captured.png_bytes);
            }
            Ok(Err(e)) => warn!("screen capture failed: {e}"),
            Err(_) => warn!("screen capture timed out"),
        }
    } else if let Ok(Ok(captured)) = cap_result {
        screen.width = captured.width;
        screen.height = captured.height;
        screen.scale_factor = captured.scale_factor;
        screen.screenshot_hash = Some(sha256_hex(&captured.png_bytes));
        if opts.include_ocr {
            raw_png = Some(captured.png_bytes);
        }
    }

    let monitors = tokio::time::timeout(
        std::time::Duration::from_millis(1500),
        backend.monitors(),
    )
    .await
    .ok()
    .and_then(|r| r.ok())
    .unwrap_or_default();
    screen.monitors = monitors;

    let cursor = tokio::time::timeout(
        std::time::Duration::from_millis(1500),
        backend.cursor_position(),
    )
    .await
    .ok()
    .and_then(|r| r.ok())
    .unwrap_or_default();
    let active_window = tokio::time::timeout(
        std::time::Duration::from_millis(1500),
        backend.active_window(),
    )
    .await
    .ok()
    .and_then(|r| r.ok())
    .flatten();
    let ui_tree = if opts.include_ui_tree {
        tokio::time::timeout(
            std::time::Duration::from_millis(2500),
            backend.ui_tree(),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or_default()
    } else {
        vec![]
    };

    // OCR runs on a blocking thread so the 100-500ms tesseract pass cannot
    // stall the daemon's request loop. Skipped silently when the feature is
    // off or when we have no PNG bytes to feed.
    let ocr = if opts.include_ocr && crate::ocr::enabled() {
        match raw_png.as_ref() {
            Some(bytes) => {
                let bytes = bytes.clone();
                let join = tokio::task::spawn_blocking(move || crate::ocr::extract(&bytes));
                match tokio::time::timeout(std::time::Duration::from_secs(5), join).await {
                    Ok(Ok(fragments)) => fragments,
                    Ok(Err(e)) => {
                        warn!("ocr task join error: {e}");
                        vec![]
                    }
                    Err(_) => {
                        warn!("ocr timed out");
                        vec![]
                    }
                }
            }
            None => vec![],
        }
    } else {
        vec![]
    };

    let safety_state = SafetyState {
        agent_active: !safety.is_emergency_stopped() && !safety.policy().human_takeover,
        dry_run: safety.policy().dry_run,
        human_takeover: safety.policy().human_takeover,
        emergency_stopped: safety.is_emergency_stopped(),
        confirmation_required: safety.policy().require_confirmation,
    };

    Observation {
        session_id: session_id.to_string(),
        timestamp: Utc::now(),
        platform: Platform::current(),
        screen,
        cursor,
        active_window,
        ui_tree,
        ocr,
        focused_element: None,
        last_action: None,
        dirty_tiles: vec![],
        visual_diff: None,
        safety_state,
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

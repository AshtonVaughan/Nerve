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
}

impl ObserveOpts {
    pub const ALL: Self = Self { include_screenshot: true, include_ui_tree: true };
    pub const FAST: Self = Self { include_screenshot: false, include_ui_tree: false };
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
    };

    if opts.include_screenshot {
        match backend.capture_primary_screen().await {
            Ok(captured) => {
                screen.width = captured.width;
                screen.height = captured.height;
                screen.scale_factor = captured.scale_factor;
                let hash = sha256_hex(&captured.png_bytes);
                screen.screenshot_hash = Some(hash);
                use base64::Engine;
                screen.screenshot_base64 =
                    Some(base64::engine::general_purpose::STANDARD.encode(&captured.png_bytes));
            }
            Err(e) => warn!("screen capture failed: {e}"),
        }
    } else {
        // We still want width/height even without the pixel payload.
        if let Ok(captured) = backend.capture_primary_screen().await {
            // Avoid copying the bytes payload back.
            screen.width = captured.width;
            screen.height = captured.height;
            screen.scale_factor = captured.scale_factor;
            screen.screenshot_hash = Some(sha256_hex(&captured.png_bytes));
        }
    }

    let cursor = backend.cursor_position().await.unwrap_or_default();
    let active_window = backend.active_window().await.unwrap_or(None);
    let ui_tree = if opts.include_ui_tree {
        backend.ui_tree().await.unwrap_or_default()
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
        ocr: vec![],
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

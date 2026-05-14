//! Observation payloads.
//!
//! An Observation is a structured snapshot of the user's machine produced by
//! the daemon. It can include a screenshot, but the design intentionally pushes
//! agents toward structured signals (windows, accessibility tree, OCR) so the
//! model does not need to re-interpret pixels on every step.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Windows,
    Linux,
    Unknown,
}

impl Default for Platform {
    fn default() -> Self {
        Self::current()
    }
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Platform::Macos
        } else if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "linux") {
            Platform::Linux
        } else {
            Platform::Unknown
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CursorPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Screen {
    pub width: i32,
    pub height: i32,
    pub scale_factor: f32,
    /// Base64-encoded PNG. Omitted when callers ask for `include_screenshot=false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot_base64: Option<String>,
    pub screenshot_format: String,
    /// Hex SHA-256 of the raw screenshot bytes. Lets clients dedupe without
    /// re-decoding the base64 payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot_hash: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActiveWindow {
    pub title: String,
    pub app_name: String,
    pub process_name: String,
    pub pid: Option<u32>,
    pub bounds: Bounds,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiNode {
    pub role: String,
    pub label: Option<String>,
    pub value: Option<String>,
    pub bounds: Option<Bounds>,
    pub enabled: bool,
    pub focused: bool,
    pub children: Vec<UiNode>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OcrFragment {
    pub text: String,
    pub bounds: Bounds,
    pub confidence: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SafetyState {
    pub agent_active: bool,
    pub dry_run: bool,
    pub human_takeover: bool,
    pub emergency_stopped: bool,
    pub confirmation_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
    pub platform: Platform,
    pub screen: Screen,
    pub cursor: CursorPosition,
    pub active_window: Option<ActiveWindow>,
    #[serde(default)]
    pub ui_tree: Vec<UiNode>,
    #[serde(default)]
    pub ocr: Vec<OcrFragment>,
    pub focused_element: Option<UiNode>,
    pub last_action: Option<String>,
    /// Coarse "what changed since last tick" tile bounds. Populated when the
    /// observation was produced for a `delta_frames` subscription.
    #[serde(default)]
    pub dirty_tiles: Vec<Bounds>,
    pub safety_state: SafetyState,
    /// Reserved for future visual diff payloads (e.g. JPEG patches).
    pub visual_diff: Option<serde_json::Value>,
}

/// Capabilities advertised by the daemon. Lets agents and SDKs degrade
/// gracefully when, for example, the accessibility tree is unavailable.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Capabilities {
    pub platform: Platform,
    pub screen_capture: bool,
    pub input_control: bool,
    pub accessibility_tree: bool,
    pub clipboard: bool,
    pub semantic_actions: bool,
    pub ocr: bool,
    pub wayland_limited: bool,
    pub missing_permissions: Vec<String>,
    pub backends: Backends,
    pub version: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Backends {
    pub screen_capture: String,
    pub input: String,
    pub accessibility: String,
    pub clipboard: String,
}

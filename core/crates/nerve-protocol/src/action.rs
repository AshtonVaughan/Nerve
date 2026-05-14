//! Action payloads.
//!
//! Two flavours of action coexist:
//!
//! * [`LowLevelAction`] is a concrete primitive (click x/y, type text).
//! * [`SemanticAction`] is a higher-level intent (click the "Save" button) that
//!   the [`crate::compiler`] in `nerve-core` lowers to a low-level action via
//!   accessibility APIs, OCR, or coordinate fallback.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl Default for MouseButton {
    fn default() -> Self {
        MouseButton::Left
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LowLevelAction {
    /// Hint: return an Observation without performing any action.
    GetObservation { include_screenshot: Option<bool> },
    Screenshot,
    MoveMouse { x: i32, y: i32 },
    Click {
        x: i32,
        y: i32,
        #[serde(default)]
        button: MouseButton,
    },
    DoubleClick { x: i32, y: i32 },
    RightClick { x: i32, y: i32 },
    Drag {
        from_x: i32,
        from_y: i32,
        to_x: i32,
        to_y: i32,
        #[serde(default)]
        button: MouseButton,
    },
    Scroll {
        x: i32,
        y: i32,
        delta_x: i32,
        delta_y: i32,
    },
    TypeText {
        text: String,
        /// milliseconds between keystrokes; None = as fast as the backend allows.
        delay_ms: Option<u64>,
        /// When true, the daemon writes the text to the clipboard and issues
        /// the OS-appropriate paste hotkey (Cmd/Ctrl+V) instead of typing key
        /// by key. This is required for Unicode / CJK / IME-bound text that
        /// the OS keyboard layout cannot synthesise directly. Defaults to
        /// false so callers stay in control of which path runs.
        #[serde(default)]
        unicode_paste: bool,
    },
    KeyPress { key: String },
    Hotkey { keys: Vec<String> },
    ClipboardGet,
    ClipboardSet { text: String },
    Wait { ms: u64 },
    EmergencyStop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementTarget {
    /// Visible text or accessibility label.
    pub text: Option<String>,
    /// Accessibility role, e.g. "button", "menuitem", "checkbox".
    pub role: Option<String>,
    /// App name (matches `active_window.app_name`).
    pub app: Option<String>,
    /// Optional bounding box hint for OCR fallback.
    pub bounds: Option<crate::observation::Bounds>,
    /// Match index when multiple candidates share the same text/role.
    pub index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SemanticAction {
    ClickElement { target: ElementTarget },
    ClickElementByText { text: String, app: Option<String> },
    ClickElementByRole { role: String, app: Option<String> },
    PressButtonNamed { name: String, app: Option<String> },
    FocusWindow { title: Option<String>, app: Option<String> },
    SelectMenuItem { path: Vec<String>, app: Option<String> },
    TypeIntoFocusedElement { text: String },
    FindTextOnScreen { text: String },
    VerifyTextPresent { text: String, timeout_ms: Option<u64> },
    VerifyWindowActive { app: Option<String>, title: Option<String> },
    WaitForText { text: String, timeout_ms: u64 },
    WaitForWindow { app: Option<String>, title: Option<String>, timeout_ms: u64 },
    CloseWindow { app: Option<String>, title: Option<String> },
    OpenApp { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnyAction {
    Low(LowLevelAction),
    Semantic(SemanticAction),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionEnvelope {
    pub id: String,
    pub action: AnyAction,
    /// Optional client-side note that ends up in the audit log.
    pub note: Option<String>,
    /// Idempotency key — if the daemon has seen this same value for the
    /// current session, it returns the cached [`ActionResult`] instead of
    /// executing the action again. Lets SDKs retry on transient errors
    /// without risk of a double click.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// How the daemon ultimately fulfilled an action.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMethod {
    AccessibilityAction,
    NativeUiAction,
    BrowserDomAdapter,
    OcrBoundingBox,
    CoordinateClick,
    Keyboard,
    Clipboard,
    Wait,
    Capture,
    /// Used when the action was rejected by safety, was a dry-run, or didn't
    /// need to touch the OS (e.g. `get_observation`).
    NoOp,
}

/// Diagnostic explaining how a semantic action was lowered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledPlan {
    pub method: ExecutionMethod,
    /// Concrete primitive that ended up being executed. None if the action was
    /// rejected or replaced by a no-op.
    pub primitive: Option<LowLevelAction>,
    /// Ordered list of alternatives tried before settling on `method`.
    #[serde(default)]
    pub attempted: Vec<ExecutionMethod>,
    /// Free-form trace describing how the target was located.
    #[serde(default)]
    pub trace: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub id: String,
    pub ok: bool,
    pub timestamp: DateTime<Utc>,
    pub method: ExecutionMethod,
    pub cursor: Option<crate::observation::CursorPosition>,
    pub active_window: Option<String>,
    pub error: Option<String>,
    /// Optional return payload (e.g. clipboard contents, OCR matches).
    pub data: Option<serde_json::Value>,
    /// Hex SHA-256 of the screenshot taken before this action ran.
    pub screenshot_before: Option<String>,
    /// Hex SHA-256 of the screenshot taken after this action ran.
    pub screenshot_after: Option<String>,
    /// Set when this was a semantic action, describing the lowering decision.
    pub compiled: Option<CompiledPlan>,
}

/// Persistent audit log entry. Mirrors [`ActionResult`] but also carries the
/// inputs so a session can be replayed exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub session_id: String,
    pub action_id: String,
    pub timestamp: DateTime<Utc>,
    pub action: AnyAction,
    pub result: ActionResult,
    pub active_window_before: Option<String>,
    pub active_window_after: Option<String>,
    pub safety_decision: SafetyDecision,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SafetyDecision {
    Allowed,
    DryRun,
    Confirmed,
    Blocked,
    RateLimited,
    EmergencyStopped,
}

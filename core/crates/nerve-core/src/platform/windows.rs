//! Windows platform backend.
//!
//! Delegates to the portable backend for the MVP. Native upgrades:
//!
//! * Screen capture: DXGI Desktop Duplication (`IDXGIOutputDuplication`) or
//!   Windows.Graphics.Capture for low-latency frame deltas.
//! * Window enumeration: `EnumWindows` + `GetWindowText` + `GetWindowThreadProcessId`.
//! * Active window: `GetForegroundWindow`.
//! * UI tree: `UIAutomationCore` via `windows-rs`.
//! * Input: `SendInput` with `INPUT_MOUSE` / `INPUT_KEYBOARD` records.
//! * Integrity / UAC: if the daemon runs at medium IL and the target window is
//!   high IL, `SendInput` silently no-ops. We need to detect and report this.

use async_trait::async_trait;
use std::sync::Arc;

use nerve_protocol::{
    ActiveWindow, Backends, Capabilities, CursorPosition, MouseButton, Platform, UiNode,
};

use crate::errors::Result;

use super::portable::PortableBackend;
use super::{CapturedScreen, PlatformBackend};

pub struct WindowsBackend {
    inner: Arc<PortableBackend>,
}

impl WindowsBackend {
    pub fn new() -> Self { Self { inner: PortableBackend::shared() } }
}

#[async_trait]
impl PlatformBackend for WindowsBackend {
    fn name(&self) -> &'static str { "windows" }
    fn platform(&self) -> Platform { Platform::Windows }
    fn capabilities(&self) -> Capabilities {
        let mut caps = self.inner.capabilities();
        caps.platform = Platform::Windows;
        caps.backends = self.backends();
        // TODO: flip to true once UIAutomation is wired.
        caps.accessibility_tree = false;
        caps
    }
    fn backends(&self) -> Backends {
        Backends {
            screen_capture: "xcap (TODO: DXGI Duplication)".to_string(),
            input: "enigo (TODO: SendInput)".to_string(),
            accessibility: "none (TODO: UIAutomation)".to_string(),
            clipboard: "arboard".to_string(),
        }
    }

    async fn capture_primary_screen(&self) -> Result<CapturedScreen> { self.inner.capture_primary_screen().await }
    async fn cursor_position(&self) -> Result<CursorPosition> { self.inner.cursor_position().await }
    async fn active_window(&self) -> Result<Option<ActiveWindow>> { self.inner.active_window().await }
    async fn ui_tree(&self) -> Result<Vec<UiNode>> { self.inner.ui_tree().await }

    async fn move_mouse(&self, x: i32, y: i32) -> Result<()> { self.inner.move_mouse(x, y).await }
    async fn click(&self, x: i32, y: i32, button: MouseButton) -> Result<()> { self.inner.click(x, y, button).await }
    async fn double_click(&self, x: i32, y: i32) -> Result<()> { self.inner.double_click(x, y).await }
    async fn drag(&self, from: (i32, i32), to: (i32, i32), button: MouseButton) -> Result<()> { self.inner.drag(from, to, button).await }
    async fn scroll(&self, x: i32, y: i32, dx: i32, dy: i32) -> Result<()> { self.inner.scroll(x, y, dx, dy).await }

    async fn type_text(&self, text: &str, delay_ms: Option<u64>) -> Result<()> { self.inner.type_text(text, delay_ms).await }
    async fn key_press(&self, key: &str) -> Result<()> { self.inner.key_press(key).await }
    async fn hotkey(&self, keys: &[String]) -> Result<()> { self.inner.hotkey(keys).await }

    async fn clipboard_get(&self) -> Result<String> { self.inner.clipboard_get().await }
    async fn clipboard_set(&self, text: &str) -> Result<()> { self.inner.clipboard_set(text).await }

    async fn open_app(&self, name: &str) -> Result<()> { self.inner.open_app(name).await }

    async fn missing_permissions(&self) -> Vec<String> {
        // TODO: query GetTokenInformation for integrity level and add a
        // "UIPI: target window runs at higher integrity" hint when applicable.
        self.inner.missing_permissions().await
    }
}

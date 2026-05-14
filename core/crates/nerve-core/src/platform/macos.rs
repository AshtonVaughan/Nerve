//! macOS platform backend.
//!
//! The MVP delegates to the portable backend (`xcap` + `enigo` + `arboard`).
//! Production work should replace each method with a native API call:
//!
//! * Screen capture: `ScreenCaptureKit` (`SCStreamConfiguration`, `SCDisplay`)
//!   - frees us from CGDisplay's polling cost and gives delta-frame streaming.
//! * Window enumeration: `CGWindowListCopyWindowInfo` filtered by `kCGWindowLayer == 0`.
//! * Active window: `NSWorkspace.frontmostApplication` then `AXFocusedUIElement`.
//! * UI tree: `AXUIElementCreateApplication` + recursive `AXUIElementCopyAttributeValues`.
//! * Input: `CGEventCreateMouseEvent` / `CGEventCreateKeyboardEvent`.
//! * Permissions: `CGPreflightScreenCaptureAccess`, `AXIsProcessTrustedWithOptions`.
//!
//! Each of these requires the `objc2` / `core-graphics` / `screencapturekit`
//! crate families and entitlements at app-bundle level, so they're tracked as
//! follow-up work rather than rolled into the MVP.

use async_trait::async_trait;
use std::sync::Arc;

use nerve_protocol::{
    ActiveWindow, Backends, Capabilities, CursorPosition, MouseButton, Platform, UiNode,
};

use crate::errors::Result;

use super::portable::PortableBackend;
use super::{CapturedScreen, PlatformBackend};

pub struct MacosBackend {
    inner: Arc<PortableBackend>,
}

impl MacosBackend {
    pub fn new() -> Self {
        Self { inner: PortableBackend::shared() }
    }
}

#[async_trait]
impl PlatformBackend for MacosBackend {
    fn name(&self) -> &'static str { "macos" }
    fn platform(&self) -> Platform { Platform::Macos }
    fn capabilities(&self) -> Capabilities {
        let mut caps = self.inner.capabilities();
        caps.platform = Platform::Macos;
        caps.backends = self.backends();
        // TODO: flip these to true once we wire ScreenCaptureKit / AX APIs.
        caps.accessibility_tree = false;
        caps
    }
    fn backends(&self) -> Backends {
        Backends {
            screen_capture: "xcap (TODO: ScreenCaptureKit)".to_string(),
            input: "enigo (TODO: CGEvent)".to_string(),
            accessibility: "none (TODO: AXUIElement)".to_string(),
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
        let mut missing = self.inner.missing_permissions().await;
        // TODO: real checks via CGPreflightScreenCaptureAccess and AXIsProcessTrustedWithOptions.
        // For now we surface the canonical names so doctor still reports something useful.
        if missing.contains(&"screen_capture".to_string()) {
            missing.push("Screen Recording (System Settings > Privacy & Security)".into());
        }
        missing.push("Accessibility (System Settings > Privacy & Security)".into());
        missing
    }
}

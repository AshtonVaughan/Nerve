//! Windows platform backend.
//!
//! Most paths still delegate to the portable backend (`xcap` + `enigo` +
//! `arboard`), but the hot ones — Unicode typing, foreground window,
//! integrity-level probe — go through [`super::windows_native`]. UI
//! Automation tree walking is wired up in Tier 2.4 and lives behind the
//! same module.

use async_trait::async_trait;
use std::sync::Arc;

use nerve_protocol::{
    ActiveWindow, Backends, Capabilities, CursorPosition, MouseButton, Platform, UiNode,
};

use crate::errors::{NerveError, Result};

use super::portable::PortableBackend;
use super::{CapturedScreen, PlatformBackend};

pub struct WindowsBackend {
    inner: Arc<PortableBackend>,
}

impl WindowsBackend {
    pub fn new() -> Self {
        Self {
            inner: PortableBackend::shared(),
        }
    }
}

#[async_trait]
impl PlatformBackend for WindowsBackend {
    fn name(&self) -> &'static str {
        "windows"
    }
    fn platform(&self) -> Platform {
        Platform::Windows
    }
    fn capabilities(&self) -> Capabilities {
        let mut caps = self.inner.capabilities();
        caps.platform = Platform::Windows;
        caps.backends = self.backends();
        caps.accessibility_tree = true;
        caps
    }
    fn backends(&self) -> Backends {
        Backends {
            screen_capture: "xcap (DXGI Duplication pending)".to_string(),
            input: "SendInput (Unicode) + enigo (hotkeys)".to_string(),
            accessibility: "UIAutomation".to_string(),
            clipboard: "arboard".to_string(),
        }
    }

    async fn capture_primary_screen(&self) -> Result<CapturedScreen> {
        self.inner.capture_primary_screen().await
    }

    async fn cursor_position(&self) -> Result<CursorPosition> {
        self.inner.cursor_position().await
    }

    async fn active_window(&self) -> Result<Option<ActiveWindow>> {
        // Use the native foreground-window probe; fall back to portable
        // (xcap) if the native call returned None.
        let native = tokio::task::spawn_blocking(super::windows_native::foreground_window)
            .await
            .map_err(|e| NerveError::Backend(format!("active_window join: {e}")))?;
        if native.is_some() {
            return Ok(native);
        }
        self.inner.active_window().await
    }

    async fn ui_tree(&self) -> Result<Vec<UiNode>> {
        // UIA tree walking is COM-heavy; do it on a blocking thread so a
        // misbehaving target app can't block the daemon's tokio worker.
        // The walker has its own depth / count cap.
        let nodes = tokio::task::spawn_blocking(super::windows_native::ui_tree)
            .await
            .map_err(|e| NerveError::Backend(format!("ui_tree join: {e}")))?;
        Ok(nodes)
    }

    async fn move_mouse(&self, x: i32, y: i32) -> Result<()> {
        self.inner.move_mouse(x, y).await
    }
    async fn click(&self, x: i32, y: i32, button: MouseButton) -> Result<()> {
        self.inner.click(x, y, button).await
    }
    async fn double_click(&self, x: i32, y: i32) -> Result<()> {
        self.inner.double_click(x, y).await
    }
    async fn drag(
        &self,
        from: (i32, i32),
        to: (i32, i32),
        button: MouseButton,
    ) -> Result<()> {
        self.inner.drag(from, to, button).await
    }
    async fn scroll(&self, x: i32, y: i32, dx: i32, dy: i32) -> Result<()> {
        self.inner.scroll(x, y, dx, dy).await
    }

    async fn type_text(&self, text: &str, delay_ms: Option<u64>) -> Result<()> {
        // Honour explicit per-character delays via enigo so callers asking
        // for slow / visible typing still get it. Otherwise prefer the
        // native SendInput path because it survives keyboard layouts and
        // IME composition that enigo can drop on Windows.
        if let Some(ms) = delay_ms {
            return self.inner.type_text(text, Some(ms)).await;
        }
        if super::windows_native::target_higher_integrity() {
            return Err(NerveError::PermissionDenied(
                "foreground window has higher integrity (UIPI); input would be silently dropped"
                    .into(),
            ));
        }
        let text_owned = text.to_string();
        let count = tokio::task::spawn_blocking(move || {
            super::windows_native::send_unicode(&text_owned)
        })
        .await
        .map_err(|e| NerveError::Backend(format!("send_unicode join: {e}")))??;
        if count < text.chars().count() {
            return Err(NerveError::Backend(format!(
                "SendInput accepted only {count} of {} characters",
                text.chars().count()
            )));
        }
        Ok(())
    }

    async fn key_press(&self, key: &str) -> Result<()> {
        // Modifier keys / VK-only keys still need enigo's VK path. Native
        // SendInput would require a hand-rolled VK table; not worth it for
        // hotkeys, which is the only thing that hits this path.
        self.inner.key_press(key).await
    }
    async fn hotkey(&self, keys: &[String]) -> Result<()> {
        self.inner.hotkey(keys).await
    }

    async fn clipboard_get(&self) -> Result<String> {
        self.inner.clipboard_get().await
    }
    async fn clipboard_set(&self, text: &str) -> Result<()> {
        self.inner.clipboard_set(text).await
    }

    async fn open_app(&self, name: &str) -> Result<()> {
        self.inner.open_app(name).await
    }

    async fn missing_permissions(&self) -> Vec<String> {
        let mut missing = self.inner.missing_permissions().await;
        if super::windows_native::target_higher_integrity() {
            missing.push(
                "UIPI: foreground window runs at higher integrity than the Nerve daemon"
                    .to_string(),
            );
        }
        missing
    }
}

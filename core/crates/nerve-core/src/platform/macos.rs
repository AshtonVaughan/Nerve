//! macOS platform backend.
//!
//! Screen capture stays on the portable (xcap → CGDisplay) path until the
//! ScreenCaptureKit work lands. Active-window, accessibility tree, mouse
//! clicks, and permission probes go through [`super::macos_native`] when
//! the `macos-accessibility` feature is on, which is the default for
//! native macOS deployments.

use async_trait::async_trait;
use std::sync::Arc;

use nerve_protocol::{
    ActiveWindow, Backends, Capabilities, CursorPosition, MouseButton, Platform, UiNode,
};

use crate::errors::{NerveError, Result};

use super::portable::PortableBackend;
use super::{CapturedScreen, PlatformBackend};

pub struct MacosBackend {
    inner: Arc<PortableBackend>,
}

impl MacosBackend {
    pub fn new() -> Self {
        Self {
            inner: PortableBackend::shared(),
        }
    }
}

#[async_trait]
impl PlatformBackend for MacosBackend {
    fn name(&self) -> &'static str {
        "macos"
    }
    fn platform(&self) -> Platform {
        Platform::Macos
    }
    fn capabilities(&self) -> Capabilities {
        let mut caps = self.inner.capabilities();
        caps.platform = Platform::Macos;
        caps.backends = self.backends();
        #[cfg(feature = "macos-accessibility")]
        {
            caps.accessibility_tree = true;
        }
        caps
    }
    fn backends(&self) -> Backends {
        Backends {
            screen_capture: "xcap (ScreenCaptureKit pending)".to_string(),
            #[cfg(feature = "macos-accessibility")]
            input: "CGEvent (clicks) + enigo (keys)".to_string(),
            #[cfg(not(feature = "macos-accessibility"))]
            input: "enigo (CGEvent fallback)".to_string(),
            #[cfg(feature = "macos-accessibility")]
            accessibility: "AXUIElement".to_string(),
            #[cfg(not(feature = "macos-accessibility"))]
            accessibility: "none (rebuild with --features macos-accessibility)".to_string(),
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
        #[cfg(feature = "macos-accessibility")]
        {
            let native =
                tokio::task::spawn_blocking(super::macos_native::frontmost_app)
                    .await
                    .map_err(|e| NerveError::Backend(format!("active_window join: {e}")))?;
            if native.is_some() {
                return Ok(native);
            }
        }
        self.inner.active_window().await
    }

    async fn ui_tree(&self) -> Result<Vec<UiNode>> {
        #[cfg(feature = "macos-accessibility")]
        {
            let nodes = tokio::task::spawn_blocking(super::macos_native::ax_tree)
                .await
                .map_err(|e| NerveError::Backend(format!("ax_tree join: {e}")))?;
            return Ok(nodes);
        }
        #[allow(unreachable_code)]
        Ok(Vec::new())
    }

    async fn move_mouse(&self, x: i32, y: i32) -> Result<()> {
        self.inner.move_mouse(x, y).await
    }

    async fn click(&self, x: i32, y: i32, button: MouseButton) -> Result<()> {
        #[cfg(feature = "macos-accessibility")]
        {
            let ok = tokio::task::spawn_blocking(move || {
                super::macos_native::cgevent_click(x, y, button)
            })
            .await
            .map_err(|e| NerveError::Backend(format!("cgevent_click join: {e}")))?;
            if ok {
                return Ok(());
            }
        }
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
        self.inner.type_text(text, delay_ms).await
    }
    async fn key_press(&self, key: &str) -> Result<()> {
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
        #[cfg(feature = "macos-accessibility")]
        {
            if !super::macos_native::screen_recording_granted() {
                missing.push(
                    "Screen Recording (System Settings > Privacy & Security > Screen Recording)"
                        .into(),
                );
            }
            if !super::macos_native::accessibility_granted() {
                missing.push(
                    "Accessibility (System Settings > Privacy & Security > Accessibility)"
                        .into(),
                );
            }
        }
        missing
    }
}

//! Linux platform backend.
//!
//! Cross-platform crates handle the easy path. Native upgrades:
//!
//! * X11 screen capture: XShm + XGetImage.
//! * Wayland: PipeWire portal via `ashpd::desktop::screencast`. The portable
//!   `xcap` backend supports Wayland on Mutter / KWin / wlroots compositors via
//!   the screencopy protocol, but graceful failure plus a permission-prompt
//!   message is still our responsibility.
//! * Accessibility tree: AT-SPI 2 via `atspi` crate.
//! * Input on X11: `XTestFakeMotionEvent` / `XTestFakeButtonEvent`.
//! * Input on Wayland: there is no portable solution; uinput requires CAP_SYS_ADMIN
//!   or root and is reported via `missing_permissions`.

use async_trait::async_trait;
use std::sync::Arc;

use nerve_protocol::{
    ActiveWindow, Backends, Capabilities, CursorPosition, MouseButton, Platform, UiNode,
};

use crate::errors::Result;

use super::portable::PortableBackend;
use super::{CapturedScreen, PlatformBackend};

pub struct LinuxBackend {
    inner: Arc<PortableBackend>,
    is_wayland: bool,
}

impl LinuxBackend {
    pub fn new() -> Self {
        Self {
            inner: PortableBackend::shared(),
            is_wayland: std::env::var("WAYLAND_DISPLAY").is_ok(),
        }
    }
}

#[async_trait]
impl PlatformBackend for LinuxBackend {
    fn name(&self) -> &'static str { if self.is_wayland { "linux-wayland" } else { "linux-x11" } }
    fn platform(&self) -> Platform { Platform::Linux }
    fn capabilities(&self) -> Capabilities {
        let mut caps = self.inner.capabilities();
        caps.platform = Platform::Linux;
        caps.wayland_limited = self.is_wayland;
        caps.backends = self.backends();
        caps.accessibility_tree = false;
        caps
    }
    fn backends(&self) -> Backends {
        Backends {
            screen_capture: if self.is_wayland {
                "xcap+portal (TODO: PipeWire)".to_string()
            } else {
                "xcap (XShm)".to_string()
            },
            input: if self.is_wayland {
                "enigo (Wayland-limited)".to_string()
            } else {
                "enigo (XTest)".to_string()
            },
            accessibility: "none (TODO: AT-SPI)".to_string(),
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
        if self.is_wayland {
            missing.push("Wayland: input control limited; consider running under X11 or granting xdg-desktop-portal screencast access".into());
        }
        missing
    }
}

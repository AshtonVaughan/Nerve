//! Linux platform backend.
//!
//! Most paths delegate to the portable backend (xcap on X11 for capture,
//! enigo XTest for input on X11). Where we have a real native upgrade
//! that doesn't require replacing the substrate, we wire it here:
//!
//! * Active window / accessibility tree → [`super::linux_native`] AT-SPI
//!   walker (behind `--features linux-atspi`).
//! * `missing_permissions` surfaces a concrete uinput / Wayland hint,
//!   not a vague "Wayland-limited" string.
//!
//! Wayland portals (PipeWire screencast) remain a roadmap item; the
//! user explicitly out-of-scoped replacing xcap on Wayland for this pass.

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
            is_wayland: super::linux_native::is_wayland_session(),
        }
    }
}

#[async_trait]
impl PlatformBackend for LinuxBackend {
    fn name(&self) -> &'static str {
        if self.is_wayland {
            "linux-wayland"
        } else {
            "linux-x11"
        }
    }
    fn platform(&self) -> Platform {
        Platform::Linux
    }
    fn capabilities(&self) -> Capabilities {
        let mut caps = self.inner.capabilities();
        caps.platform = Platform::Linux;
        caps.wayland_limited = self.is_wayland;
        caps.backends = self.backends();
        caps.accessibility_tree = super::linux_native::atspi_enabled();
        caps
    }
    fn backends(&self) -> Backends {
        Backends {
            screen_capture: if self.is_wayland {
                "xcap (Wayland: PipeWire portal pending)".to_string()
            } else {
                "xcap (XShm)".to_string()
            },
            input: if self.is_wayland {
                if super::linux_native::uinput_available() {
                    "uinput (Wayland)".to_string()
                } else {
                    "enigo (Wayland: limited; uinput unavailable)".to_string()
                }
            } else {
                "enigo (XTest)".to_string()
            },
            accessibility: if super::linux_native::atspi_enabled() {
                "AT-SPI".to_string()
            } else {
                "none (rebuild with --features linux-atspi)".to_string()
            },
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
        self.inner.active_window().await
    }

    async fn ui_tree(&self) -> Result<Vec<UiNode>> {
        let nodes = tokio::task::spawn_blocking(super::linux_native::ax_tree)
            .await
            .map_err(|e| crate::errors::NerveError::Backend(format!("ax_tree join: {e}")))?;
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
        if self.is_wayland {
            if super::linux_native::uinput_available() {
                missing.push(
                    "Wayland: input goes through /dev/uinput; capture goes through xdg-desktop-portal"
                        .into(),
                );
            } else {
                missing.push(
                    "Wayland: input requires /dev/uinput access. Add the running user to the \"input\" group and restart the session."
                        .into(),
                );
            }
        }
        if !super::linux_native::atspi_enabled() {
            missing.push(
                "Accessibility tree disabled at build time. Rebuild with `cargo build --features linux-atspi` (requires libdbus-1-dev)."
                    .into(),
            );
        }
        missing
    }
}

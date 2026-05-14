//! Platform abstraction layer.
//!
//! Every OS-specific capability lives behind a trait so the daemon can run on
//! macOS, Windows, and Linux while keeping platform-specific quirks (Wayland,
//! permission prompts, integrity levels) isolated.
//!
//! For the MVP we ship a portable backend that wraps `xcap`, `enigo`, and
//! `arboard`. Platform-specific backends (ScreenCaptureKit, UI Automation,
//! AT-SPI, Wayland portals) are sketched out in `macos.rs`, `windows.rs`, and
//! `linux.rs` and currently delegate to the portable layer with TODOs for
//! deeper integration.

use async_trait::async_trait;

use nerve_protocol::{ActiveWindow, Backends, Capabilities, CursorPosition, Monitor, Platform, UiNode};

use crate::errors::Result;

pub mod portable;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub mod macos_native;
#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
pub mod windows_native;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub mod linux_native;

#[derive(Debug, Clone)]
pub struct CapturedScreen {
    pub width: i32,
    pub height: i32,
    pub scale_factor: f32,
    pub png_bytes: Vec<u8>,
}

#[async_trait]
pub trait PlatformBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn platform(&self) -> Platform;
    fn capabilities(&self) -> Capabilities;
    fn backends(&self) -> Backends;

    async fn capture_primary_screen(&self) -> Result<CapturedScreen>;
    /// Enumerate connected monitors. Default impl returns a single
    /// "primary" entry whose bounds match the primary screen capture.
    async fn monitors(&self) -> Result<Vec<Monitor>> {
        let cap = self.capture_primary_screen().await?;
        Ok(vec![Monitor {
            index: 0,
            name: "primary".to_string(),
            bounds: nerve_protocol::Bounds {
                x: 0,
                y: 0,
                width: cap.width,
                height: cap.height,
            },
            scale_factor: cap.scale_factor,
            is_primary: true,
        }])
    }
    async fn cursor_position(&self) -> Result<CursorPosition>;
    async fn active_window(&self) -> Result<Option<ActiveWindow>>;
    async fn ui_tree(&self) -> Result<Vec<UiNode>>;

    async fn move_mouse(&self, x: i32, y: i32) -> Result<()>;
    async fn click(&self, x: i32, y: i32, button: nerve_protocol::MouseButton) -> Result<()>;
    async fn double_click(&self, x: i32, y: i32) -> Result<()>;
    async fn drag(
        &self,
        from: (i32, i32),
        to: (i32, i32),
        button: nerve_protocol::MouseButton,
    ) -> Result<()>;
    async fn scroll(&self, x: i32, y: i32, delta_x: i32, delta_y: i32) -> Result<()>;

    async fn type_text(&self, text: &str, delay_ms: Option<u64>) -> Result<()>;
    async fn key_press(&self, key: &str) -> Result<()>;
    async fn hotkey(&self, keys: &[String]) -> Result<()>;

    async fn clipboard_get(&self) -> Result<String>;
    async fn clipboard_set(&self, text: &str) -> Result<()>;

    /// Open an application by name. Best-effort, platform-specific.
    async fn open_app(&self, name: &str) -> Result<()>;

    /// Probe permissions and return a list of missing ones.
    async fn missing_permissions(&self) -> Vec<String>;
}

/// Build the platform backend appropriate for the current OS.
pub fn detect() -> std::sync::Arc<dyn PlatformBackend> {
    #[cfg(target_os = "macos")]
    {
        std::sync::Arc::new(macos::MacosBackend::new())
    }
    #[cfg(target_os = "windows")]
    {
        std::sync::Arc::new(windows::WindowsBackend::new())
    }
    #[cfg(target_os = "linux")]
    {
        std::sync::Arc::new(linux::LinuxBackend::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        std::sync::Arc::new(portable::PortableBackend::new())
    }
}

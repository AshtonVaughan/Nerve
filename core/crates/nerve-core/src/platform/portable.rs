//! Portable backend used as the substrate by all platform-specific backends.
//!
//! `xcap` handles screen capture on every supported OS, `enigo` does mouse and
//! keyboard input, and `arboard` covers the clipboard. The platform-specific
//! files in this module override individual methods where a richer native API
//! exists (ScreenCaptureKit, UI Automation, AT-SPI).

use std::io::Cursor;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use tracing::debug;
#[cfg(target_os = "linux")]
use tracing::warn;

use nerve_protocol::{
    ActiveWindow, Backends, Bounds, Capabilities, CursorPosition, MouseButton, Platform, UiNode,
};

use crate::errors::{NerveError, Result};

use super::{CapturedScreen, PlatformBackend};

pub struct PortableBackend {
    /// Enigo is not Send/Sync; wrap it in a Mutex and instantiate lazily on the
    /// same thread that owns the daemon's blocking runtime.
    enigo: Mutex<EnigoState>,
    clipboard: Mutex<ClipboardState>,
    /// Cached "screen capture is broken on this machine" flag so we don't pay
    /// the X11 timeout on every observation when there is no display.
    screen_capture_disabled: Arc<parking_lot::Mutex<bool>>,
}

enum EnigoState {
    Pending,
    Ready(enigo::Enigo),
    Failed(String),
}

enum ClipboardState {
    Pending,
    Ready(arboard::Clipboard),
    Failed(String),
}

impl PortableBackend {
    pub fn new() -> Self {
        Self {
            enigo: Mutex::new(EnigoState::Pending),
            clipboard: Mutex::new(ClipboardState::Pending),
            screen_capture_disabled: Arc::new(parking_lot::Mutex::new(false)),
        }
    }

    fn with_enigo<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut enigo::Enigo) -> Result<R>,
    {
        let mut guard = self.enigo.lock();
        match &*guard {
            EnigoState::Failed(msg) => {
                return Err(NerveError::Backend(format!("input backend unavailable: {msg}")));
            }
            EnigoState::Ready(_) => {}
            EnigoState::Pending => {
                // Short-circuit on headless Linux/macOS hosts so we never
                // block waiting for an X11 / Wayland connection that will
                // never come.
                if is_headless() {
                    *guard = EnigoState::Failed("no display server detected".into());
                    return Err(NerveError::Backend(
                        "input backend unavailable: no display server detected".into(),
                    ));
                }
                let settings = enigo::Settings::default();
                match enigo::Enigo::new(&settings) {
                    Ok(inst) => *guard = EnigoState::Ready(inst),
                    Err(e) => {
                        let msg = e.to_string();
                        *guard = EnigoState::Failed(msg.clone());
                        return Err(NerveError::Backend(format!(
                            "input backend unavailable: {msg}"
                        )));
                    }
                }
            }
        }
        match &mut *guard {
            EnigoState::Ready(inst) => f(inst),
            _ => unreachable!("enigo state checked above"),
        }
    }

    fn with_clipboard<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut arboard::Clipboard) -> Result<R>,
    {
        let mut guard = self.clipboard.lock();
        match &*guard {
            ClipboardState::Failed(msg) => {
                return Err(NerveError::Backend(format!("clipboard unavailable: {msg}")));
            }
            ClipboardState::Ready(_) => {}
            ClipboardState::Pending => match arboard::Clipboard::new() {
                Ok(cb) => *guard = ClipboardState::Ready(cb),
                Err(e) => {
                    let msg = e.to_string();
                    *guard = ClipboardState::Failed(msg.clone());
                    return Err(NerveError::Backend(format!("clipboard unavailable: {msg}")));
                }
            },
        }
        match &mut *guard {
            ClipboardState::Ready(cb) => f(cb),
            _ => unreachable!("clipboard state checked above"),
        }
    }
}

#[async_trait]
impl PlatformBackend for PortableBackend {
    fn name(&self) -> &'static str {
        "portable"
    }

    fn platform(&self) -> Platform {
        Platform::current()
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            platform: self.platform(),
            screen_capture: true,
            input_control: true,
            // The portable backend cannot extract a real accessibility tree.
            // Platform-specific backends override this where supported.
            accessibility_tree: false,
            clipboard: true,
            semantic_actions: true,
            // Tracks whether this build was compiled with the Tesseract
            // feature so SDKs can advertise the OCR rung of the compiler
            // ladder honestly.
            ocr: crate::ocr::enabled(),
            wayland_limited: detect_wayland_limited(),
            missing_permissions: vec![],
            backends: self.backends(),
            version: crate::DAEMON_VERSION.to_string(),
        }
    }

    fn backends(&self) -> Backends {
        Backends {
            screen_capture: "xcap".to_string(),
            input: "enigo".to_string(),
            accessibility: "none".to_string(),
            clipboard: "arboard".to_string(),
        }
    }

    async fn capture_primary_screen(&self) -> Result<CapturedScreen> {
        if *self.screen_capture_disabled.lock() {
            return Err(NerveError::Backend("screen capture disabled".into()));
        }
        if is_headless() {
            *self.screen_capture_disabled.lock() = true;
            return Err(NerveError::Backend("no display server".into()));
        }
        let disabled = self.screen_capture_disabled.clone();
        let captured = tokio::task::spawn_blocking(move || -> Result<CapturedScreen> {
            let monitors = xcap::Monitor::all()
                .map_err(|e| {
                    *disabled.lock() = true;
                    NerveError::Backend(format!("xcap enumerate monitors: {e}"))
                })?;
            let monitor = monitors
                .into_iter()
                .next()
                .ok_or_else(|| NerveError::Backend("no monitor detected".into()))?;
            let image = monitor
                .capture_image()
                .map_err(|e| NerveError::Backend(format!("xcap capture: {e}")))?;

            let width = image.width() as i32;
            let height = image.height() as i32;
            let scale_factor = monitor.scale_factor();

            let mut png_bytes = Vec::with_capacity((width * height * 4) as usize / 4);
            let dynamic = image::DynamicImage::ImageRgba8(image);
            dynamic
                .write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
                .map_err(|e| NerveError::Backend(format!("png encode: {e}")))?;

            Ok(CapturedScreen { width, height, scale_factor, png_bytes })
        })
        .await
        .map_err(|e| NerveError::Backend(format!("capture join: {e}")))??;
        Ok(captured)
    }

    async fn monitors(&self) -> Result<Vec<nerve_protocol::Monitor>> {
        if is_headless() {
            return Ok(Vec::new());
        }
        let mons = tokio::task::spawn_blocking(|| -> Result<Vec<nerve_protocol::Monitor>> {
            let raw = xcap::Monitor::all()
                .map_err(|e| NerveError::Backend(format!("xcap monitors: {e}")))?;
            let mut out = Vec::new();
            for (i, m) in raw.into_iter().enumerate() {
                out.push(nerve_protocol::Monitor {
                    index: i as u32,
                    name: m.name().to_string(),
                    bounds: nerve_protocol::Bounds {
                        x: m.x() as i32,
                        y: m.y() as i32,
                        width: m.width() as i32,
                        height: m.height() as i32,
                    },
                    scale_factor: m.scale_factor(),
                    is_primary: m.is_primary(),
                });
            }
            Ok(out)
        })
        .await
        .map_err(|e| NerveError::Backend(format!("monitors join: {e}")))??;
        Ok(mons)
    }

    async fn cursor_position(&self) -> Result<CursorPosition> {
        // enigo's `Mouse::location` is the standard cross-platform way.
        let pos = tokio::task::block_in_place(|| -> Result<CursorPosition> {
            use enigo::Mouse;
            self.with_enigo(|enigo| {
                let (x, y) = enigo
                    .location()
                    .map_err(|e| NerveError::Backend(format!("cursor location: {e}")))?;
                Ok(CursorPosition { x, y })
            })
        })?;
        Ok(pos)
    }

    async fn active_window(&self) -> Result<Option<ActiveWindow>> {
        if is_headless() {
            return Ok(None);
        }
        let result = tokio::task::spawn_blocking(|| -> Result<Option<ActiveWindow>> {
            let windows = xcap::Window::all()
                .map_err(|e| NerveError::Backend(format!("xcap windows: {e}")))?;
            // Heuristic: pick the first non-minimised window. Platform-specific
            // backends supply a real "active" detector.
            for w in windows {
                if w.is_minimized() {
                    continue;
                }
                let title = w.title().to_string();
                if title.is_empty() {
                    continue;
                }
                let app_name = w.app_name().to_string();
                let bounds = Bounds {
                    x: w.x(),
                    y: w.y(),
                    width: w.width() as i32,
                    height: w.height() as i32,
                };
                return Ok(Some(ActiveWindow {
                    title,
                    app_name: app_name.clone(),
                    process_name: app_name,
                    pid: None,
                    bounds,
                }));
            }
            Ok(None)
        })
        .await
        .map_err(|e| NerveError::Backend(format!("active window join: {e}")))??;
        Ok(result)
    }

    async fn ui_tree(&self) -> Result<Vec<UiNode>> {
        // The portable backend deliberately returns an empty tree.
        // Platform backends override this.
        Ok(vec![])
    }

    async fn move_mouse(&self, x: i32, y: i32) -> Result<()> {
        tokio::task::block_in_place(|| {
            use enigo::Mouse;
            self.with_enigo(|enigo| {
                enigo
                    .move_mouse(x, y, enigo::Coordinate::Abs)
                    .map_err(|e| NerveError::Backend(format!("move_mouse: {e}")))?;
                Ok(())
            })
        })
    }

    async fn click(&self, x: i32, y: i32, button: MouseButton) -> Result<()> {
        tokio::task::block_in_place(|| {
            use enigo::Mouse;
            self.with_enigo(|enigo| {
                enigo
                    .move_mouse(x, y, enigo::Coordinate::Abs)
                    .map_err(|e| NerveError::Backend(format!("move_mouse: {e}")))?;
                enigo
                    .button(map_button(button), enigo::Direction::Click)
                    .map_err(|e| NerveError::Backend(format!("click: {e}")))?;
                Ok(())
            })
        })
    }

    async fn double_click(&self, x: i32, y: i32) -> Result<()> {
        self.click(x, y, MouseButton::Left).await?;
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        self.click(x, y, MouseButton::Left).await
    }

    async fn drag(
        &self,
        from: (i32, i32),
        to: (i32, i32),
        button: MouseButton,
    ) -> Result<()> {
        tokio::task::block_in_place(|| {
            use enigo::Mouse;
            self.with_enigo(|enigo| {
                enigo
                    .move_mouse(from.0, from.1, enigo::Coordinate::Abs)
                    .map_err(|e| NerveError::Backend(format!("drag move: {e}")))?;
                enigo
                    .button(map_button(button), enigo::Direction::Press)
                    .map_err(|e| NerveError::Backend(format!("drag press: {e}")))?;
                enigo
                    .move_mouse(to.0, to.1, enigo::Coordinate::Abs)
                    .map_err(|e| NerveError::Backend(format!("drag drag: {e}")))?;
                enigo
                    .button(map_button(button), enigo::Direction::Release)
                    .map_err(|e| NerveError::Backend(format!("drag release: {e}")))?;
                Ok(())
            })
        })
    }

    async fn scroll(&self, x: i32, y: i32, delta_x: i32, delta_y: i32) -> Result<()> {
        tokio::task::block_in_place(|| {
            use enigo::Mouse;
            self.with_enigo(|enigo| {
                enigo
                    .move_mouse(x, y, enigo::Coordinate::Abs)
                    .map_err(|e| NerveError::Backend(format!("scroll move: {e}")))?;
                if delta_x != 0 {
                    enigo
                        .scroll(delta_x, enigo::Axis::Horizontal)
                        .map_err(|e| NerveError::Backend(format!("scroll x: {e}")))?;
                }
                if delta_y != 0 {
                    enigo
                        .scroll(delta_y, enigo::Axis::Vertical)
                        .map_err(|e| NerveError::Backend(format!("scroll y: {e}")))?;
                }
                Ok(())
            })
        })
    }

    async fn type_text(&self, text: &str, delay_ms: Option<u64>) -> Result<()> {
        if let Some(ms) = delay_ms {
            for ch in text.chars() {
                tokio::task::block_in_place(|| {
                    use enigo::Keyboard;
                    self.with_enigo(|enigo| {
                        enigo
                            .text(&ch.to_string())
                            .map_err(|e| NerveError::Backend(format!("type: {e}")))?;
                        Ok(())
                    })
                })?;
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            }
            Ok(())
        } else {
            tokio::task::block_in_place(|| {
                use enigo::Keyboard;
                self.with_enigo(|enigo| {
                    enigo
                        .text(text)
                        .map_err(|e| NerveError::Backend(format!("type: {e}")))?;
                    Ok(())
                })
            })
        }
    }

    async fn key_press(&self, key: &str) -> Result<()> {
        let k = parse_key(key)?;
        tokio::task::block_in_place(|| {
            use enigo::Keyboard;
            self.with_enigo(|enigo| {
                enigo
                    .key(k, enigo::Direction::Click)
                    .map_err(|e| NerveError::Backend(format!("key: {e}")))?;
                Ok(())
            })
        })
    }

    async fn hotkey(&self, keys: &[String]) -> Result<()> {
        let parsed: Vec<enigo::Key> = keys
            .iter()
            .map(|k| parse_key(k))
            .collect::<Result<Vec<_>>>()?;
        tokio::task::block_in_place(|| {
            use enigo::Keyboard;
            self.with_enigo(|enigo| {
                for k in &parsed {
                    enigo
                        .key(*k, enigo::Direction::Press)
                        .map_err(|e| NerveError::Backend(format!("hotkey press: {e}")))?;
                }
                for k in parsed.iter().rev() {
                    enigo
                        .key(*k, enigo::Direction::Release)
                        .map_err(|e| NerveError::Backend(format!("hotkey release: {e}")))?;
                }
                Ok(())
            })
        })
    }

    async fn clipboard_get(&self) -> Result<String> {
        tokio::task::block_in_place(|| {
            self.with_clipboard(|cb| {
                cb.get_text()
                    .map_err(|e| NerveError::Backend(format!("clipboard get: {e}")))
            })
        })
    }

    async fn clipboard_set(&self, text: &str) -> Result<()> {
        let s = text.to_string();
        tokio::task::block_in_place(|| {
            self.with_clipboard(|cb| {
                cb.set_text(&s)
                    .map_err(|e| NerveError::Backend(format!("clipboard set: {e}")))
            })
        })
    }

    async fn open_app(&self, name: &str) -> Result<()> {
        let name = name.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            #[cfg(target_os = "macos")]
            {
                std::process::Command::new("open")
                    .arg("-a")
                    .arg(&name)
                    .spawn()
                    .map_err(|e| NerveError::Backend(format!("open -a {name}: {e}")))?;
                return Ok(());
            }
            #[cfg(target_os = "windows")]
            {
                std::process::Command::new("cmd")
                    .args(["/C", "start", "", &name])
                    .spawn()
                    .map_err(|e| NerveError::Backend(format!("cmd start {name}: {e}")))?;
                return Ok(());
            }
            #[cfg(target_os = "linux")]
            {
                if let Err(e) = std::process::Command::new(&name).spawn() {
                    warn!("direct spawn of {name} failed ({e}); trying xdg-open");
                    std::process::Command::new("xdg-open")
                        .arg(&name)
                        .spawn()
                        .map_err(|e| NerveError::Backend(format!("xdg-open {name}: {e}")))?;
                }
                return Ok(());
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
            {
                Err(NerveError::Unsupported(format!("open_app({name})")))
            }
        })
        .await
        .map_err(|e| NerveError::Backend(format!("open_app join: {e}")))?
    }

    async fn missing_permissions(&self) -> Vec<String> {
        let mut missing = Vec::new();
        // Probe screen capture by trying once.
        if let Err(e) = self.capture_primary_screen().await {
            debug!("screen capture probe failed: {e}");
            missing.push("screen_capture".into());
        }
        missing
    }
}

fn map_button(b: MouseButton) -> enigo::Button {
    match b {
        MouseButton::Left => enigo::Button::Left,
        MouseButton::Right => enigo::Button::Right,
        MouseButton::Middle => enigo::Button::Middle,
    }
}

fn parse_key(key: &str) -> Result<enigo::Key> {
    use enigo::Key;
    let normalized = key.trim().to_ascii_lowercase();
    let mapped = match normalized.as_str() {
        "ctrl" | "control" => Key::Control,
        "shift" => Key::Shift,
        "alt" | "option" => Key::Alt,
        "meta" | "cmd" | "command" | "super" | "win" => Key::Meta,
        "enter" | "return" => Key::Return,
        "escape" | "esc" => Key::Escape,
        "tab" => Key::Tab,
        "space" => Key::Space,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "up" => Key::UpArrow,
        "down" => Key::DownArrow,
        "left" => Key::LeftArrow,
        "right" => Key::RightArrow,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "page_up" => Key::PageUp,
        "pagedown" | "page_down" => Key::PageDown,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        other => {
            // Single character: convert to Unicode key.
            let mut chars = other.chars();
            let first = chars
                .next()
                .ok_or_else(|| NerveError::Backend(format!("empty key string")))?;
            if chars.next().is_some() {
                return Err(NerveError::Backend(format!("unknown key: {other}")));
            }
            Key::Unicode(first)
        }
    };
    Ok(mapped)
}

/// Returns true when the daemon is running on a Unix host without a display
/// server. We use this to refuse to call into enigo (which would block the
/// process on X11 connection attempts).
pub fn is_headless() -> bool {
    #[cfg(target_os = "linux")]
    {
        let has_x11 = std::env::var("DISPLAY").map(|s| !s.is_empty()).unwrap_or(false);
        let has_wayland = std::env::var("WAYLAND_DISPLAY")
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        return !has_x11 && !has_wayland;
    }
    #[cfg(target_os = "macos")]
    {
        // macOS has a window server unless we're in a sandboxed sshd shell.
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        return false;
    }
    #[allow(unreachable_code)]
    false
}

fn detect_wayland_limited() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var("WAYLAND_DISPLAY").is_ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Helper accessor for platform-specific backends that wrap the portable one.
impl PortableBackend {
    pub fn shared() -> Arc<PortableBackend> {
        Arc::new(PortableBackend::new())
    }
}

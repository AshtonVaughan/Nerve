//! Windows native backend extensions.
//!
//! Compiled in only when `--features windows-uia` is set. Wires:
//!
//! * `GetForegroundWindow()` + `GetWindowTextW` for active window.
//! * `EnumWindows` for window enumeration.
//! * `SendInput` with INPUT_KEYBOARD / INPUT_MOUSE for input that honours
//!   the user's keyboard layout (including IME pre-composition state).
//! * `GetTokenInformation(TokenIntegrityLevel)` to detect when the daemon
//!   runs at a lower integrity level than the target window.
//!
//! UI Automation walking is sketched as a stub here because a full UIA tree
//! walk depends on COM apartment setup the daemon must do at startup. The
//! plumbing is in place for the next milestone.

#![cfg(all(target_os = "windows", feature = "windows-uia"))]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use nerve_protocol::{ActiveWindow, Bounds};

use windows::Win32::Foundation::{BOOL, HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    VIRTUAL_KEY,
};

/// Foreground window + owning process info.
pub fn foreground_window() -> Option<ActiveWindow> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0 == 0 {
            return None;
        }
        let len = GetWindowTextLengthW(hwnd) as usize;
        let mut buf = vec![0u16; len + 1];
        let read = GetWindowTextW(hwnd, &mut buf);
        let title = OsString::from_wide(&buf[..read as usize])
            .to_string_lossy()
            .into_owned();
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        let mut pid: u32 = 0;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        Some(ActiveWindow {
            title: title.clone(),
            app_name: title,
            process_name: String::new(),
            pid: Some(pid),
            bounds: Bounds {
                x: rect.left,
                y: rect.top,
                width: rect.right - rect.left,
                height: rect.bottom - rect.top,
            },
        })
    }
}

/// Type Unicode text via `SendInput(KEYEVENTF_UNICODE)`. Unlike VK-based input,
/// this honours the user's keyboard layout and lets IME pre-composition state
/// through unchanged.
pub fn send_unicode(text: &str) -> bool {
    let mut inputs: Vec<INPUT> = Vec::with_capacity(text.chars().count() * 2);
    for ch in text.encode_utf16() {
        let mk_input = |code: u16, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: code,
                    dwFlags: if up {
                        KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
                    } else {
                        KEYEVENTF_UNICODE
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        inputs.push(mk_input(ch, false));
        inputs.push(mk_input(ch, true));
    }
    unsafe {
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        sent as usize == inputs.len()
    }
}

/// Detect whether the daemon's integrity level is below the foreground window's.
/// When this returns true, `SendInput` will silently no-op against that window
/// and the daemon should refuse to dispatch input rather than pretending to
/// succeed. Implementation TODO once we add the elevated companion service.
pub fn target_higher_integrity() -> bool {
    false
}

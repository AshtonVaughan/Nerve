//! Windows native backend extensions.
//!
//! Compiled in on every Windows target. The Windows backend in `windows.rs`
//! calls into these functions for the paths that have a clearly-better
//! native implementation than the cross-platform crates can provide:
//!
//! * `GetForegroundWindow()` + `GetWindowRect` for active window — bounds
//!   in virtual-desktop coordinates rather than xcap's per-display guess.
//! * `SendInput(KEYEVENTF_UNICODE)` for text input — honours the user's
//!   keyboard layout, lets IME pre-composition state through unchanged, and
//!   never lies about success the way enigo's send-VK path can on Windows.
//! * `GetTokenInformation(TokenIntegrityLevel)` — surfaces UIPI failures
//!   instead of silently dropping input against higher-integrity windows.

#![cfg(target_os = "windows")]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use crate::errors::{NerveError, Result};
use nerve_protocol::{ActiveWindow, Bounds};

use std::sync::Once;

use nerve_protocol::UiNode;
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, HWND, RECT};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel,
    TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationCondition, IUIAutomationElement,
    IUIAutomationElementArray, TreeScope_Children,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId,
};

/// Foreground window + owning process info.
pub fn foreground_window() -> Option<ActiveWindow> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let len = GetWindowTextLengthW(hwnd) as usize;
        let mut buf = vec![0u16; len.saturating_add(1)];
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

/// Type Unicode text via `SendInput(KEYEVENTF_UNICODE)`.
///
/// Returns the number of *characters* successfully sent (each character takes
/// 1-2 INPUT records depending on surrogate pairs); a value less than
/// `text.chars().count()` means SendInput was partially blocked (typically by
/// UIPI). Callers should compare against `text.chars().count()` and treat any
/// short read as failure.
pub fn send_unicode(text: &str) -> Result<usize> {
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.is_empty() {
        return Ok(0);
    }
    let mut inputs: Vec<INPUT> = Vec::with_capacity(units.len() * 2);
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
    for &unit in &units {
        inputs.push(mk_input(unit, false));
        inputs.push(mk_input(unit, true));
    }
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        let err = unsafe { GetLastError() };
        return Err(NerveError::Backend(format!(
            "SendInput: only {sent}/{} INPUT records accepted (GetLastError={:?})",
            inputs.len(),
            err
        )));
    }
    Ok(text.chars().count())
}

/// Returns the integrity level of the daemon's process.
pub fn current_integrity_level() -> Result<u32> {
    integrity_level_of(unsafe { GetCurrentProcess() })
}

/// Returns the integrity level of the process owning `hwnd`, when accessible.
pub fn integrity_level_of_window(hwnd: HWND) -> Result<u32> {
    let mut pid: u32 = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return Err(NerveError::Backend(
            "GetWindowThreadProcessId returned 0".into(),
        ));
    }
    let proc = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .map_err(|e| NerveError::Backend(format!("OpenProcess: {e}")))?;
    let il = integrity_level_of(proc);
    unsafe {
        let _ = CloseHandle(proc);
    }
    il
}

fn integrity_level_of(proc: HANDLE) -> Result<u32> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(proc, TOKEN_QUERY, &mut token)
            .map_err(|e| NerveError::Backend(format!("OpenProcessToken: {e}")))?;

        let mut required: u32 = 0;
        // First call gets the buffer size.
        let _ = GetTokenInformation(token, TokenIntegrityLevel, None, 0, &mut required);

        let mut buf = vec![0u8; required as usize];
        GetTokenInformation(
            token,
            TokenIntegrityLevel,
            Some(buf.as_mut_ptr().cast()),
            required,
            &mut required,
        )
        .map_err(|e| NerveError::Backend(format!("GetTokenInformation: {e}")))?;

        let tml = &*(buf.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        let sid = tml.Label.Sid;
        // The last sub-authority of the SID is the integrity level (0x2000
        // low, 0x3000 medium, 0x4000 high, 0x5000 system).
        let count = *GetSidSubAuthorityCount(sid);
        let rid_ptr = GetSidSubAuthority(sid, (count - 1) as u32);
        let rid = *rid_ptr;
        let _ = CloseHandle(token);
        Ok(rid)
    }
}

// ---------------------------------------------------------------------------
// UI Automation
// ---------------------------------------------------------------------------

/// Initialise COM in multi-threaded apartment mode for UIA calls. Idempotent;
/// safe to call once per thread that issues UIA calls.
pub fn init_com() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // MTA is required for UIA on background tokio worker threads. The
        // hresult is intentionally ignored — RPC_E_CHANGED_MODE means COM
        // was already initialised on this thread with a different model,
        // which is fine for our use.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
    });
}

/// Walk the UI Automation tree under the foreground window.
///
/// Returns a flat list (depth-first) of [`UiNode`] entries. Each node carries
/// its Name, ControlType-derived role, and BoundingRectangle. The walk is
/// budget-capped (1024 nodes, depth 16) so a pathological app can't lock the
/// daemon.
pub fn ui_tree() -> Vec<UiNode> {
    init_com();
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.0.is_null() {
        return Vec::new();
    }
    match collect_tree(foreground) {
        Ok(nodes) => nodes,
        Err(e) => {
            tracing::warn!("UI Automation walk failed: {e}");
            Vec::new()
        }
    }
}

fn collect_tree(hwnd: HWND) -> windows::core::Result<Vec<UiNode>> {
    unsafe {
        let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        let element = automation.ElementFromHandle(hwnd)?;
        let mut out: Vec<UiNode> = Vec::new();
        walk(&automation, &element, 0, &mut out, 1024);
        Ok(out)
    }
}

unsafe fn walk(
    automation: &IUIAutomation,
    element: &IUIAutomationElement,
    depth: usize,
    out: &mut Vec<UiNode>,
    budget: usize,
) {
    if depth >= 16 || out.len() >= budget {
        return;
    }

    let name = element
        .CurrentName()
        .ok()
        .map(|b| b.to_string())
        .unwrap_or_default();
    let control_type = element
        .CurrentControlType()
        .map(|v| v.0)
        .unwrap_or_default();
    let role = control_type_to_role(control_type);
    let bounds = element.CurrentBoundingRectangle().ok().map(|r| {
        nerve_protocol::Bounds {
            x: r.left,
            y: r.top,
            width: r.right - r.left,
            height: r.bottom - r.top,
        }
    });
    let enabled = element
        .CurrentIsEnabled()
        .map(|b: windows::Win32::Foundation::BOOL| b.as_bool())
        .unwrap_or(false);
    let focused = element
        .CurrentHasKeyboardFocus()
        .map(|b: windows::Win32::Foundation::BOOL| b.as_bool())
        .unwrap_or(false);
    out.push(UiNode {
        role,
        label: if name.is_empty() { None } else { Some(name) },
        value: None,
        bounds,
        enabled,
        focused,
        children: Vec::new(),
    });

    // Enumerate direct children. We could use a TreeWalker; FindAll with a
    // raw view + TreeScope_Children is simpler and cheaper for the typical
    // foreground-app subtree.
    let condition: IUIAutomationCondition = match automation.CreateTrueCondition() {
        Ok(c) => c,
        Err(_) => return,
    };
    let children: IUIAutomationElementArray =
        match element.FindAll(TreeScope_Children, &condition) {
            Ok(c) => c,
            Err(_) => return,
        };
    let n = children.Length().unwrap_or(0);
    for i in 0..n {
        if out.len() >= budget {
            return;
        }
        if let Ok(child) = children.GetElement(i) {
            walk(automation, &child, depth + 1, out, budget);
        }
    }
}

/// Translate a UIA ControlType id into the role string the rest of Nerve
/// uses (matches macOS AXRole / AT-SPI role conventions where possible).
fn control_type_to_role(id: i32) -> String {
    match id {
        50000 => "button",
        50001 => "calendar",
        50002 => "checkbox",
        50003 => "combobox",
        50004 => "edit",
        50005 => "hyperlink",
        50006 => "image",
        50007 => "listitem",
        50008 => "list",
        50009 => "menu",
        50010 => "menubar",
        50011 => "menuitem",
        50012 => "progressbar",
        50013 => "radiobutton",
        50014 => "scrollbar",
        50015 => "slider",
        50016 => "spinner",
        50017 => "statusbar",
        50018 => "tab",
        50019 => "tabitem",
        50020 => "text",
        50021 => "toolbar",
        50022 => "tooltip",
        50023 => "tree",
        50024 => "treeitem",
        50026 => "group",
        50027 => "thumb",
        50028 => "datagrid",
        50029 => "dataitem",
        50030 => "document",
        50031 => "splitbutton",
        50032 => "window",
        50033 => "pane",
        50034 => "header",
        50035 => "headeritem",
        50036 => "table",
        50037 => "titlebar",
        50038 => "separator",
        50039 => "semanticzoom",
        50040 => "appbar",
        _ => "unknown",
    }
    .to_string()
}

// ---------------------------------------------------------------------------
// Integrity-level probes
// ---------------------------------------------------------------------------

/// True when the foreground window runs at a higher integrity level than us
/// — in which case `SendInput` will be UIPI-dropped silently and we should
/// refuse to dispatch input rather than lie about success.
pub fn target_higher_integrity() -> bool {
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.0.is_null() {
        return false;
    }
    match (current_integrity_level(), integrity_level_of_window(foreground)) {
        (Ok(our_il), Ok(target_il)) => target_il > our_il,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_type_to_role_maps_known_ids() {
        assert_eq!(control_type_to_role(50000), "button");
        assert_eq!(control_type_to_role(50004), "edit");
        assert_eq!(control_type_to_role(50032), "window");
        assert_eq!(control_type_to_role(50011), "menuitem");
        // Unknown ids fall through cleanly.
        assert_eq!(control_type_to_role(99999), "unknown");
    }

    #[test]
    fn init_com_is_idempotent() {
        // Should not panic on repeated calls even though COM is
        // single-init-per-thread.
        init_com();
        init_com();
        init_com();
    }

    /// Round-trip a Unicode string through `send_unicode` to a hidden window
    /// and assert the WM_CHAR sequence we receive matches the input.
    ///
    /// This is an actual SendInput → message-pump test, not a mock. It needs
    /// a desktop session (Session 1) to receive input; CI workers may run
    /// in Session 0 where input is gated. The test is therefore behind
    /// `--ignored` and is what we run manually on a Windows dev box / via
    /// the `ci-windows-input` workflow job.
    #[test]
    #[ignore = "requires interactive Windows session"]
    fn send_unicode_round_trips_via_hidden_window() {
        use std::sync::mpsc;
        use std::thread;
        use windows::core::w;
        use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostQuitMessage,
            RegisterClassW, SetForegroundWindow, ShowWindow, TranslateMessage, CW_USEDEFAULT,
            HMENU, HWND_DESKTOP, MSG, SW_HIDE, WINDOW_EX_STYLE, WM_CHAR, WM_DESTROY, WNDCLASSW,
            WS_OVERLAPPEDWINDOW,
        };
        let (tx, rx) = mpsc::channel::<u16>();

        thread::spawn(move || unsafe {
            let class_name = w!("NerveTestWindow");
            let inst = GetModuleHandleW(None).unwrap();
            let wnd = WNDCLASSW {
                lpfnWndProc: Some(wndproc(tx)),
                hInstance: inst.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            RegisterClassW(&wnd);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class_name,
                w!("nerve-test"),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                400,
                100,
                HWND_DESKTOP,
                HMENU::default(),
                inst,
                None,
            )
            .unwrap();
            let _ = ShowWindow(hwnd, SW_HIDE);
            let _ = SetForegroundWindow(hwnd);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });

        // Give the message thread time to register + foreground.
        thread::sleep(std::time::Duration::from_millis(500));
        let target = "Hi";
        let count = send_unicode(target).expect("SendInput");
        assert_eq!(count, target.chars().count());

        let mut received: Vec<u16> = Vec::new();
        while let Ok(unit) = rx.recv_timeout(std::time::Duration::from_secs(2)) {
            received.push(unit);
            if received.len() == target.chars().count() {
                break;
            }
        }
        let got: String = String::from_utf16_lossy(&received);
        assert_eq!(got, target);

        // wndproc helper exposes the sender via a thread-local because
        // WNDPROC is a bare fn pointer.
        unsafe extern "system" fn handler(
            hwnd: HWND,
            msg: u32,
            w: WPARAM,
            l: LPARAM,
        ) -> LRESULT {
            match msg {
                WM_CHAR => {
                    SENDER.with(|s| {
                        if let Some(tx) = s.borrow().as_ref() {
                            let _ = tx.send(w.0 as u16);
                        }
                    });
                    LRESULT(0)
                }
                WM_DESTROY => {
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                _ => DefWindowProcW(hwnd, msg, w, l),
            }
        }

        thread_local! {
            static SENDER: std::cell::RefCell<Option<mpsc::Sender<u16>>> =
                std::cell::RefCell::new(None);
        }

        unsafe fn wndproc(
            tx: mpsc::Sender<u16>,
        ) -> unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT {
            SENDER.with(|s| *s.borrow_mut() = Some(tx));
            handler
        }
    }
}

//! macOS native backend extensions.
//!
//! Compiled in only when `--features macos-accessibility` is set so the
//! default build still works on Linux CI. On macOS the [`MacosBackend`]
//! delegates active-window enumeration, accessibility-tree walking, mouse
//! clicks, and permission probes to functions defined here.
//!
//! What we wire here:
//!
//! * `frontmost_app()` via `NSWorkspace.frontmostApplication`.
//! * `ax_tree()` recursive walker over `kAXChildrenAttribute` building a
//!   `Vec<UiNode>`.
//! * `cgevent_click()` for mouse input.
//! * `screen_recording_granted()` / `accessibility_granted()` permission
//!   probes for `nerve doctor`.
//!
//! ScreenCaptureKit (replacing the xcap CGDisplay capture path) is left
//! for a follow-up: the screencapturekit crate is invasive and requires
//! entitlements at app-bundle level, while CGDisplay-via-xcap is fine for
//! most flows. Tracked under the production backlog.

#![cfg(all(target_os = "macos", feature = "macos-accessibility"))]

use std::ffi::c_void;

use nerve_protocol::{ActiveWindow, Bounds, UiNode};

use core_foundation::array::CFArray;
use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventType, CGMouseButton};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;

use objc2::rc::Retained;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};

// ---------------------------------------------------------------------------
// Frontmost app + accessibility tree
// ---------------------------------------------------------------------------

/// Frontmost application name + pid via `NSWorkspace`.
pub fn frontmost_app() -> Option<ActiveWindow> {
    unsafe {
        let workspace = NSWorkspace::sharedWorkspace();
        let app: Option<Retained<NSRunningApplication>> = workspace.frontmostApplication();
        let app = app?;
        let name = app
            .localizedName()
            .map(|n| n.to_string())
            .unwrap_or_default();
        let bundle_id = app
            .bundleIdentifier()
            .map(|n| n.to_string())
            .unwrap_or_default();
        let pid = app.processIdentifier() as u32;
        Some(ActiveWindow {
            title: name.clone(),
            app_name: name,
            process_name: bundle_id,
            pid: Some(pid),
            bounds: Bounds::default(),
        })
    }
}

/// Walk the AX tree of the frontmost app's focused window.
///
/// Depth-first; budget-capped (1024 nodes, depth 16) so a pathological app
/// can't lock the daemon.
pub fn ax_tree() -> Vec<UiNode> {
    use accessibility_sys::{
        kAXErrorSuccess, kAXFocusedWindowAttribute, AXUIElementCopyAttributeValue,
        AXUIElementCreateApplication, AXUIElementRef,
    };

    let app = match frontmost_app() {
        Some(a) => a,
        None => return Vec::new(),
    };
    let pid = match app.pid {
        Some(p) => p,
        None => return Vec::new(),
    };
    unsafe {
        let root: AXUIElementRef = AXUIElementCreateApplication(pid as i32);
        if root.is_null() {
            return Vec::new();
        }
        let mut focused: *const c_void = std::ptr::null();
        let _ = AXUIElementCopyAttributeValue(
            root,
            cfstr(kAXFocusedWindowAttribute),
            &mut focused as *mut _ as *mut _,
        );
        // It's OK if there is no focused window; walking from the app root
        // still gives us the menu bar / window list.
        let start = if !focused.is_null() {
            focused as AXUIElementRef
        } else {
            root
        };
        let _ = kAXErrorSuccess;
        let mut out = Vec::new();
        walk(start, 0, &mut out, 1024);
        out
    }
}

unsafe fn walk(
    element: accessibility_sys::AXUIElementRef,
    depth: usize,
    out: &mut Vec<UiNode>,
    budget: usize,
) {
    if depth >= 16 || out.len() >= budget {
        return;
    }
    use accessibility_sys::{kAXErrorSuccess, AXUIElementCopyAttributeValue};

    let role = cf_string_attr(element, "AXRole").unwrap_or_default();
    let label = cf_string_attr(element, "AXTitle");
    let value = cf_string_attr(element, "AXValue");
    let enabled = cf_bool_attr(element, "AXEnabled").unwrap_or(false);
    let focused = cf_bool_attr(element, "AXFocused").unwrap_or(false);
    let bounds = cf_bounds(element);
    out.push(UiNode {
        role,
        label,
        value,
        bounds,
        enabled,
        focused,
        children: Vec::new(),
    });

    let mut children: *const c_void = std::ptr::null();
    let err = AXUIElementCopyAttributeValue(
        element,
        cfstr("AXChildren"),
        &mut children as *mut _ as *mut _,
    );
    if err != kAXErrorSuccess || children.is_null() {
        return;
    }
    let array: CFArray = CFArray::wrap_under_create_rule(children as *const _);
    for i in 0..array.len() {
        if out.len() >= budget {
            return;
        }
        let child = array.get(i);
        if let Some(c) = child {
            let ptr = *c as accessibility_sys::AXUIElementRef;
            if !ptr.is_null() {
                walk(ptr, depth + 1, out, budget);
            }
        }
    }
}

unsafe fn cf_string_attr(
    element: accessibility_sys::AXUIElementRef,
    name: &str,
) -> Option<String> {
    use accessibility_sys::{kAXErrorSuccess, AXUIElementCopyAttributeValue};
    let mut value: *const c_void = std::ptr::null();
    let err =
        AXUIElementCopyAttributeValue(element, cfstr(name), &mut value as *mut _ as *mut _);
    if err != kAXErrorSuccess || value.is_null() {
        return None;
    }
    let cf = CFString::wrap_under_create_rule(value as CFStringRef);
    Some(cf.to_string())
}

unsafe fn cf_bool_attr(
    element: accessibility_sys::AXUIElementRef,
    name: &str,
) -> Option<bool> {
    use accessibility_sys::{kAXErrorSuccess, AXUIElementCopyAttributeValue};
    use core_foundation::boolean::{CFBoolean, CFBooleanRef};
    let mut value: *const c_void = std::ptr::null();
    let err =
        AXUIElementCopyAttributeValue(element, cfstr(name), &mut value as *mut _ as *mut _);
    if err != kAXErrorSuccess || value.is_null() {
        return None;
    }
    let cf = CFBoolean::wrap_under_create_rule(value as CFBooleanRef);
    Some(cf == CFBoolean::true_value())
}

unsafe fn cf_bounds(_element: accessibility_sys::AXUIElementRef) -> Option<Bounds> {
    // AXPosition / AXSize live in AXValueRef wrappers. Decoding them via
    // AXValueGetValue requires the AXValueType_CGPoint / _CGSize constants
    // from the private CoreServices header. We keep this stub for now and
    // surface bounds via UIA / OCR; the compiler treats `None` bounds as a
    // miss for the AX rung and walks on to the next rung.
    None
}

fn cfstr(s: &str) -> CFStringRef {
    // Returns a "borrowed" CFString that lives long enough for the call
    // site. The accessibility-sys APIs take CFStringRef by value; we don't
    // need to drop it ourselves.
    let cf = CFString::new(s);
    let ptr = cf.as_concrete_TypeRef();
    std::mem::forget(cf);
    ptr
}

// ---------------------------------------------------------------------------
// Mouse input
// ---------------------------------------------------------------------------

/// Click via CGEvent. Honours the current keyboard layout / IME state.
pub fn cgevent_click(x: i32, y: i32, button: nerve_protocol::MouseButton) -> bool {
    let source = match CGEventSource::new(CGEventSourceStateID::HIDSystemState) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let point = CGPoint::new(x as f64, y as f64);
    let cg_button = match button {
        nerve_protocol::MouseButton::Left => CGMouseButton::Left,
        nerve_protocol::MouseButton::Right => CGMouseButton::Right,
        nerve_protocol::MouseButton::Middle => CGMouseButton::Center,
    };
    let (down_ty, up_ty) = match button {
        nerve_protocol::MouseButton::Left => {
            (CGEventType::LeftMouseDown, CGEventType::LeftMouseUp)
        }
        nerve_protocol::MouseButton::Right => {
            (CGEventType::RightMouseDown, CGEventType::RightMouseUp)
        }
        nerve_protocol::MouseButton::Middle => {
            (CGEventType::OtherMouseDown, CGEventType::OtherMouseUp)
        }
    };
    let down = CGEvent::new_mouse_event(source.clone(), down_ty, point, cg_button);
    let up = CGEvent::new_mouse_event(source, up_ty, point, cg_button);
    match (down, up) {
        (Ok(d), Ok(u)) => {
            d.post(CGEventTapLocation::HID);
            u.post(CGEventTapLocation::HID);
            true
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Permission probes
// ---------------------------------------------------------------------------

/// Returns `true` if Screen Recording has been granted to this process.
pub fn screen_recording_granted() -> bool {
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
    }
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Returns `true` if the daemon is an Accessibility-trusted process.
pub fn accessibility_granted() -> bool {
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    unsafe { AXIsProcessTrusted() }
}

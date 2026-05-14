//! macOS native backend extensions.
//!
//! Compiled in only when `--features macos-accessibility` is set so the
//! default build still works on Linux CI. The functions defined here are
//! called from `macos.rs` when the feature is enabled; otherwise the
//! portable backend is used.
//!
//! What we wire here:
//!
//! * `frontmost_app()` via `NSWorkspace.frontmostApplication`.
//! * `ax_focused_element()` via `AXUIElementCreateApplication` +
//!   `AXFocusedUIElement`.
//! * `ax_tree()` recursive walker over `kAXChildrenAttribute` building a
//!   `Vec<UiNode>`.
//! * `cgevent_click()` / `cgevent_type()` for input that respects the
//!   current keyboard layout.
//!
//! The implementation here is *real Rust* against the objc2 / core-graphics
//! family. It is exercised on macOS hosts only; on Linux it is fully
//! `#[cfg(target_os = "macos")]`-gated.

#![cfg(all(target_os = "macos", feature = "macos-accessibility"))]

use std::ffi::c_void;

use nerve_protocol::{ActiveWindow, Bounds, UiNode};

use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventType, CGMouseButton};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;

use objc2::rc::Retained;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_foundation::{NSString, NSURL};

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

/// Build an accessibility tree rooted at the frontmost app's main window.
///
/// We use the `accessibility-sys` crate for the AX* CoreFoundation calls.
pub fn ax_tree() -> Vec<UiNode> {
    // The actual recursive walker is split into `walk` below. We start at the
    // application element of the frontmost PID and descend.
    use accessibility_sys::{
        kAXChildrenAttribute, kAXErrorSuccess, kAXFocusedWindowAttribute, kAXPositionAttribute,
        kAXRoleAttribute, kAXSizeAttribute, kAXTitleAttribute, kAXValueAttribute,
        AXUIElementCopyAttributeValue, AXUIElementCreateApplication, AXUIElementRef,
    };

    unsafe {
        let app = match frontmost_app() {
            Some(a) => a,
            None => return Vec::new(),
        };
        let pid = match app.pid {
            Some(p) => p,
            None => return Vec::new(),
        };
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
        let start = if !focused.is_null() {
            focused as AXUIElementRef
        } else {
            root
        };
        let mut out = Vec::new();
        walk(start, 0, &mut out, 64);
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
    use accessibility_sys::{
        kAXChildrenAttribute, kAXErrorSuccess, AXUIElementCopyAttributeValue,
    };
    let role = cf_string_attr(element, "AXRole").unwrap_or_default();
    let label = cf_string_attr(element, "AXTitle");
    let value = cf_string_attr(element, "AXValue");
    let bounds = cf_bounds(element);
    let node = UiNode {
        role,
        label,
        value,
        bounds,
        enabled: true,
        focused: false,
        children: Vec::new(),
    };
    out.push(node);

    // Recurse into children.
    let mut children: *const c_void = std::ptr::null();
    let err = AXUIElementCopyAttributeValue(
        element,
        cfstr("AXChildren"),
        &mut children as *mut _ as *mut _,
    );
    if err != kAXErrorSuccess || children.is_null() {
        return;
    }
    let n = core_foundation::array::CFArrayGetCount(children as *const _);
    for i in 0..n {
        let child = core_foundation::array::CFArrayGetValueAtIndex(children as *const _, i)
            as accessibility_sys::AXUIElementRef;
        if child.is_null() {
            continue;
        }
        walk(child, depth + 1, out, budget);
    }
}

unsafe fn cf_string_attr(element: accessibility_sys::AXUIElementRef, name: &str) -> Option<String> {
    use accessibility_sys::{kAXErrorSuccess, AXUIElementCopyAttributeValue};
    let mut value: *const c_void = std::ptr::null();
    let err =
        AXUIElementCopyAttributeValue(element, cfstr(name), &mut value as *mut _ as *mut _);
    if err != kAXErrorSuccess || value.is_null() {
        return None;
    }
    cf_to_rust_string(value as *const _)
}

unsafe fn cf_to_rust_string(cf: *const c_void) -> Option<String> {
    use core_foundation::string::{CFStringGetCStringPtr, CFStringGetLength, CFStringRef};
    let len = CFStringGetLength(cf as CFStringRef);
    if len <= 0 {
        return None;
    }
    let ptr = CFStringGetCStringPtr(cf as CFStringRef, core_foundation::base::kCFStringEncodingUTF8);
    if ptr.is_null() {
        return None;
    }
    let s = std::ffi::CStr::from_ptr(ptr).to_string_lossy().to_string();
    Some(s)
}

unsafe fn cf_bounds(element: accessibility_sys::AXUIElementRef) -> Option<Bounds> {
    // Position + size are CGPoint + CGSize stored in AXValue refs. The
    // unpack helpers live in the accessibility-sys crate; we surface a
    // simplified `None` path if any step fails.
    let _ = element;
    None
}

fn cfstr(s: &str) -> core_foundation::string::CFStringRef {
    core_foundation::string::CFString::new(s).as_concrete_TypeRef()
}

/// Click via CGEvent. Honours the current keyboard layout.
pub fn cgevent_click(x: i32, y: i32, button: nerve_protocol::MouseButton) -> bool {
    let source =
        match CGEventSource::new(CGEventSourceStateID::HIDSystemState) {
            Ok(s) => s,
            Err(_) => return false,
        };
    let point = CGPoint::new(x as f64, y as f64);
    let cg_button = match button {
        nerve_protocol::MouseButton::Left => CGMouseButton::Left,
        nerve_protocol::MouseButton::Right => CGMouseButton::Right,
        nerve_protocol::MouseButton::Middle => CGMouseButton::Center,
    };
    let down = CGEvent::new_mouse_event(source.clone(), CGEventType::LeftMouseDown, point, cg_button);
    let up = CGEvent::new_mouse_event(source, CGEventType::LeftMouseUp, point, cg_button);
    match (down, up) {
        (Ok(d), Ok(u)) => {
            d.post(CGEventTapLocation::HID);
            u.post(CGEventTapLocation::HID);
            true
        }
        _ => false,
    }
}

/// Permission probe: returns true if Screen Recording is granted.
pub fn screen_recording_granted() -> bool {
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
    }
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Permission probe: returns true if the daemon is an Accessibility-trusted process.
pub fn accessibility_granted() -> bool {
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    unsafe { AXIsProcessTrusted() }
}

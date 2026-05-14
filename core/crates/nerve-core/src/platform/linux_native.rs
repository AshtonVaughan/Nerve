//! Linux native backend extensions.
//!
//! Compiled in only when `--features linux-atspi` is set. Wires AT-SPI 2 for
//! accessibility tree extraction. Wayland portals and uinput are tracked
//! separately because both touch sandbox / capability surfaces that require
//! per-OS install steps.

#![cfg(all(target_os = "linux", feature = "linux-atspi"))]

use nerve_protocol::{UiNode};

/// Walk the AT-SPI tree for the currently-focused application.
///
/// AT-SPI 2 is a D-Bus protocol; the `atspi` crate exposes async helpers
/// that we run on a dedicated tokio task. The walker keeps a depth/budget
/// cap so a pathological app can't lock the daemon.
pub async fn ax_tree() -> Vec<UiNode> {
    // The full implementation:
    //   1. atspi::AccessibilityConnection::new().await
    //   2. accessible.root_accessible() -> Accessible
    //   3. recursive `accessible.children()` until depth N or count cap
    //   4. for each: role(), name(), description(), accessible_id(),
    //      get_state() and bounding-box from text/component interfaces.
    //
    // For the MVP we return an empty Vec — the protocol layer surfaces this
    // honestly to clients via Capabilities.accessibility_tree.
    Vec::new()
}

# Platform backends

Nerve abstracts every OS-specific capability behind the
[`PlatformBackend`](../core/crates/nerve-core/src/platform/mod.rs) trait.
Each platform's module overrides individual methods where a native API
gives us a richer signal than the cross-platform crates.

The MVP ships a *portable backend* (`xcap` for screen capture, `enigo` for
input, `arboard` for clipboard) that every platform-specific backend
inherits from. The platform files explicitly call out which methods will
get a native upgrade after the MVP.

## macOS

| Capability            | MVP                          | Production target                                  |
| --------------------- | ---------------------------- | -------------------------------------------------- |
| Screen capture        | `xcap` (CG fallback)         | `ScreenCaptureKit` (`SCStream`, `SCDisplay`)       |
| Active window / list  | `xcap`                       | `CGWindowListCopyWindowInfo` + frontmost via `NSWorkspace` |
| Accessibility tree    | none                         | `AXUIElement` walked recursively from the frontmost app |
| Mouse / keyboard      | `enigo` (CG events)          | `CGEventCreateMouseEvent` / `CGEventCreateKeyboardEvent` |
| Permissions probe     | runtime call probe           | `CGPreflightScreenCaptureAccess`, `AXIsProcessTrustedWithOptions` |

The required entitlements once we ship a signed bundle:

* `com.apple.security.device.audio-input` — only if voice input is added.
* Permissions: Screen Recording, Accessibility, Automation (per target app).

The doctor command surfaces missing permissions with their exact System
Settings path so users don't have to guess.

## Windows

| Capability            | MVP                          | Production target                                  |
| --------------------- | ---------------------------- | -------------------------------------------------- |
| Screen capture        | `xcap` (GDI fallback)        | DXGI Desktop Duplication or Windows.Graphics.Capture |
| Active window         | `xcap`                       | `GetForegroundWindow` + `GetWindowText`            |
| Accessibility tree    | none                         | UI Automation (`windows-rs` crate)                 |
| Mouse / keyboard      | `enigo` (SendInput)          | `SendInput` direct + raw input where required      |
| Permissions probe     | none                         | Integrity-level / UIPI check via `GetTokenInformation` |

UIPI gotcha: a medium-IL daemon cannot send synthesized input to a high-IL
window (e.g. UAC dialog). Production builds should refuse to send input in
that case rather than silently no-op, and ask the user to run an elevated
companion service if the workflow requires it.

## Linux X11

| Capability            | MVP                          | Production target                                  |
| --------------------- | ---------------------------- | -------------------------------------------------- |
| Screen capture        | `xcap` (XShm)                | XShm + XComposite for layered windows               |
| Active window         | `xcap`                       | `_NET_ACTIVE_WINDOW` via `xcb`                     |
| Accessibility tree    | none                         | AT-SPI 2 via the `atspi` crate                     |
| Mouse / keyboard      | `enigo` (XTest)              | XTest direct, plus XInput2 for high-DPI            |

The portable backend already works fine on X11. The main upgrade is the
accessibility tree.

## Linux Wayland

Wayland is the hard one. Direct input synthesis is not part of the public
protocol — there is no equivalent of XTest. There are three viable
backstops:

1. **PipeWire screencast** via `xdg-desktop-portal` for screen capture.
   The portal requires user consent on first use; the daemon should remember
   the consent token across restarts.
2. **wl_data_device** for clipboard.
3. **uinput** for input: requires `CAP_SYS_ADMIN` or membership of the
   `input` group. The daemon should not silently fail when uinput is
   missing — it should report `input_control = false` in capabilities and
   refuse to dispatch input actions.

The `LinuxBackend` already flags `wayland_limited = true` and surfaces a
helpful hint via `missing_permissions` when running under Wayland. The
production roadmap is to wire portals first, then opt-in uinput second.

## Headless / containers

When the OS exposes no display server at all (CI hosts, sandboxes), the
backend marks screen capture as "disabled" after its first failure and
short-circuits subsequent calls. This is important because some host
environments take seconds to return from a failed X11 connect, and we
absolutely must not turn that into a per-observation cost.

In headless mode the daemon still:

* serves the WebSocket protocol normally,
* honours dry-run safety,
* writes audit logs,
* answers `get_capabilities` with realistic flags.

Headless mode is the recommended posture for CI-bound benchmarks and unit
tests of higher-level agent logic.

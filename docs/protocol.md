# Protocol

Every message between an SDK / CLI and the daemon is a JSON object on a
single WebSocket. Requests carry a `request_id`; responses echo it back
verbatim so the SDKs can resolve their pending futures.

The full Rust types live in
[`core/crates/nerve-protocol`](../core/crates/nerve-protocol/src/). The
sections below are the human-readable spec.

## Connection

* URL: `ws://127.0.0.1:8765/`
* Subprotocol: none required
* The daemon sends an unsolicited `hello` frame after the upgrade completes,
  carrying the protocol version, daemon version, host platform, and a
  connection id.

## Client → server

| `kind`                        | Purpose |
| ----------------------------- | ------- |
| `session_start`               | Begin a session, optionally with a custom `SafetyPolicy`. |
| `session_stop`                | End a session and flush its audit log. |
| `get_capabilities`            | What the daemon can do on this host. |
| `get_observation`             | One-shot observation snapshot. |
| `subscribe_observations`      | Stream observations at `interval_ms`. |
| `unsubscribe_observations`    | Stop the most recent subscription. |
| `execute_action`              | Run one action (low-level or semantic). |
| `execute_action_batch`        | Run a list of actions sequentially. |
| `get_action_log`              | Read the audit log for a session. |
| `replay_session`              | Re-emit a session's audit entries. |
| `set_safety_policy`           | Replace the active policy. |
| `emergency_stop`              | Engage the global stop. |
| `confirm_action`              | Approve / reject an action awaiting confirmation. |
| `ping`                        | Heartbeat — daemon echoes `pong` with the same nonce. |

## Server → client

| `kind`                  | Purpose |
| ----------------------- | ------- |
| `hello`                 | Sent once on connect. |
| `session_started`       | Reply to `session_start`. Carries `Capabilities`. |
| `session_stopped`       | Reply to `session_stop`. |
| `capabilities`          | Reply to `get_capabilities`. |
| `observation`           | Either reply to `get_observation` or a streamed item. |
| `action_result`         | Reply to `execute_action`. |
| `batch_result`          | Reply to `execute_action_batch`. |
| `action_log`            | Reply to `get_action_log`. |
| `policy_updated`        | Reply to `set_safety_policy` and `confirm_action`. |
| `emergency_stopped`     | Reply to `emergency_stop` and async broadcast event. |
| `confirmation_required` | Async event when an action awaits human approval. |
| `replay_progress`       | One entry as the daemon replays a session. |
| `replay_complete`       | Final replay envelope. |
| `pong`                  | Heartbeat reply. |
| `error`                 | Anything the daemon refused or could not process. |

## Action envelope

```json
{
  "id": "act_a32f5e",
  "action": { "type": "click", "x": 812, "y": 441, "button": "left" },
  "note": "optional free-form context"
}
```

The `action` field accepts both low-level primitives and semantic actions.
The daemon recognises which based on the `type` value.

### Low-level actions

| `type`               | Required fields |
| -------------------- | --------------- |
| `get_observation`    | optional `include_screenshot` |
| `screenshot`         | — |
| `move_mouse`         | `x`, `y` |
| `click`              | `x`, `y`, optional `button` |
| `double_click`       | `x`, `y` |
| `right_click`        | `x`, `y` |
| `drag`               | `from_x`, `from_y`, `to_x`, `to_y`, optional `button` |
| `scroll`             | `x`, `y`, `delta_x`, `delta_y` |
| `type_text`          | `text`, optional `delay_ms` |
| `key_press`          | `key` |
| `hotkey`             | `keys` (array) |
| `clipboard_get`      | — |
| `clipboard_set`      | `text` |
| `wait`               | `ms` |
| `emergency_stop`     | — |

### Semantic actions

| `type`                       | Required fields |
| ---------------------------- | --------------- |
| `click_element`              | `target` (text / role / app / bounds / index) |
| `click_element_by_text`      | `text`, optional `app` |
| `click_element_by_role`      | `role`, optional `app` |
| `press_button_named`         | `name`, optional `app` |
| `focus_window`               | optional `title`, optional `app` |
| `select_menu_item`           | `path` (array), optional `app` |
| `type_into_focused_element`  | `text` |
| `find_text_on_screen`        | `text` |
| `verify_text_present`        | `text`, optional `timeout_ms` |
| `verify_window_active`       | optional `app`, optional `title` |
| `wait_for_text`              | `text`, `timeout_ms` |
| `wait_for_window`            | optional `app`/`title`, `timeout_ms` |
| `close_window`               | optional `app`/`title` |
| `open_app`                   | `name` |

## Action result

```json
{
  "id": "act_a32f5e",
  "ok": true,
  "timestamp": "2026-05-14T08:00:00Z",
  "method": "accessibility_action",
  "cursor": { "x": 812, "y": 441 },
  "active_window": "Chrome",
  "error": null,
  "data": null,
  "screenshot_before": "sha256:...",
  "screenshot_after": "sha256:...",
  "compiled": {
    "method": "accessibility_action",
    "primitive": { "type": "click", "x": 812, "y": 441, "button": "left" },
    "attempted": ["accessibility_action"],
    "trace": ["accessibility match text=Save role=button bounds={...}"]
  }
}
```

`method` always reports the strategy that ultimately fired. For semantic
actions, `compiled` exposes the full ladder the [compiler](./architecture.md)
walked so audit logs are reproducible.

## Observation

See [`observation.rs`](../core/crates/nerve-protocol/src/observation.rs) for
the canonical schema. Notable points:

* `screen.screenshot_base64` is omitted when the caller asks for
  `include_screenshot=false` so streaming subscriptions stay cheap.
* `screen.screenshot_hash` is a SHA-256 of the PNG bytes, useful for
  diff-style change detection.
* `ui_tree` is an empty array when the active backend has no accessibility
  story. Future macOS / Windows / Linux backends populate it with
  `AXUIElement`, `UIAutomation`, or AT-SPI walks respectively.

## Capabilities

```json
{
  "platform": "macos",
  "screen_capture": true,
  "input_control": true,
  "accessibility_tree": true,
  "clipboard": true,
  "semantic_actions": true,
  "ocr": false,
  "wayland_limited": false,
  "missing_permissions": [],
  "backends": {
    "screen_capture": "xcap (TODO: ScreenCaptureKit)",
    "input": "enigo (TODO: CGEvent)",
    "accessibility": "none (TODO: AXUIElement)",
    "clipboard": "arboard"
  },
  "version": "0.1.0"
}
```

Agents should consult `capabilities` before issuing semantic actions —
the daemon falls back to coordinate clicks if the accessibility tree is
missing, but a hint in the model prompt is cheaper than a fallback.

## Versioning

`protocol_version` is sent in the `hello` frame. Breaking changes will bump
the major component; minor bumps add new optional fields only.

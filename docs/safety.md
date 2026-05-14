# Safety

Nerve treats every action as a *potentially consequential side effect*.
The safety layer is part of the action path — not a wrapper around it —
which means there is no way for an SDK to "bypass" it. Even direct CLI
calls and replays go through the same engine.

## The five decisions

Every action receives a [`SafetyDecision`](../core/crates/nerve-protocol/src/action.rs):

| Decision           | Effect |
| ------------------ | ------ |
| `allowed`          | Executes immediately. |
| `dry_run`          | Logs and returns success but performs no OS call. |
| `confirmed`        | Blocks until a `confirm_action` arrives. |
| `blocked`          | Rejected outright (allow/blocklist, takeover). |
| `rate_limited`     | Rejected because the per-minute limit was hit. |
| `emergency_stopped`| Rejected; daemon is in stop-the-world. |

The decision is stored in every `AuditEntry` so reviews can answer
"what did the safety layer think when this happened?" with a single grep.

## Policy

[`SafetyPolicy`](../core/crates/nerve-protocol/src/policy.rs) is set per
session. Callers can ship a policy with `session_start` or update it later
with `set_safety_policy`. The default is conservative:

* `dry_run = false`
* `require_confirmation = false`
* `human_takeover = false`
* `max_actions_per_minute = 600`
* `block_password_fields = true`
* `confirm_payment_fields = true`

Field-detection (password / payment) heuristics are placeholder hooks today
— they read from accessibility/UI metadata once those backends are wired.
The plumbing is in place so the rest of the runtime doesn't need to change
when the heuristic lands.

## Allow / block lists

Both lists match against the active window's `app_name`. The match is
case-insensitive. When `app_allowlist` is non-empty, **only** apps on the
list can be touched — this is the killer feature for an agent that should
not be allowed to wander out of "Chrome".

## Rate limiting

A rolling 60-second window enforces `max_actions_per_minute`. The limit is
shared across all actions in a session, not per-app. Hitting it returns
`rate_limited`; the SDK should treat it as a soft retry signal.

## Emergency stop

`emergency_stop` engages a global flag that:

* causes every future `evaluate()` to return `emergency_stopped`,
* fires a broadcast event so every connected client receives an
  unsolicited `emergency_stopped` notification,
* persists for the lifetime of the daemon process (restart to clear).

The CLI command `nerve emergency-stop` is the recommended panic button.
The dashboard exposes the same button at the top right.

## Redaction

The `Redactor` runs over text payloads before they hit the audit log. The
default ruleset matches:

* OpenAI / Anthropic / GitHub / Stripe live keys
* AWS access keys
* generic long-hex private keys (>= 40 chars)
* loose BIP-39-style seed phrases (12+ word runs)

User patterns can be added via `policy.redact_patterns`. Anything matched
is replaced with `<REDACTED>` before persistence.

## Confirmation flow

When `require_confirmation = true`, mutating actions return
`SafetyDecision::Confirmed` and the daemon sends a `confirmation_required`
event. The client can:

* approve with `confirm_action { allow: true }` — the action proceeds,
* deny with `confirm_action { allow: false }` — the action is rejected,
* let the 30-second timer expire — the action is rejected by default.

The dashboard listens for these events and prompts the user. SDKs that run
fully autonomous agents should treat them as "stop the loop, surface to the
operator" signals.

## Audit log

Every action — regardless of decision — appends one JSON line to
`<log_dir>/<session_id>.jsonl`. Fields are documented in
[`action.rs`](../core/crates/nerve-protocol/src/action.rs). Highlights:

* `safety_decision` — the verdict the engine returned.
* `screenshot_before` / `screenshot_after` — SHA-256 of the PNG bytes the
  daemon saw immediately before and after the action.
* `active_window_before` / `active_window_after` — what the OS reported as
  the foreground app on either side of the call.
* `compiled` — for semantic actions, the full lowering ladder.

The log is append-only. The daemon never overwrites entries; a corrupted
line is logged as a warning and skipped on read.

## Replay

`replay_session` walks the audit log of a previous session and re-emits
each entry to the client. Replay does *not* re-execute the action; it
exists so the dashboard, a CI run, or a human reviewer can step through
exactly what happened. Re-execution is left as an explicit opt-in
("replay-and-redo") so a sensitive log isn't accidentally re-run against
production.

## Tests

`nerve-core` ships unit tests for:

* the redactor's default and user-supplied patterns
  (`safety::redact::tests`),
* the audit log's append + read round trip (`audit::tests`).

Both run via `cargo test --workspace`.

# Privacy policy

Nerve is a local daemon that intentionally has access to your entire desktop
session. This document explains exactly what data Nerve sees, where it goes,
and how to remove it.

## What Nerve collects (by default)

* **Screenshots.** When an agent requests an observation that includes a
  screenshot, the daemon captures the primary monitor and stores the PNG
  bytes in the in-memory observation. By default, only the SHA-256 hash is
  persisted (to the audit log); the PNG itself is not written to disk.
* **Cursor position and active window metadata** — title, app name, process
  name, bounds, pid. These appear in observations and audit entries.
* **Accessibility tree and OCR results** — only when the platform backend
  produces them. Strings inside tree nodes are NOT redacted automatically;
  callers are responsible for not asking the agent to read sensitive UI.
* **Audit log entries** — every action, including the raw inputs (e.g. the
  `text` field of a `type_text` action) and the safety decision. The
  redactor runs over caller-supplied `note` fields and over typed text
  matching the default + user-supplied patterns.
* **Clipboard contents** — when `clipboard_get` runs, the contents pass
  through the daemon and may end up in observations and audit logs.

## What Nerve does NOT collect

* Nerve does not phone home. The daemon binds to `127.0.0.1` by default and
  has no outbound network calls.
* The crash-report opt-in is **off** unless `telemetry.crash_reports = true`
  is set in the config and you have wired a reporting endpoint.
* The Prometheus metrics exporter is local-only and records counts and
  latencies, never action payloads.

## Where Nerve stores data

| Item | Default path |
| ---- | ------------ |
| Audit logs | `~/.local/share/nerve/logs/<session>.jsonl` (Linux) |
| | `~/Library/Application Support/nerve/logs/...` (macOS) |
| | `%LOCALAPPDATA%\nerve\logs\...` (Windows) |
| Config | `~/.config/nerve/config.toml` (Linux) |
| | `~/Library/Application Support/nerve/config.toml` (macOS) |
| | `%APPDATA%\nerve\config.toml` (Windows) |
| Auth token | inside the config file |
| Rolled audit shards | next to the live log, gzipped if `audit.compress_rolled = true` |

## Encryption at rest

The MVP does **not** encrypt audit logs at rest. If the host has full-disk
encryption (FileVault, BitLocker, LUKS), the logs inherit that protection.
A roadmap item adds per-session AEAD encryption using a key derived from the
auth token.

## Redaction

The bundled redactor scrubs:

* OpenAI / Anthropic / GitHub / Stripe live keys.
* AWS access keys.
* Hex-encoded private keys of 40 chars or more.
* Loose BIP-39-style 12 / 24 word seed phrase candidates.

Add custom regexes via `policy.redact_patterns`. Anything matched is
replaced with `<REDACTED>` before the entry is written.

## Deleting your data

* `nerve logs --session <id>` lists the entries for a session.
* `rm <log_dir>/<session>.jsonl*` deletes a session entirely.
* `nerve token clear` removes the auth token from the config.
* Uninstall (`packaging/macos/build-pkg.sh` undo, `apt remove nerve`, etc.)
  removes the binary but leaves logs / config in place. Delete those
  directories manually if you want a clean slate.

## Third-party data

When an agent driving Nerve calls a model provider (OpenAI, Anthropic,
Gemini, …), the screenshots and prompts the agent sends are subject to
**that provider's** privacy policy, not Nerve's. Nerve never proxies model
calls — your agent talks to the model directly.

## Reporting concerns

Email `security@nerve.dev` (or open a GitHub issue if non-sensitive).

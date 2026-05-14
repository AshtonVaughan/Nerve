# Architecture

Nerve is a local-first execution runtime for computer-use agents. The system
is organised around a single long-lived daemon that exposes a stable
WebSocket protocol; everything else — SDKs, CLI, dashboard, demo agents,
benchmarks — is a client of that daemon.

```
                ┌─────────────────────────────────────────────────────────────┐
                │                       Nerve daemon                          │
                │                                                             │
   WebSocket    │   ┌────────────┐  ┌──────────────┐  ┌──────────────────┐    │
   :8765        │   │  Server    │  │   Executor   │  │  Semantic action │    │
   ─────────►   │   │  (axum)    │──►  + safety    │──►  compiler        │    │
                │   └────────────┘  └──────────────┘  └──────────────────┘    │
                │         │                │                  │               │
                │         ▼                ▼                  ▼               │
                │   ┌────────────┐  ┌──────────────┐  ┌──────────────────┐    │
                │   │ Sessions / │  │ Audit log    │  │ Platform backend │    │
                │   │ subscriber │  │ (JSONL)      │  │ (macOS / Win /   │    │
                │   │  registry  │  └──────────────┘  │  Linux X11 / WL) │    │
                │   └────────────┘                    └──────────────────┘    │
                │                                                             │
                └─────────────────────────────────────────────────────────────┘
                          ▲                       ▲                ▲
                          │                       │                │
                ┌─────────┴────────┐   ┌──────────┴───────┐   ┌────┴──────┐
                │  Python / TS     │   │  Dashboard       │   │  CLI       │
                │  SDK             │   │  (live HTML)     │   │  (nerve)   │
                └──────────────────┘   └──────────────────┘   └────────────┘
                          ▲
                          │
                ┌─────────┴────────┐
                │  Model adapters  │
                │  (Claude / GPT / │
                │   Gemini / local)│
                └──────────────────┘
```

## Crates

| Crate              | Role |
| ------------------ | ---- |
| `nerve-protocol`   | Wire types: actions, observations, capabilities, policies, WS envelopes. Pure `serde`. |
| `nerve-core`       | Daemon library: WebSocket server, platform backends, action executor, semantic compiler, safety engine, audit log. |
| `nerve-cli`        | The `nerve` binary. Hosts the daemon and ships every client subcommand. |

## Layering

1. **Protocol** is the single source of truth. SDKs and CLI talk to the
   daemon over JSON; types are shared via the `nerve-protocol` crate so the
   Rust pieces never drift.
2. **Platform backend** is a trait (`PlatformBackend`) with one
   implementation per OS. The portable backend (`xcap` + `enigo` +
   `arboard`) is the substrate; native backends override individual methods
   where a richer OS API exists.
3. **Executor** sits between the WebSocket layer and the platform backend.
   For each action it (a) consults the safety engine, (b) compiles semantic
   actions to concrete primitives, (c) snapshots before/after state, (d)
   writes an audit entry, (e) returns the result.
4. **Server** is `axum`-based: HTTP for the dashboard, WebSocket for the
   protocol. Both live on the same port so the daemon needs a single OS
   permission grant.

## Threading model

* The daemon runs a multi-threaded Tokio runtime. Every WebSocket connection
  is one task; subscriptions are independent child tasks that exit when
  their backing channel closes.
* Blocking input/screen-capture calls (e.g. `enigo`, `xcap`) are wrapped in
  `tokio::task::spawn_blocking` / `tokio::task::block_in_place` so they
  don't starve other workers.
* Failed platform-backend initialisations are *sticky* — once the backend
  reports that screen capture or input is unavailable, we cache the failure
  and short-circuit subsequent calls. This is what keeps the daemon
  responsive on headless CI machines and unprivileged containers.

## Safety boundary

The safety engine inspects every action before execution:

* emergency stop and human takeover short-circuit everything
* allow / blocklists are matched against the reported app
* a rolling rate limit caps action frequency
* `dry_run` and `require_confirmation` return no-op / await-confirm results
* on every action the engine produces a `SafetyDecision` that is recorded
  alongside the action in the audit log

This means every consequential side effect is gated by, *and explainable
through*, the same engine.

## Audit and replay

Each session writes one JSON-lines file at
`<log_dir>/<session_id>.jsonl`. Each line is one `AuditEntry`. Replay is
implemented as a tight loop that re-emits these entries to the client; on
the server side we simply read the file back in order. That keeps replay
trivially testable and lets external tooling (jq, sqlite-utils, etc) work
directly with the logs.

## Why a daemon, not a library

The daemon model gives us three things that an in-process library could not:

1. **One permission grant.** macOS Accessibility / Screen Recording, Windows
   integrity-level promotion, AT-SPI on Linux — these are per-process
   permissions that the user grants once to the daemon.
2. **Multi-client.** The dashboard, the CLI, and an agent SDK can all attach
   to the same session.
3. **Persistent observation stream.** Compared to the snapshot-per-call
   shape of raw CUA loops, a long-lived daemon can stream observations
   without re-grabbing OS handles each step.

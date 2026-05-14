# Threat model

Nerve is a local daemon with full input and screen-capture access on the
host. That access surface makes it a high-value target. This document
enumerates the threats we explicitly defend against, the ones we accept
(with mitigations), and the ones that are out of scope.

## Trust boundaries

```
+-----------+        +--------------+         +-----------+
|  Client   |  WS    |  Nerve       |  OS     |  Desktop  |
|  (SDK /   | <----> |  daemon      | <-----> |  apps,    |
|  CLI /    |        |  (local user |         |  files,   |
|  agent)   |        |   privilege) |         |  network) |
+-----------+        +--------------+         +-----------+
```

The daemon runs at the local user's privilege level. It never elevates,
never escalates, never opens an outbound socket of its own.

## In-scope threats

### T1 — Hostile client trying to bypass the safety policy

* **Mitigation:** every action — including replays and idempotent retries —
  passes through the safety engine on the daemon side. The SDK has no API
  surface that skips it.

### T2 — Hostile network attacker on the same machine

* **Mitigation:** default bind is `127.0.0.1`. Non-loopback binds require
  `loopback_only = false` plus TLS plus an auth token.

### T3 — Hostile network attacker on another machine (when bound non-loopback)

* **Mitigation:** TLS via rustls (auto-generated self-signed cert on first
  launch unless an operator supplies a real one), constant-time auth-token
  compare, version negotiation refuses incompatible clients.

### T4 — Local user reading the audit log

* **Mitigation:** the redactor scrubs known secret shapes before write.
  Operators can add custom patterns. Full-disk encryption is assumed.
  Per-log AEAD is on the roadmap.

### T5 — Action floods or runaway agents

* **Mitigation:** rate limit (default 600 actions/min), emergency stop,
  session timeout, allow / blocklists. Subscription streams use
  `try_send` so a malicious client cannot exhaust the daemon's memory by
  refusing to read.

### T6 — Replay-after-leak (an old auth token still works)

* **Mitigation:** `nerve token rotate` regenerates a 240-bit base32 token
  and writes it back to the config file. Connected clients are not
  invalidated mid-session; restart the daemon to force re-auth.

## Accepted residual risks

### R1 — A compromised agent runs arbitrary actions on the host

This is not a bug — it is the entire premise of computer-use. We rely on
the safety policy + audit log + emergency stop as the failure-recovery
loop. Operators must scope the policy to the smallest app surface the agent
needs.

### R2 — Screen contents leak through the model

Sending screenshots to a third-party model is a privacy decision the agent
caller makes, not Nerve. The runtime never proxies model calls.

### R3 — Permission grants don't survive uninstall + reinstall on macOS

This is a macOS limitation: re-installing changes the bundle code-signing
identity, requiring the user to re-grant Screen Recording / Accessibility.
The doctor command surfaces this with a clear hint.

## Out of scope

* **Local-root attacker.** A local user with full sudo can read the daemon's
  memory, replace the binary, or simulate input at the kernel level. Nerve
  is not a sandbox.
* **Hardware-level keyloggers / screen recorders.** Out of scope.
* **Supply-chain attacks on `cargo`/`npm`/`pip` mirrors.** We pin and
  ship `Cargo.lock` + `package-lock.json` to reduce the surface, but a
  malicious mirror that serves a tampered crate is the OS / package manager's
  responsibility, not Nerve's.

## Reporting vulnerabilities

`security@nerve.dev` for sensitive reports. We aim to triage within 72
hours. See [`SECURITY.md`](../SECURITY.md) for the full policy.

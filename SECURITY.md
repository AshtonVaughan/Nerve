# Security policy

## Supported versions

The latest release on `main` is supported. We currently do not maintain a
back-port lane.

## Reporting a vulnerability

Email `security@nerve.dev` with:

* A short description of the issue.
* A minimal reproduction.
* The git SHA you tested against.

We acknowledge reports within **72 hours** and aim for a fix or
public-disclosure decision within **30 days** depending on severity.

Please do not file public GitHub issues for sensitive reports.

## Scope

In scope:

* Authentication or transport-security bypass in the daemon's
  HTTP / WebSocket protocol.
* Safety-policy bypass (e.g. an action that should be `blocked` reaching the
  platform backend).
* Audit log integrity issues — entries that should appear but don't, or
  payloads that should be redacted but aren't.
* Memory unsafety in `nerve-core`.

Out of scope:

* Issues that require local root / Administrator on the host.
* Hardware-level attacks.
* Vulnerabilities in third-party crates we depend on (please report those
  upstream and we'll bump the version).

## Coordinated disclosure

Once a fix is ready we credit reporters in the release notes, unless they
ask to remain anonymous.

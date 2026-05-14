# ADR 0001 — Daemon, not library

Date: 2026-05-14
Status: accepted

## Context

Every existing computer-use loop today is built as either (a) a library
embedded in the agent process or (b) an MCP-style stdio bridge. Both leak
the cost of OS-level permission grants and capture lifecycle into every
agent invocation.

## Decision

Nerve ships as a long-lived local daemon that exposes a WebSocket protocol.
Agents, the dashboard, the CLI, and benchmarks are all clients.

## Consequences

* +1 permission grant per machine instead of per-agent.
* Multiple clients can attach to the same session.
* Persistent capture means observations can stream rather than poll.
* We pay the cost of a separate process: install, autostart, audit log
  durability, IPC overhead.
* Some debugging is harder because agents can no longer set a breakpoint
  inside the runtime.

The trade favours acquirability: enterprise security teams can audit one
binary on disk, one open port, one set of permission grants.

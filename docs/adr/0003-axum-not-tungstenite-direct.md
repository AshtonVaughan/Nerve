# ADR 0003 — axum for the WS+HTTP server

Date: 2026-05-14
Status: accepted

## Context

The first MVP rolled its own HTTP / WebSocket multiplexer on top of
`tokio-tungstenite`. The peek-and-route logic was subtle enough that it
shipped a CLOSE_WAIT leak and a real-world hang during stress runs.

## Decision

Swap the WS layer to `axum 0.7`. axum already handles WS upgrades inline
with the rest of the router, gives us request/response middleware, and
makes adding new HTTP endpoints (Prometheus, replay viewer, etc.) a
one-liner.

## Consequences

* +1 dependency (axum), already pulled in by half the Rust web ecosystem.
* Reduced code surface in `nerve-core::server`.
* Per-request middleware is cheap to add when we wire telemetry / auth
  middleware later.

## Notes

We considered `actix-web` and rolling our own with `hyper`. axum's
ergonomics + the fact that it's `tokio` native make it the obvious choice
for a daemon already on tokio.

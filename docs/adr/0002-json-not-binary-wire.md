# ADR 0002 — JSON wire protocol

Date: 2026-05-14
Status: accepted

## Context

Streaming screenshots over a WebSocket with base64-encoded JSON costs ~33%
more bandwidth than a binary frame. The screenshots are 2-4 MB each.

## Decision

Stay with JSON for the MVP. Use SHA-256 hashes + delta-tile bounds to keep
streaming cheap. Add `compression: deflate` to the WebSocket once an
operator hits the bandwidth wall.

## Consequences

* SDKs in any language with `serde_json` / `JSON.parse` work out of the box.
* The audit log is human-readable JSONL — `jq`, `sqlite-utils`, `grep`
  remain useful tools.
* We pay 33% extra bandwidth on full-frame screenshots until compression
  ships. The dashboard uses delta frames + a separate cursor-only stream
  to keep the steady-state cost much lower.

A binary path (protobuf or msgpack) is plausible later; the cost is one
extra schema to keep in sync. We deliberately don't bake that in today.

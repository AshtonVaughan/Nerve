# API reference

The wire protocol and SDKs share one source of truth: the Rust types in
`core/crates/nerve-protocol`. Generate the live API reference with:

```bash
cd core
cargo doc --no-deps --workspace --open
```

The HTML output covers every public type, including every field of
`Observation`, `ActionEnvelope`, `SafetyPolicy`, `Capabilities`, etc.

This document covers the parts that don't live in rustdoc — overall
shape, naming conventions, and forwards-compatibility rules.

## Naming

| Concept | Convention |
|---------|-----------|
| Message envelope `kind` | `snake_case` |
| Field names | `snake_case` (matches Rust serde defaults) |
| Action `type` | `snake_case` (`click`, `type_text`, `click_element_by_text`) |
| Error `code` | enum-style `snake_case` (`auth_required`, `version_mismatch`) |
| Capability flags | `snake_case` booleans, default `false` when missing |

## Stability

* **Stable types** (`ClientMessage`, `ServerMessage`, `ActionEnvelope`,
  `ActionResult`, `Observation`, `Capabilities`, `SafetyPolicy`,
  `AuditEntry`, `ErrorCode`): adding new optional fields is allowed in any
  minor release; renaming or removing a field is a major-bump breaking
  change.
* **Experimental types** (the new feature-gated platform extras): subject
  to change in 0.x.

## Versioning

* `protocol_version_struct` is the source of truth (semver triple).
* `protocol_version` is the legacy string field kept for the original
  0.1.0 SDKs. New SDKs should read the struct version.
* The compatibility check (`ProtocolVersion::compatible_with`) refuses
  cross-major and, while we're still 0.x, cross-minor versions.

## SDK conventions

* All Python `*Dict` types in `nerve.typed` are `TypedDict` — they are
  enforced by `mypy --strict`, not at runtime.
* TypeScript types in `@nerve/sdk` are exact (no index signatures).
* Both SDKs treat unknown fields in incoming messages as ignorable, so
  newer daemons can ship with older SDKs in the field.

## Where to look

| Question | Answer |
|---------|--------|
| What's the exact JSON for a `click_element_by_text` action? | [`docs/protocol.md`](protocol.md) |
| Which fields does the audit log capture? | [`core/crates/nerve-protocol/src/action.rs`](../core/crates/nerve-protocol/src/action.rs) |
| Which methods does the SDK expose? | [`sdks/python/nerve/async_client.py`](../sdks/python/nerve/async_client.py) |
| Which actions does the safety engine treat as "read only"? | [`core/crates/nerve-core/src/safety/mod.rs`](../core/crates/nerve-core/src/safety/mod.rs) |

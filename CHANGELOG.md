# Changelog

All notable changes to Nerve land here. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning: [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

* Stable `ErrorCode` enum + retryable / retry_after_ms metadata on every
  `ServerMessage::Error` so SDKs can match codes, not strings.
* `ProtocolVersion` with semver-aware compatibility; daemon refuses
  mismatched clients during `session_start`.
* `auth_token` enforcement in `session_start` with constant-time compare.
  CLI: `nerve token rotate / show / clear`.
* `ActionEnvelope.idempotency_key` + per-session LRU cache. Retries of the
  same logical action replay the cached `ActionResult` instead of
  re-executing.
* `cursor_only = true` subscription mode at ~60Hz — drives the silky
  cursor pip on the dashboard.
* `delta_frames = true` subscription mode attaches dirty-tile bounds so
  clients can decide what regions to refresh.
* Multi-monitor descriptor (`Observation.screen.monitors`).
* Bundled Tesseract-backed OCR behind the `ocr-tesseract` feature.
* macOS / Windows / Linux native backend stubs (feature-gated) for AX +
  UIA + AT-SPI integration paths.
* Real Python adapters for OpenAI Computer Use, Anthropic Computer Use,
  Gemini, Ollama, vLLM (replacing the previous `NotImplementedError`
  placeholders).
* TLS support via rustls with `auto_self_signed` opt-in for non-loopback
  binds.
* TOML config (`~/.config/nerve/config.toml`) resolved from
  `--config / $NERVE_CONFIG / platform default`.
* Audit log rotation (size-based, gzip-compressed shards, configurable
  retention).
* Prometheus exporter at `GET /metrics`, plus counters / histograms /
  gauges sprinkled through the WS server.
* CLI subcommands: `config show/init/path`, `token rotate/show/clear`,
  `service install/uninstall/status`, `completion <shell>`,
  `start --daemonize --pid-file`.
* `nerve-core::tls` self-signed cert helper.
* Subscription backpressure: `try_send` so a slow client cannot block the
  daemon.
* GitHub Actions CI matrix (Ubuntu / macOS / Windows), release workflow
  that builds per-target archives.
* cargo-fuzz target for the JSON wire protocol.
* Load test (`benchmarks/harness/load.py`) reporting p50/p95/p99 latency
  across N concurrent WS clients.
* Playwright dashboard smoke test.
* Dashboard: auth-token entry, policy editor, replay viewer, dirty-tile
  overlay.
* Compliance docs: privacy policy, threat model, contributing guide,
  security policy.
* Strict Python `TypedDict` views of every wire type in `nerve.typed`.
* Browser CDP adapter scaffold (`nerve-core::browser`) the compiler will
  consult ahead of OCR / coordinate fallbacks.
* Unicode / IME-aware text typing via the new `unicode_paste` flag on
  `type_text`. The semantic compiler auto-enables it for non-ASCII text.
* Headless detection: on Unix hosts without DISPLAY / WAYLAND_DISPLAY,
  Nerve refuses to attempt screen capture / input rather than hanging.

### Changed

* `WsServer` migrated from a hand-rolled HTTP/WS multiplexer to `axum`.
* `Observation.screen` gained `monitors`, `screenshot_hash`, and
  `dirty_tiles`.
* All `ServerMessage::Error { code: String }` sites updated to use the new
  `ServerMessage::error(...)` helper.

### Fixed

* Lazy enigo initialisation no longer retries on every action when the
  first attempt failed.
* CLOSE_WAIT leak on disconnect — handle_socket now drops its WS sink
  before tearing down channels.

## [0.1.0] - 2026-05-14

### Added

* Initial MVP: Rust workspace (protocol / core / cli), Python SDK,
  TypeScript SDK, web dashboard, mock + placeholder model adapters,
  benchmark harness, documentation.

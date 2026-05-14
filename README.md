# Nerve

> **Nerve is the real-time control layer that gives AI agents a body.**

Nerve is a cross-platform, real-time computer-use execution runtime for AI
agents. It is the missing native layer underneath OpenAI CUA, Anthropic
Computer Use, and every "let the model use the computer" workflow you have
seen demoed in the last 18 months.

* **Native daemon.** A single Rust process running locally — `SendInput`,
  `CGEvent`, XTest, no remote IPC.
* **Persistent control plane.** WebSocket protocol with structured
  observations, semantic actions, audit, replay, safety policies, and
  emergency stop.
* **Hybrid perception.** Screenshots when needed, accessibility tree where
  available, OCR as a fallback — exposed through one observation object.
* **Action compiler.** Semantic actions (`click "Save" button in TextEdit`)
  are lowered through an explicit ladder: AX → native UI → browser DOM →
  OCR → coordinate fallback. Every decision is logged for replay.
* **Model-agnostic.** The model is the brain, Nerve is the body. The
  reference Python and TypeScript SDKs include adapter slots for
  OpenAI / Anthropic / Gemini / Ollama / vLLM; the mock agent ships built-in.

## What's here

```
core/                       # Rust workspace — daemon, CLI, protocol
  crates/nerve-protocol/    # Shared JSON / WebSocket types
  crates/nerve-core/        # Daemon library
  crates/nerve-cli/         # `nerve` binary
sdks/python/                # Python SDK (sync + asyncio)
sdks/typescript/            # TypeScript / JavaScript SDK
agents/                     # Model adapters + the mock + the demo
dashboard/                  # Local web dashboard (served by the daemon)
benchmarks/                 # Harness + canonical local tasks
docs/                       # Architecture, protocol, safety, etc.
```

## Quickstart

```bash
# 1. Build the daemon.
cd core && cargo build --release

# 2. Start it. macOS / Linux:
./target/release/nerve start
# Windows:
.\target\release\nerve.exe start

# 3. Open the dashboard.
open http://127.0.0.1:8765/

# 4. Drive it from Python.
pip install -e ./sdks/python
python -c "from nerve import NerveClient; c = NerveClient(); c.connect(); print(c.get_capabilities().platform); c.stop()"

# 5. Drive it from TypeScript.
cd sdks/typescript && npm install && npm run build
node -e "import('./dist/index.js').then(async ({NerveClient}) => { const c = new NerveClient(); await c.connect(); console.log((await c.getObservation()).platform); await c.stop(); })"
```

Full setup notes per OS are in [`docs/quickstart.md`](./docs/quickstart.md).

## Why a daemon, not a library

| Problem | What an in-process library does | What Nerve does |
| ------- | ------------------------------- | --------------- |
| Permissions | Each agent process needs its own Screen Recording grant. | Granted once to the daemon. |
| Multi-client | Only the calling process can see what is going on. | Dashboard + SDK + CLI can attach concurrently. |
| Observation cost | Each step re-grabs OS handles. | Persistent capture + streamed observations. |
| Auditability | DIY logging per agent. | One canonical JSONL log; same replay everywhere. |
| Safety | Per-agent guardrails. | One enforcement point: dry-run, allowlist, emergency stop. |

See [`docs/competitive-positioning.md`](./docs/competitive-positioning.md)
for the head-to-head against raw CUA / Computer Use loops.

## Status

This is the **MVP**. The wire protocol, SDKs, CLI, dashboard, demo, and
benchmark harness are real and pass `cargo test` + the local smoke test.
The platform backends ship a portable substrate that works on every OS;
*native upgrades* (ScreenCaptureKit, UI Automation, AT-SPI) are sketched
out in the per-platform modules with concrete entry points.

### Current limitations

* Screen capture, input, and clipboard go through `xcap`, `enigo`, and
  `arboard` respectively. Native APIs are next on the roadmap (see
  [`docs/platform-backends.md`](./docs/platform-backends.md)).
* Accessibility tree extraction is stubbed: the compiler can use a tree
  when one is supplied, but no backend supplies one yet.
* OCR is not bundled — the compiler returns a screenshot for caller-side
  OCR (Tesseract / EasyOCR / cloud).
* The OpenAI / Anthropic / Gemini / Ollama / vLLM adapters are
  placeholders that fail loud with a clear error message until they are
  wired.
* On Wayland, input is best-effort: the daemon advertises
  `wayland_limited = true` and the doctor command surfaces the consent path.

## What ships next

1. ScreenCaptureKit on macOS + UI Automation on Windows + AT-SPI on Linux.
2. Real OpenAI CUA + Anthropic Computer Use adapters.
3. Built-in OCR (Tesseract via `leptess`) so semantic verification can
   close the loop without the agent paying for it.
4. Multi-monitor support and a dedicated cursor-only observation stream
   for silky 120 fps dashboards.
5. Hardened Wayland input path via `uinput` (group-gated) and PipeWire
   capture via xdg-desktop-portal.

## License

Apache-2.0. See [`LICENSE`](./LICENSE).

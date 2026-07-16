# Nerve

> [!CAUTION]`n> **This project no longer works and is not maintained.** It is retained only as an experimental reference and should not be treated as usable software.


A hackable Rust daemon for driving a desktop computer over a WebSocket
protocol. Mouse, keyboard, clipboard, screenshot, accessibility tree -
exposed as a single local service that any process (Python, TypeScript,
CLI, browser dashboard, AI agent) can attach to.

It is not trying to be Claude Code, Anthropic Cowork, UI-TARS Desktop, or
Apple Intelligence. It is the layer underneath those things: a working
reference implementation of a cross-platform computer-use runtime, with
audit logs, safety policies, multi-client fan-out, and an emergency-stop
button - so you don't have to write that part yourself.

## What you can use it for today

- **Drive a desktop app from Python or TypeScript test code** with one
  install instead of stitching together `pyautogui` + screenshot libs +
  per-OS hacks.
- **Add computer-use to your own AI agent or tool** without re-implementing
  `SendInput`, UIA trees, screen capture, and a dry-run / audit / safety
  layer for every platform.
- **Read the source as a reference** if you're building anything in this
  space - the daemon split, action ladder, prompt-cached LLM adapter,
  cassette benchmarks, and per-platform scaffolding are all worked
  examples.

## Status

| Layer | State |
|---|---|
| Rust daemon, WebSocket protocol, CLI, dashboard | Working |
| Audit log, safety policies, dry-run, emergency stop, rate limits | Working |
| Python SDK (sync + asyncio), TypeScript SDK | Working |
| Windows native: UIA tree walk, `SendInput` Unicode input | Working |
| Anthropic Computer Use adapter (`computer_20250124`, full `tool_result` loop) | Working end-to-end |
| OpenAI CUA adapter | Code shipped, not tested live |
| macOS native (ScreenCaptureKit + AX + CGEvent + permissions) | Compiles, not tested by author |
| Linux native (X11 + Wayland + AT-SPI + uinput) | Compiles, not tested by author |
| Tesseract OCR (`--features ocr-tesseract`) | Compiles, needs libtesseract on host |
| Chrome DevTools Protocol bridge (`--features browser-cdp`) | Compiles, not tested live |
| 26-test suite (unit + compiler ladder + integration) | Passing |
| Cassette-replayed benchmarks (8 tasks) | Passing |

The Windows path is the verified golden path. Cross-platform code exists
for macOS and Linux but the author only ships and tests on Windows. PRs
welcome from anyone who wants to validate or harden the other platforms.

## Quickstart

```bash
# 1. Build the daemon (requires stable Rust)
cd core && cargo build --release

# 2. Start it
./target/release/nerve start              # macOS/Linux
.\target\release\nerve.exe start          # Windows

# 3. Open the dashboard
http://127.0.0.1:8765/

# 4. Drive it from Python
pip install -e ./sdks/python
python -c "from nerve import NerveClient; c = NerveClient(); c.connect(); print(c.get_capabilities().platform); c.stop()"

# 5. Stop the daemon cleanly
./target/release/nerve stop
```

Full per-OS setup notes: [`docs/quickstart.md`](./docs/quickstart.md).

## Architecture in one diagram

```
   Python SDK ─┐
TypeScript SDK ─┤
  nerve CLI    ─┼──► WebSocket :8765 ──► nerve daemon (Rust)
  Dashboard    ─┤        (audit + safety + dry-run)
  AI adapter   ─┘                  │
                                   ├── platform backend
                                   │   (UIA / SendInput on Win,
                                   │    AX / CGEvent on Mac,
                                   │    AT-SPI / uinput on Linux,
                                   │    xcap / enigo / arboard fallback)
                                   │
                                   └── action compiler
                                       (semantic → AX → bounds → OCR
                                        → ElementNotFound)
```

## Why a daemon, not a library

| Problem | In-process library | Nerve |
| ------- | ------------------ | ----- |
| OS permissions | Each agent re-grants Screen Recording + Accessibility. | Granted once to the daemon. |
| Multi-client | Only the calling process sees what's happening. | Dashboard + SDK + CLI + agent all attach concurrently. |
| Observation cost | Each step re-grabs OS handles. | Persistent capture + streamed observations. |
| Auditability | DIY logging per agent. | One canonical JSONL log; same replay everywhere. |
| Safety | Per-agent guardrails. | One enforcement point: dry-run, allowlist, emergency stop. |

## What ships in the repo

```
core/                       # Rust workspace - daemon, CLI, protocol
  crates/nerve-protocol/    # Wire JSON / WebSocket types
  crates/nerve-core/        # Daemon library
  crates/nerve-cli/         # `nerve` binary
sdks/python/                # Python SDK (sync + asyncio + typed)
sdks/typescript/            # TypeScript / JavaScript SDK
agents/                     # Reference adapters (mock, anthropic, openai, demo)
dashboard/                  # Local web dashboard (served by the daemon)
benchmarks/                 # Harness, 8 canonical tasks, cassettes
docs/                       # Architecture, protocol, safety, ADRs, threat model
packaging/                  # Distro recipes (deb, msi, homebrew, macos pkg)
scripts/smoke.sh            # End-to-end smoke test
```

## Honest limitations

- **The market is contested.** ByteDance UI-TARS Desktop, Cua (YC X25),
  Anthropic Cowork, OpenAI Codex desktop, and Apple Intelligence /
  Microsoft Agent 365 all ship working products in this space. Nerve is
  not trying to compete with any of them as a consumer product. It's a
  hackable foundation for people building something narrower.
- **Cross-platform is a stated goal but only Windows is verified by the
  author.** macOS / Linux backends compile but need someone with the
  hardware to validate end-to-end.
- **The Anthropic adapter is a reference, not the product.** It exists to
  prove the wire protocol works against a real model loop. The intended
  consumption path is "your tool connects to Nerve over WebSocket or a
  future MCP server", not "Nerve calls the model for you."
- **OCR and browser-CDP are feature-gated** because both pull in build-time
  dependencies most users don't have. They work when enabled.
- **Apple Intelligence and Windows Agent 365** have OS-level hooks that
  third-party userspace daemons cannot match. If you need the native AI
  on consumer hardware, use those. Nerve is for everything else.

## Working AI loop demo

If you want to see the full Computer Use protocol exercised against the
real Anthropic API:

```bash
# In one terminal
.\core\target\release\nerve.exe start

# In another
ANTHROPIC_API_KEY=sk-ant-... \
  python -m agents.demo.live_screenshot_test
```

Claude takes one screenshot, describes your real desktop in a paragraph,
and exits. Cost: ~$0.005. This proves the wire protocol, the platform
capture, the prompt-cached request, and the `tool_result` loop all work.

For the multi-turn loop with the demo task list:

```bash
ANTHROPIC_API_KEY=sk-... \
  python -m agents.demo.run_demo --adapter anthropic --auto-start --max-steps 10
```

`--live` removes dry-run and lets the model actually drive your mouse and
keyboard. Use with care.

## License

Apache-2.0. See [`LICENSE`](./LICENSE).

## Status of active development

Active development on the consumer-product framing has stopped - that
slot is occupied by larger players. The daemon, protocol, and SDKs are
maintained as a working reference and a hackable starting point. If
you're using it for something specific, file an issue and the relevant
code path will get the attention it needs.

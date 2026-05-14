# Competitive positioning

## The thing we are not

Nerve is not:

* A standalone AI agent. It does not call any model by default. The model
  is brought by the caller; Nerve only executes its decisions.
* A browser-only automation framework. Browser sites are one surface; the
  hard work is the rest of the OS — IDEs, settings panels, native dialogs.
* A pixel-only screenshot bot. Coordinate clicks are the fallback, not the
  primary plan.
* An MCP server. MCP exposes tools to a model; Nerve exposes a *runtime*
  with safety, audit, and replay. An MCP wrapper around the Nerve daemon
  is one valid usage, not the project itself.

## The thing it competes with

The reference loop today looks like this:

```
┌──────────────┐    screenshot     ┌───────────────┐    tool_call    ┌──────────────┐
│  Computer    │ ─────────────────►│  Model API    │ ─────────────► │  Adapter     │
│  Use model   │                   │  (CUA, CCU)   │                │  (clicks the │
│  caller      │ ◄─────────────────│               │ ◄───────────── │  pixels)     │
└──────────────┘   tool_result     └───────────────┘                └──────────────┘
        ▲                                                                     │
        └─────────────────────  one round-trip per step  ─────────────────────┘
```

Every step pays:

* one OS-level screenshot,
* one model call,
* one OS-level action.

That makes the loop slow, expensive, and brittle: the model has to
re-derive structure from pixels each turn, with no native handle on the
elements it just acted on.

## The Nerve loop

```
┌──────────────┐                                                                     ┌──────────────┐
│  Caller      │  semantic action ──► action compiler ──► native action  ───────►   │  OS          │
│  (model or   │                                                                     │  (macOS /    │
│   script)    │  ◄── verified result ◄── observation diff ◄── platform watcher  ──  │   Windows /  │
└──────────────┘                                                                     │   Linux)     │
        ▲                                                                            └──────────────┘
        │
        └─────── model call only when the next step is genuinely ambiguous ────────
```

The differences that matter:

1. **Persistent observation stream.** The daemon already knows the active
   window, cursor, and (where available) the accessibility tree. Agents
   that subscribe instead of polling pay nothing per tick.
2. **Semantic actions.** "Click the Save button in TextEdit" is one
   call, not a pixel-search + click. The compiler tries the accessibility
   API first, the native menu shortcut second, the OCR bounding box third,
   and the raw coordinate click last.
3. **Native execution.** `SendInput`, `CGEvent`, XTest — no remote IPC,
   no Selenium driver. Action latency drops to microseconds.
4. **Verification.** Every action carries before/after screenshot hashes,
   `active_window` snapshots, and the compiler trace. The agent can ask
   "did this work?" without taking another full screenshot.
5. **Safety, audit, replay.** All gates and logs live inside the runtime,
   not bolted on per-agent.

## Concrete example — "save the current file"

Raw CUA-style:

| Step | Cost |
|------|------|
| Screenshot screen                       | 1 OS call |
| Send screenshot + prompt to model       | 1 model call |
| Model returns "click 812, 441"          | — |
| Click 812, 441                          | 1 OS call |
| Screenshot screen                       | 1 OS call |
| Send screenshot + prompt to model       | 1 model call |
| Model decides "type filename"           | — |
| Type filename                           | 1 OS call |
| ...continued                            | ... |

Nerve loop:

| Step | Cost |
|------|------|
| `execute_action { semantic: click_element, target: { text: "Save", role: "button" } }` | 1 round trip |
| Daemon does AX lookup, invokes `AXPress`                                                 | 1 native call |
| Returns `compiled.method = "accessibility_action"`                                       | — |
| `execute_action { semantic: type_into_focused_element, text: "report.pdf" }`             | 1 round trip |
| Daemon types via native keyboard API                                                     | 1 native call |
| Returns success                                                                          | — |

Same outcome, zero screenshots in the hot path, zero model calls beyond
the initial plan, and a full audit trail.

## Where Nerve obviously loses

* **Tasks the model has never seen** — e.g. "find the button that looks
  like a sun." There the model still needs visual input. Nerve doesn't
  remove the model from those steps; it just stops paying for the model
  *every other step*.
* **Heavily-customised apps without accessibility metadata.** When AX is
  empty, the compiler falls back to OCR/coordinate clicks — the same cost
  as a CUA loop, but Nerve still wins on logging and safety.
* **Browser-internal automation.** Real browser DOM access via WebDriver
  is still superior for web-only flows. The compiler reserves a
  `BrowserDomAdapter` method specifically so a future bolted-in CDP
  driver can win those cases.

## What we want to be known for

* "The execution layer that turns a slow CUA loop into a fast native one."
* "The runtime that makes computer-use agents auditable by default."
* "The body for AI agents."

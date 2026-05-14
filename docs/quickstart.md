# Quickstart

## Install the daemon

### macOS / Linux

```bash
# Prerequisites: stable Rust, libxdo-dev / libxtst-dev on Linux X11.
git clone https://github.com/ashtonvaughan/nerve.git
cd nerve/core
cargo build --release

# Run it.
./target/release/nerve start
```

On Linux you may need to install build-time X11 deps first:

```bash
sudo apt install -y libxdo-dev libxtst-dev libxcb1-dev libdbus-1-dev
```

### Windows

```powershell
git clone https://github.com/ashtonvaughan/nerve.git
cd nerve\core
cargo build --release
.\target\release\nerve.exe start
```

If `cargo build` fails with linker errors, install the [Visual Studio C++
build tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
first.

## Verify the install

In another terminal:

```bash
nerve status         # daemon reachable?
nerve doctor         # full OS / permission inventory
nerve capabilities   # JSON capabilities the daemon advertises
```

`nerve doctor` will tell you exactly which permissions still need to be
granted (Screen Recording / Accessibility on macOS, integrity level on
Windows, Wayland / X11 hints on Linux).

## Hello world via CLI

```bash
nerve start --dry-run                   # boot the daemon (foreground)
nerve screenshot -o /tmp/test.png       # writes /tmp/test.png
nerve type "Hello from Nerve"           # types into the focused window
nerve hotkey ctrl+s                     # save
nerve clipboard-set "from nerve"        # set clipboard
nerve clipboard-get                     # echo back
nerve logs --limit 5                    # tail recent audit entries
```

## Python SDK

```bash
pip install -e ./sdks/python  # editable install while in development
```

```python
from nerve import NerveClient
from nerve.types import SafetyPolicy

with NerveClient() as client:
    caps = client.get_capabilities()
    print(caps.platform, caps.has_accessibility)
    client.click(100, 200)
    client.click_element(text="Save", role="button")
    client.type_text("Hello from Nerve")
    client.hotkey(["ctrl", "s"])
    for entry in client.get_action_log(limit=5):
        print(entry.timestamp, entry.method, entry.ok)
```

For asyncio agents, swap in `AsyncNerveClient`:

```python
import asyncio
from nerve import AsyncNerveClient

async def main():
    async with AsyncNerveClient() as client:
        async for obs in client.subscribe_observations(interval_ms=500):
            if obs.safety["emergency_stopped"]:
                break

asyncio.run(main())
```

## TypeScript SDK

```bash
cd sdks/typescript
npm install
npm run build
```

```ts
import { NerveClient } from "@nerve/sdk";

const client = new NerveClient();
await client.connect();
const obs = await client.getObservation();
await client.click(100, 200);
await client.clickElement({ text: "Save", role: "button" });
await client.typeText("Hello from Nerve");
await client.hotkey(["ctrl", "s"]);
await client.stop();
```

The SDK works in Node 18+, Bun, Deno, and modern browsers. In Node it uses
the `ws` package; everywhere else it picks up the global `WebSocket`.

## Dashboard

The dashboard ships inside the daemon and is served from the same port:

```
http://127.0.0.1:8765/
```

It shows the live screenshot with a cursor pip, current observation JSON,
recent actions, and an emergency-stop button. No extra build step is
required.

## Demo agent

```bash
python -m agents.demo.run_demo --auto-start          # dry-run
python -m agents.demo.run_demo --auto-start --live   # live mode (will type!)
```

The demo uses the deterministic [`MockAgent`](../agents/mock/__init__.py).
Swap it for `OpenAICuaAdapter` / `AnthropicComputerUseAdapter` once the
adapters are implemented.

## Benchmarks

```bash
python -m benchmarks.harness.runner --auto-start
```

Results land in `benchmarks/results/bench-<timestamp>.json`. See
[`benchmark-methodology.md`](./benchmark-methodology.md) for what each
metric means.

## Where to go next

* [Architecture](./architecture.md) — what the daemon is doing under the hood.
* [Protocol](./protocol.md) — every WebSocket message, every JSON field.
* [Platform backends](./platform-backends.md) — macOS / Windows / Linux upgrade paths.
* [Safety](./safety.md) — how Nerve gates and audits every action.
* [Competitive positioning](./competitive-positioning.md) — how this differs from raw CUA loops.

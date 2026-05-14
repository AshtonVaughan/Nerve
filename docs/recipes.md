# Recipes

End-to-end snippets for common agent patterns. Every recipe assumes the
daemon is running locally (`nerve start`) and the relevant SDK is installed.

## Open a text editor and write a note

```python
from nerve import NerveClient

with NerveClient() as c:
    c.open_app("TextEdit")  # macOS — use "Notepad" on Windows, "gedit" on Linux
    c.click_element(text="New", role="menuitem", app="TextEdit")  # if needed
    c.type_text("Hello from Nerve")
    c.hotkey(["meta", "s"])  # cmd+s on macOS
    c.type_text("nerve-note.txt")
    c.key_press("enter")
```

## Save the focused file via accessibility action (no pixels)

```python
c.click_element(text="Save", role="button")  # AX-first; falls back to OCR
```

## Read the active URL bar in Chrome

```python
c.click_element(role="textfield", app="Google Chrome")  # focuses the omnibox
c.hotkey(["meta", "l"])
c.hotkey(["meta", "c"])  # cmd+c
url = c.clipboard_get()
```

## Wait until a window appears, then act

```python
c.execute({
    "type": "wait_for_window",
    "app": "Calculator",
    "timeout_ms": 5_000,
})
c.click_element(text="7")
```

## Form-fill that survives a flaky tab order

```python
fields = [
    {"role": "textfield", "label": "Email"},
    {"role": "textfield", "label": "Password"},
]
for f, value in zip(fields, ["ada@nerve.dev", "hunter2"]):
    c.click_element(role=f["role"], text=f["label"])
    c.type_text(value)
c.click_element(text="Sign in", role="button")
```

## Replay a previous session

```bash
nerve replay <session_id> --speed 4
```

## Run with maximum safety while developing

```python
from nerve.types import SafetyPolicy

policy = SafetyPolicy(
    dry_run=True,                        # log + log; no OS calls
    require_confirmation=True,
    app_allowlist=["TextEdit"],
    max_actions_per_minute=60,
)
client = NerveClient()
client.connect(policy=policy)
```

## Subscribe to a cheap cursor stream for live UIs

```python
async for tick in client.subscribe_cursor(interval_ms=16):
    print(tick.cursor)
```

(TypeScript: `client.subscribeCursor({ intervalMs: 16 })`.)

## Tail the audit log

```bash
nerve logs --session sess_abcdef --limit 200
```

## Wire an OpenAI CUA model

```python
import os, asyncio
from nerve import AsyncNerveClient
from agents import AdapterState
from agents.openai import OpenAICuaAdapter

os.environ.setdefault("OPENAI_API_KEY", "sk-...")

async def go():
    client = AsyncNerveClient()
    await client.connect()
    agent = OpenAICuaAdapter()
    state = AdapterState(task="Open Chrome and search 'nerve runtime'.")
    obs = await client.get_observation(include_screenshot=True)
    while not state.done:
        actions = agent.plan(obs.raw, state)
        if not actions:
            break
        results = await client.execute_batch(actions)
        for r in results:
            if not r.ok:
                state.done = True
        obs = await client.get_observation(include_screenshot=True)

asyncio.run(go())
```

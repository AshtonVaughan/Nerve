# nerve-sdk (Python)

Official Python SDK for the [Nerve](https://github.com/ashtonvaughan/nerve) real-time computer-use runtime.

## Install

```bash
pip install -e ./sdks/python
```

## Quickstart

```python
from nerve import NerveClient

client = NerveClient()
client.connect()
print(client.get_capabilities())

obs = client.get_observation()
print(obs.platform, obs.active_window)

client.click_element(text="Save", role="button")
client.type_text("Hello from Nerve")
client.hotkey(["ctrl", "s"])

for entry in client.get_action_log(limit=10):
    print(entry.timestamp, entry.method, entry.ok)

client.stop()
```

## Async

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

# @nerve/sdk

TypeScript / JavaScript SDK for the [Nerve](https://github.com/ashtonvaughan/nerve) real-time computer-use runtime.

Works in Node.js (via the `ws` package) and modern browsers / Bun / Deno (via the global `WebSocket`).

## Install

```bash
cd sdks/typescript
npm install
npm run build
```

## Quickstart

```ts
import { NerveClient } from "@nerve/sdk";

const client = new NerveClient();
await client.connect();
const obs = await client.getObservation();
console.log(obs.platform, obs.active_window);

await client.click(100, 200);
await client.clickElement({ text: "Save", role: "button" });
await client.typeText("Hello from Nerve");
await client.hotkey(["ctrl", "s"]);

for await (const obs of client.subscribeObservations({ intervalMs: 500 })) {
  if (obs.safety_state.emergency_stopped) break;
}

await client.stop();
```

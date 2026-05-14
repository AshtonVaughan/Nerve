"""Soak / load tester for the Nerve daemon.

Spawns N concurrent WebSocket clients, each opening a session and running a
configurable number of actions per second for the configured duration.
Reports per-client and aggregate latency stats at the end.

Usage::

    python -m benchmarks.harness.load --clients 100 --duration-s 60 --actions-per-s 10
"""

from __future__ import annotations

import argparse
import asyncio
import json
import statistics
import time
import uuid
from typing import Any, Dict, List

import websockets


async def one_client(idx: int, host: str, port: int, duration_s: float, actions_per_s: float, latencies: List[float], errors: Dict[str, int]) -> None:
    try:
        async with websockets.connect(f"ws://{host}:{port}/") as ws:
            # drain hello
            await ws.recv()
            await ws.send(json.dumps({
                "kind": "session_start",
                "request_id": f"c{idx}-start",
                "client_name": f"load-{idx}",
            }))
            await ws.recv()
            interval = 1.0 / max(actions_per_s, 0.01)
            deadline = time.perf_counter() + duration_s
            while time.perf_counter() < deadline:
                rid = f"c{idx}-{uuid.uuid4().hex[:6]}"
                t0 = time.perf_counter()
                await ws.send(json.dumps({
                    "kind": "execute_action",
                    "request_id": rid,
                    "action": {
                        "id": f"act_{uuid.uuid4().hex}",
                        "action": {"type": "wait", "ms": 1},
                    },
                }))
                resp = await ws.recv()
                latencies.append((time.perf_counter() - t0) * 1000.0)
                msg = json.loads(resp)
                if msg.get("kind") == "error":
                    code = msg.get("code", "unknown")
                    errors[code] = errors.get(code, 0) + 1
                await asyncio.sleep(interval)
    except Exception as e:  # noqa: BLE001
        errors[type(e).__name__] = errors.get(type(e).__name__, 0) + 1


async def run(clients: int, host: str, port: int, duration_s: float, actions_per_s: float) -> Dict[str, Any]:
    latencies: List[float] = []
    errors: Dict[str, int] = {}
    started = time.perf_counter()
    await asyncio.gather(*[
        one_client(i, host, port, duration_s, actions_per_s, latencies, errors)
        for i in range(clients)
    ])
    elapsed = time.perf_counter() - started
    latencies.sort()
    n = len(latencies)
    def pct(p: float) -> float:
        return latencies[int(min(n - 1, p * n))] if n else 0.0
    return {
        "clients": clients,
        "duration_s": elapsed,
        "actions": n,
        "errors": errors,
        "p50_ms": pct(0.5),
        "p95_ms": pct(0.95),
        "p99_ms": pct(0.99),
        "max_ms": latencies[-1] if n else 0.0,
        "mean_ms": statistics.mean(latencies) if n else 0.0,
        "throughput_actions_per_s": n / elapsed if elapsed > 0 else 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--clients", type=int, default=10)
    parser.add_argument("--duration-s", type=float, default=10.0)
    parser.add_argument("--actions-per-s", type=float, default=5.0)
    args = parser.parse_args()
    result = asyncio.run(run(args.clients, args.host, args.port, args.duration_s, args.actions_per_s))
    print(json.dumps(result, indent=2))
    if result["errors"]:
        return 1
    return 0


if __name__ == "__main__":
    import sys
    sys.exit(main())

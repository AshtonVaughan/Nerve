"""Cassette recorder for the benchmark harness.

Runs a benchmark task against a real adapter (Anthropic / OpenAI / mock)
and records the action sequence the adapter emits. Saved cassettes live
under `benchmarks/cassettes/<task>.json` and are replayed by the runner
in CI via `--cassettes`.

Usage::

    # Record the calculator_demo cassette using Anthropic Computer Use:
    ANTHROPIC_API_KEY=sk-ant-... \\
        python -m benchmarks.harness.record \\
            --task calculator_demo --adapter anthropic

    # Mock recording (for tests that don't need a real API):
    python -m benchmarks.harness.record --task calculator_demo --adapter mock
"""

from __future__ import annotations

import argparse
import json
import socket
import sys
import time
from pathlib import Path
from typing import Any, Dict, List

REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT))
sys.path.insert(0, str(REPO_ROOT / "sdks" / "python"))

from nerve import NerveClient  # noqa: E402
from nerve.types import SafetyPolicy  # noqa: E402

from agents.base import AdapterState  # noqa: E402
from agents.mock import MockAgent  # noqa: E402
from agents.anthropic import AnthropicComputerUseAdapter  # noqa: E402
from agents.openai import OpenAICuaAdapter  # noqa: E402

from benchmarks.harness.tasks import TASK_REGISTRY


CASSETTE_DIR = REPO_ROOT / "benchmarks" / "cassettes"


def resolve_adapter(name: str):
    import os
    if name == "mock":
        return MockAgent()
    if name == "anthropic":
        if not os.environ.get("ANTHROPIC_API_KEY"):
            raise SystemExit("ANTHROPIC_API_KEY required for --adapter anthropic")
        return AnthropicComputerUseAdapter()
    if name == "openai":
        if not os.environ.get("OPENAI_API_KEY"):
            raise SystemExit("OPENAI_API_KEY required for --adapter openai")
        return OpenAICuaAdapter()
    raise SystemExit(f"unknown adapter: {name}")


def main() -> int:
    parser = argparse.ArgumentParser(description="Record a cassette for a benchmark task.")
    parser.add_argument("--task", required=True)
    parser.add_argument("--adapter", choices=["mock", "anthropic", "openai"], default="mock")
    parser.add_argument("--max-steps", type=int, default=20)
    args = parser.parse_args()

    if args.task not in TASK_REGISTRY:
        raise SystemExit(f"unknown task: {args.task}")
    task = TASK_REGISTRY[args.task]
    agent = resolve_adapter(args.adapter)

    actions: List[Dict[str, Any]] = []
    client = NerveClient()
    policy = SafetyPolicy(dry_run=True, max_actions_per_minute=2400)
    client.connect(policy=policy)
    try:
        state = AdapterState(task=task.description)
        for _ in range(args.max_steps):
            obs = client.get_observation(include_screenshot=args.adapter != "mock")
            step = agent.plan(obs.raw, state)
            if not step:
                break
            for action in step:
                actions.append(action)
                client.execute(action)
            if args.adapter == "mock":
                state.done = True
                break
    finally:
        client.stop()

    out = CASSETTE_DIR / f"{args.task}.json"
    CASSETTE_DIR.mkdir(parents=True, exist_ok=True)
    with open(out, "w") as f:
        json.dump(
            {
                "task": args.task,
                "description": task.description,
                "recorded_with": args.adapter,
                "recorded_at": int(time.time()),
                "actions": actions,
            },
            f,
            indent=2,
        )
    print(f"wrote {out} ({len(actions)} actions)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

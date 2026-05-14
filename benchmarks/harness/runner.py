"""Benchmark runner.

The runner is intentionally I/O-bound. It does not call any external models.
Instead, it executes each task's static plan against the Nerve daemon (the
mock agent contributes no model-call overhead so we can isolate runtime
performance).

For runs against a real LLM, drop the model adapter into ``model_calls`` /
``est_cost_usd`` and feed it instead of the static plan.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional

REPO_ROOT = Path(__file__).resolve().parents[2]
SDK_PATH = REPO_ROOT / "sdks" / "python"
sys.path.insert(0, str(SDK_PATH))
sys.path.insert(0, str(REPO_ROOT))

from nerve import NerveClient  # noqa: E402
from nerve.types import SafetyPolicy  # noqa: E402

from benchmarks.harness.tasks import TASKS, BenchTask  # noqa: E402


@dataclass
class TaskResult:
    name: str
    task_success: bool
    task_duration_ms: int
    action_count: int
    failed_actions: int
    screenshot_count: int
    recovery_attempts: int
    human_interventions: int
    model_calls: int
    est_cost_usd: float
    avg_action_latency_ms: float
    methods: List[str] = field(default_factory=list)


def auto_start_daemon(dry_run: bool) -> subprocess.Popen:
    nerve = shutil.which("nerve")
    if not nerve:
        for cand in [
            REPO_ROOT / "core" / "target" / "debug" / "nerve",
            REPO_ROOT / "core" / "target" / "release" / "nerve",
        ]:
            if cand.exists():
                nerve = str(cand)
                break
    if not nerve:
        raise RuntimeError("nerve binary not found; run `cargo build` in core/")
    args = [nerve, "start"]
    if dry_run:
        args.append("--dry-run")
    proc = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(60):
        try:
            with socket.create_connection(("127.0.0.1", 8765), timeout=0.25):
                return proc
        except OSError:
            time.sleep(0.1)
    raise RuntimeError("daemon never started accepting connections")


def run_task(client: NerveClient, task: BenchTask) -> TaskResult:
    started = time.perf_counter()
    methods: List[str] = []
    failed = 0
    screenshots = 0
    recovery = 0
    latencies: List[float] = []

    for action in task.plan:
        per = time.perf_counter()
        try:
            result = client.execute(action)
        except Exception as e:  # noqa: BLE001
            methods.append("error")
            failed += 1
            continue
        latencies.append((time.perf_counter() - per) * 1000.0)
        methods.append(result.method)
        if not result.ok:
            failed += 1
        if action.get("type") in ("screenshot", "get_observation") and result.ok:
            screenshots += 1
        compiled = result.compiled if hasattr(result, "compiled") else None
        if compiled and compiled.get("attempted"):
            recovery += max(0, len(compiled["attempted"]) - 1)

    duration_ms = int((time.perf_counter() - started) * 1000)
    avg_latency = sum(latencies) / len(latencies) if latencies else 0.0

    return TaskResult(
        name=task.name,
        task_success=task.succeeded(methods),
        task_duration_ms=duration_ms,
        action_count=len(task.plan),
        failed_actions=failed,
        screenshot_count=screenshots,
        recovery_attempts=recovery,
        human_interventions=0,
        model_calls=0,
        est_cost_usd=0.0,
        avg_action_latency_ms=round(avg_latency, 2),
        methods=methods,
    )


def main() -> int:
    parser = argparse.ArgumentParser(description="Run Nerve benchmark tasks.")
    parser.add_argument("--auto-start", action="store_true")
    parser.add_argument("--live", action="store_true")
    parser.add_argument("--task", action="append", help="run only these tasks")
    parser.add_argument(
        "--out-dir", default=str(REPO_ROOT / "benchmarks" / "results"), help="directory for result JSON"
    )
    args = parser.parse_args()

    daemon_proc = None
    if args.auto_start:
        daemon_proc = auto_start_daemon(dry_run=not args.live)

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    client = NerveClient()
    policy = SafetyPolicy(dry_run=not args.live, max_actions_per_minute=2400)
    try:
        client.connect(policy=policy)
        caps = client.get_capabilities()
        print(f"[bench] platform={caps.platform} dry_run={not args.live}")
        results: List[TaskResult] = []
        for task in TASKS:
            if args.task and task.name not in args.task:
                continue
            print(f"[bench] running {task.name}")
            r = run_task(client, task)
            print(
                f"        success={r.task_success} duration={r.task_duration_ms}ms "
                f"failed={r.failed_actions} avg_latency={r.avg_action_latency_ms}ms"
            )
            results.append(r)

        ts = int(time.time())
        out_file = out_dir / f"bench-{ts}.json"
        with open(out_file, "w") as f:
            json.dump(
                {
                    "platform": caps.platform,
                    "dry_run": not args.live,
                    "results": [asdict(r) for r in results],
                },
                f,
                indent=2,
            )
        print(f"[bench] wrote {out_file}")
        if any(not r.task_success for r in results):
            return 1
        return 0
    finally:
        try:
            client.stop()
        except Exception:
            pass
        if daemon_proc is not None:
            daemon_proc.send_signal(signal.SIGINT)
            try:
                daemon_proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                daemon_proc.kill()


if __name__ == "__main__":
    sys.exit(main())

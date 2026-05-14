"""End-to-end Nerve demo.

Runs the mock agent through a small synthetic task. The demo is deliberately
gentle on the host — by default it runs in **dry-run** mode so it can be
executed on CI machines without permission to drive the actual UI. Pass
``--live`` to disable dry-run.

Usage:

    # one-shot demo against an already-running daemon
    python -m agents.demo.run_demo

    # run live (will move your mouse / type into the focused window!)
    python -m agents.demo.run_demo --live

    # spawn the daemon for the duration of the demo
    python -m agents.demo.run_demo --auto-start --live
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path
from typing import List, Optional

REPO_ROOT = Path(__file__).resolve().parents[2]
SDK_PATH = REPO_ROOT / "sdks" / "python"
if str(SDK_PATH) not in sys.path:
    sys.path.insert(0, str(SDK_PATH))
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from nerve import NerveClient  # noqa: E402
from nerve.types import SafetyPolicy  # noqa: E402

from agents import AdapterState, MockAgent  # noqa: E402


DEMO_TASKS = [
    "open the notepad app",
    "type 'Hello from Nerve'",
    "save the file",
    "look at the clipboard",
]


def find_nerve_binary() -> Optional[str]:
    candidate = shutil.which("nerve")
    if candidate:
        return candidate
    debug = REPO_ROOT / "core" / "target" / "debug" / "nerve"
    if debug.exists():
        return str(debug)
    release = REPO_ROOT / "core" / "target" / "release" / "nerve"
    if release.exists():
        return str(release)
    return None


def auto_start_daemon(dry_run: bool) -> subprocess.Popen:
    binary = find_nerve_binary()
    if not binary:
        raise RuntimeError(
            "could not locate the nerve binary. "
            "Run `cargo build` in `core/` or install the binary on PATH."
        )
    args = [binary, "start"]
    if dry_run:
        args.append("--dry-run")
    print(f"[demo] spawning daemon: {' '.join(args)}")
    proc = subprocess.Popen(
        args,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        env={**os.environ, "RUST_LOG": os.environ.get("RUST_LOG", "info")},
    )
    # Give the WS listener a moment to bind.
    for _ in range(40):
        try:
            import socket

            with socket.create_connection(("127.0.0.1", 8765), timeout=0.25):
                return proc
        except OSError:
            time.sleep(0.1)
    raise RuntimeError("daemon never started accepting connections")


def main() -> int:
    parser = argparse.ArgumentParser(description="Run the Nerve demo agent.")
    parser.add_argument("--live", action="store_true", help="disable dry-run mode")
    parser.add_argument(
        "--auto-start", action="store_true", help="spawn the daemon for the duration of the demo"
    )
    args = parser.parse_args()

    daemon_proc: Optional[subprocess.Popen] = None
    if args.auto_start:
        daemon_proc = auto_start_daemon(dry_run=not args.live)

    agent = MockAgent()
    client = NerveClient()
    policy = SafetyPolicy(dry_run=not args.live, max_actions_per_minute=240)
    try:
        session = client.connect(policy=policy)
        print(f"[demo] connected, session {session}")
        caps = client.get_capabilities()
        print(f"[demo] platform={caps.platform} ax={caps.has_accessibility} wayland_limited={caps.wayland_limited}")

        for task in DEMO_TASKS:
            print(f"[demo] task: {task}")
            state = AdapterState(task=task)
            while not state.done:
                obs = client.get_observation(include_screenshot=False)
                actions = agent.plan(obs.raw, state)
                if not actions:
                    break
                results = client.execute_batch(actions, stop_on_error=False)
                for r in results:
                    state.history.append({"id": r.id, "method": r.method, "ok": r.ok})
                    flag = "ok" if r.ok else f"err({r.error})"
                    print(f"        -> {r.method} {flag}")

        log = client.get_action_log(limit=50)
        print(f"[demo] audit log entries: {len(log)}")
        for entry in log[-5:]:
            print(f"        {entry.timestamp} {entry.method} ok={entry.ok}")
    finally:
        try:
            client.stop()
        except Exception:
            pass
        if daemon_proc is not None:
            print("[demo] stopping daemon")
            daemon_proc.send_signal(signal.SIGINT)
            try:
                daemon_proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                daemon_proc.kill()
    return 0


if __name__ == "__main__":
    sys.exit(main())

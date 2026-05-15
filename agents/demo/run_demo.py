"""End-to-end Nerve demo.

Runs an agent through a small synthetic task list. The demo is gentle by
default: it picks the MockAgent and runs in dry-run mode so the daemon
makes no OS calls, which means it works on CI / sandboxes without input
permissions.

Pass ``--adapter anthropic`` or ``--adapter openai`` to drive a real
model. Pass ``--live`` to turn dry-run off (which lets the daemon
actually move the mouse / type / click).

Usage::

    # one-shot demo against an already-running daemon
    python -m agents.demo.run_demo

    # run live with the mock (will move your mouse / type into the focused window!)
    python -m agents.demo.run_demo --live

    # spawn the daemon for the duration of the demo and use the mock
    python -m agents.demo.run_demo --auto-start --live

    # Use the real Anthropic Computer Use adapter (requires ANTHROPIC_API_KEY)
    ANTHROPIC_API_KEY=sk-ant-... \
        python -m agents.demo.run_demo --adapter anthropic --auto-start --live

    # OpenAI Computer Use Preview (requires OPENAI_API_KEY)
    OPENAI_API_KEY=sk-... \
        python -m agents.demo.run_demo --adapter openai --auto-start --live
"""

from __future__ import annotations

import argparse
import os
import shutil
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path
from typing import Optional

REPO_ROOT = Path(__file__).resolve().parents[2]
SDK_PATH = REPO_ROOT / "sdks" / "python"
if str(SDK_PATH) not in sys.path:
    sys.path.insert(0, str(SDK_PATH))
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from nerve import NerveClient  # noqa: E402
from nerve.types import SafetyPolicy  # noqa: E402

from agents import AdapterState, MockAgent  # noqa: E402
from agents.anthropic import AnthropicComputerUseAdapter  # noqa: E402
from agents.openai import OpenAICuaAdapter  # noqa: E402


DEMO_TASKS = [
    "open the notepad app",
    "type 'Hello from Nerve'",
    "save the file",
    "look at the clipboard",
]


# Adapter selection table. Each entry is a tuple of
#   (factory, required env-var-or-None, friendly name)
# so we can fail loud at *startup* when the user picked an adapter whose
# credentials aren't present, instead of mid-loop where the daemon has
# already burned actions.
ADAPTERS: dict[str, tuple] = {
    "mock": (lambda: MockAgent(), None, "MockAgent"),
    "anthropic": (
        lambda: AnthropicComputerUseAdapter(),
        "ANTHROPIC_API_KEY",
        "AnthropicComputerUseAdapter",
    ),
    "openai": (
        lambda: OpenAICuaAdapter(),
        "OPENAI_API_KEY",
        "OpenAICuaAdapter",
    ),
}


def find_nerve_binary() -> Optional[str]:
    candidate = shutil.which("nerve")
    if candidate:
        return candidate
    exe = ".exe" if sys.platform == "win32" else ""
    for build in ("release", "debug"):
        path = REPO_ROOT / "core" / "target" / build / f"nerve{exe}"
        if path.exists():
            return str(path)
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
            with socket.create_connection(("127.0.0.1", 8765), timeout=0.25):
                return proc
        except OSError:
            time.sleep(0.1)
    raise RuntimeError("daemon never started accepting connections")


def resolve_adapter(name: str):
    """Instantiate the chosen adapter or fail loud with a clear error."""
    if name not in ADAPTERS:
        raise SystemExit(
            f"unknown adapter {name!r}; valid choices: {', '.join(sorted(ADAPTERS))}"
        )
    factory, required_env, friendly = ADAPTERS[name]
    if required_env and not os.environ.get(required_env):
        raise SystemExit(
            f"adapter {name!r} requires the {required_env} environment variable. "
            f"Set it and re-run, or pick --adapter mock."
        )
    print(f"[demo] using adapter: {friendly}")
    return factory()


def main() -> int:
    parser = argparse.ArgumentParser(description="Run the Nerve demo agent.")
    parser.add_argument("--live", action="store_true", help="disable dry-run mode")
    parser.add_argument(
        "--auto-start",
        action="store_true",
        help="spawn the daemon for the duration of the demo",
    )
    parser.add_argument(
        "--adapter",
        choices=sorted(ADAPTERS),
        default="mock",
        help="model adapter to drive the demo (default: mock)",
    )
    parser.add_argument(
        "--max-steps",
        type=int,
        default=25,
        help="safety cap on the number of plan() calls per task (default: 25)",
    )
    args = parser.parse_args()

    # Validate credentials before we boot the daemon so failure is fast and
    # cheap, not after the user has already paid for an OS-level grant.
    agent = resolve_adapter(args.adapter)

    daemon_proc: Optional[subprocess.Popen] = None
    if args.auto_start:
        daemon_proc = auto_start_daemon(dry_run=not args.live)

    client = NerveClient()
    policy = SafetyPolicy(dry_run=not args.live, max_actions_per_minute=240)
    try:
        session = client.connect(policy=policy)
        print(f"[demo] connected, session {session}")
        caps = client.get_capabilities()
        print(
            f"[demo] platform={caps.platform} ax={caps.has_accessibility} "
            f"wayland_limited={caps.wayland_limited}"
        )

        max_steps = args.max_steps
        for task in DEMO_TASKS:
            print(f"[demo] task: {task}")
            state = AdapterState(task=task)
            # Local step counter so we don't fight adapters that track
            # `state.step` themselves (e.g. MockAgent gates its first batch
            # on `state.step == 0` and increments internally).
            steps = 0
            while not state.done and steps < max_steps:
                steps += 1
                # Real adapters need the screenshot to ground their next
                # decision; MockAgent doesn't, but the cost is small enough
                # that we always include it when not mock.
                obs = client.get_observation(
                    include_screenshot=args.adapter != "mock"
                )
                actions = agent.plan(obs.raw, state)
                if not actions:
                    # Either the model finished (state.done set in plan)
                    # or it returned only text. Either way: this task is done.
                    break
                # Show what the model actually asked for so dry-run output
                # isn't a wall of indistinguishable "no_op ok" lines.
                for a in actions:
                    kind = a.get("type", "?")
                    summary = {
                        k: v for k, v in a.items()
                        if k in ("x", "y", "text", "keys", "delta_x", "delta_y")
                    }
                    print(f"        ~ {kind} {summary}")
                results = client.execute_batch(actions, stop_on_error=False)
                for r in results:
                    state.history.append({"id": r.id, "method": r.method, "ok": r.ok})
                    flag = "ok" if r.ok else f"err({r.error})"
                    print(f"        -> {r.method} {flag}")
                # Real model adapters drive themselves; mock breaks after
                # one step per task.
                if args.adapter == "mock":
                    state.done = True
            if steps >= max_steps and not state.done:
                print(f"        ! reached max_steps={max_steps}, moving on")

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
            # SIGINT is not deliverable to a Windows console child from a
            # different console, so we use terminate() + a short wait. On
            # POSIX this still sends SIGTERM which the daemon handles.
            daemon_proc.terminate()
            try:
                daemon_proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                daemon_proc.kill()
    return 0


if __name__ == "__main__":
    sys.exit(main())

"""Adapter selection in run_demo: fail loud at startup, not mid-loop."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
RUN_DEMO = REPO_ROOT / "agents" / "demo" / "run_demo.py"


def _run(env: dict, *flags: str) -> subprocess.CompletedProcess:
    """Spawn the demo as a subprocess and capture stdout/stderr."""
    return subprocess.run(
        [sys.executable, str(RUN_DEMO), *flags, "--auto-start=false" if False else ""],
        # Pass adapter without --auto-start: we want to exercise *credential
        # validation*, not actually boot the daemon.
        env=env,
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        timeout=10,
    )


def test_anthropic_without_key_exits_with_clear_message():
    env = {k: v for k, v in os.environ.items() if k not in ("ANTHROPIC_API_KEY",)}
    env["PYTHONPATH"] = str(REPO_ROOT / "sdks" / "python")
    proc = subprocess.run(
        [sys.executable, str(RUN_DEMO), "--adapter", "anthropic"],
        env=env,
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        timeout=10,
    )
    # SystemExit with our message means non-zero exit code and stderr/stdout
    # containing the env var name.
    assert proc.returncode != 0
    combined = (proc.stdout + proc.stderr).lower()
    assert "anthropic_api_key" in combined
    assert "requires" in combined


def test_openai_without_key_exits_with_clear_message():
    env = {k: v for k, v in os.environ.items() if k not in ("OPENAI_API_KEY",)}
    env["PYTHONPATH"] = str(REPO_ROOT / "sdks" / "python")
    proc = subprocess.run(
        [sys.executable, str(RUN_DEMO), "--adapter", "openai"],
        env=env,
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        timeout=10,
    )
    assert proc.returncode != 0
    combined = (proc.stdout + proc.stderr).lower()
    assert "openai_api_key" in combined
    assert "requires" in combined


def test_unknown_adapter_rejected():
    env = dict(os.environ)
    env["PYTHONPATH"] = str(REPO_ROOT / "sdks" / "python")
    proc = subprocess.run(
        [sys.executable, str(RUN_DEMO), "--adapter", "no-such-thing"],
        env=env,
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        timeout=10,
    )
    # argparse rejects invalid choice with exit code 2.
    assert proc.returncode != 0
    combined = (proc.stdout + proc.stderr).lower()
    assert "no-such-thing" in combined or "invalid choice" in combined

"""Live end-to-end test: Claude opens a browser, navigates to YouTube,
clicks through a video.

This is a REAL live test. It will move your mouse, click, and type. Don't
run it while you have unsaved work in another window.

Usage::

    ANTHROPIC_API_KEY=sk-... python -m agents.demo.live_youtube_test

The daemon must already be running (`nerve start`).
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT))
sys.path.insert(0, str(REPO_ROOT / "sdks" / "python"))

from nerve import NerveClient  # noqa: E402
from nerve.types import SafetyPolicy  # noqa: E402

from agents.base import AdapterState  # noqa: E402
from agents.anthropic import AnthropicComputerUseAdapter  # noqa: E402


TASK = (
    "Open a web browser (Chrome or Edge - whichever you can find on the "
    "taskbar or via the Windows Start menu). Navigate to youtube.com. "
    "Once YouTube has loaded, click on any visible video thumbnail to "
    "open and start playing it. When the video begins playing, respond "
    "with a short text summary describing what video you opened and what "
    "you see, then stop. Do not navigate away from YouTube."
)


def main() -> int:
    agent = AnthropicComputerUseAdapter()
    client = NerveClient()
    # LIVE mode. Generous rate limit because Claude may issue many small
    # mouse moves; max_actions_per_minute is a safety net not a normal cap.
    policy = SafetyPolicy(dry_run=False, max_actions_per_minute=300)
    session = client.connect(policy=policy)
    print(f"[yt] connected, session {session}")
    print(f"[yt] model: {agent.config.model}")
    print(f"[yt] task: {TASK}")
    print()

    state = AdapterState(task=TASK)
    max_steps = 40
    final_text = ""
    t0 = time.time()
    try:
        while not state.done and state.step < max_steps:
            state.step += 1
            obs = client.get_observation(include_screenshot=True)
            screen = obs.raw.get("screen", {})
            active = obs.raw.get("active_window", {}) or {}
            app = str(active.get("app_name", "?")).encode("ascii", "replace").decode("ascii")
            print(
                f"[yt] step {state.step}/{max_steps}  "
                f"screen={screen.get('width')}x{screen.get('height')}  "
                f"active={app!r}"
            )
            actions = agent.plan(obs.raw, state)
            if not actions:
                # Final assistant text
                convo = agent._conversations.get(id(state), [])
                for msg in reversed(convo):
                    if msg.get("role") != "assistant":
                        continue
                    for block in msg.get("content", []):
                        if block.get("type") == "text":
                            final_text = block.get("text", "")
                            break
                    if final_text:
                        break
                break
            for a in actions:
                kind = a.get("type", "?")
                summary = {
                    k: v for k, v in a.items()
                    if k in ("x", "y", "text", "keys", "delta_x", "delta_y", "ms")
                }
                # Truncate long type_text for readability
                if "text" in summary and len(str(summary["text"])) > 60:
                    summary["text"] = str(summary["text"])[:57] + "..."
                print(f"        ~ {kind} {summary}")
            results = client.execute_batch(actions, stop_on_error=False)
            for r in results:
                state.history.append(
                    {"id": r.id, "method": r.method, "ok": r.ok}
                )
                flag = "ok" if r.ok else f"err({r.error})"
                print(f"        -> {r.method} {flag}")
    finally:
        try:
            client.stop()
        except Exception:
            pass

    elapsed = time.time() - t0
    print()
    print("=" * 72)
    print("CLAUDE'S FINAL REPORT:")
    print("=" * 72)
    print(final_text if final_text else "(no text response captured)")
    print("=" * 72)
    print(f"steps: {state.step}/{max_steps}   elapsed: {elapsed:.1f}s")
    print(f"notes: {state.notes}")
    return 0 if final_text else 1


if __name__ == "__main__":
    sys.exit(main())

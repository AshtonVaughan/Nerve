"""Single-shot live end-to-end proof: Claude sees the real screen.

Drives one Computer Use turn against the running daemon in LIVE mode but
restricts the task to "take a screenshot and describe it". No mouse moves,
no keyboard input. Verifies the full protocol works against real OS state
without putting the user's machine at risk.

Usage::

    ANTHROPIC_API_KEY=sk-... python -m agents.demo.live_screenshot_test
"""

from __future__ import annotations

import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT))
sys.path.insert(0, str(REPO_ROOT / "sdks" / "python"))

from nerve import NerveClient  # noqa: E402
from nerve.types import SafetyPolicy  # noqa: E402

from agents.base import AdapterState  # noqa: E402
from agents.anthropic import AnthropicComputerUseAdapter  # noqa: E402


def main() -> int:
    agent = AnthropicComputerUseAdapter()
    client = NerveClient()
    # Live mode but with the keyboard/mouse not in dry-run. Claude is told
    # only to take a screenshot and describe what's there, so even live
    # this is a no-input test.
    policy = SafetyPolicy(dry_run=False, max_actions_per_minute=60)
    session = client.connect(policy=policy)
    print(f"[live] connected, session {session}")
    print(f"[live] model: {agent.config.model}")

    state = AdapterState(
        task=(
            "Take exactly one screenshot of the current desktop. "
            "Then respond with a one-paragraph description of what you see. "
            "Do NOT use any other tools. Do NOT click anything. "
            "Do NOT type anything."
        )
    )

    max_steps = 4
    final_text = ""
    try:
        while not state.done and state.step < max_steps:
            state.step += 1
            obs = client.get_observation(include_screenshot=True)
            screen = obs.raw.get("screen", {})
            print(f"[live] step {state.step}: screen={screen.get('width')}x{screen.get('height')}")
            actions = agent.plan(obs.raw, state)
            if not actions:
                # Model returned text only -> task done. Pull the last
                # assistant message from the adapter's conversation.
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
                    if k in ("x", "y", "text", "keys", "delta_x", "delta_y")
                }
                print(f"        ~ {kind} {summary}")
            results = client.execute_batch(actions, stop_on_error=False)
            for r in results:
                state.history.append(
                    {"id": r.id, "method": r.method, "ok": r.ok}
                )
                flag = "ok" if r.ok else f"err({r.error})"
                print(f"        -> {r.method} {flag}")
    finally:
        client.stop()

    print()
    print("=" * 64)
    print("CLAUDE SAW THIS:")
    print("=" * 64)
    print(final_text if final_text else "(no text response captured)")
    print("=" * 64)
    print(f"steps taken: {state.step}")
    print(f"notes: {state.notes}")
    return 0 if final_text else 1


if __name__ == "__main__":
    sys.exit(main())

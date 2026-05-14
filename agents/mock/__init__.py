"""Deterministic mock agent.

Used by tests and the demo. The agent doesn't call any real model — it
matches a few keywords in the task string and emits an obvious action plan.
That makes it easy to exercise the runtime end-to-end on CI machines that
have no network access.
"""

from __future__ import annotations

from typing import Any, Dict, List

from ..base import AdapterState, ModelAdapter


class MockAgent:
    name = "mock"

    def plan(self, observation: Dict[str, Any], state: AdapterState) -> List[Dict[str, Any]]:
        task = (state.task or "").lower()
        plan: List[Dict[str, Any]] = []
        if state.done:
            return plan

        if state.step == 0:
            if "screenshot" in task or "look" in task:
                plan = [{"type": "screenshot"}]
            elif "clipboard" in task:
                plan = [
                    {"type": "clipboard_set", "text": "nerve says hello"},
                    {"type": "clipboard_get"},
                ]
            elif "open" in task and "app" in task:
                plan = [
                    {"type": "open_app", "name": _guess_app_name(task)},
                    {"type": "wait", "ms": 1500},
                ]
            elif "save" in task:
                plan = [
                    {"type": "click_element_by_text", "text": "Save"},
                    {"type": "wait", "ms": 500},
                ]
            elif "type" in task:
                plan = [
                    {"type": "type_text", "text": _extract_quoted(task) or "hello from nerve"}
                ]
            else:
                plan = [{"type": "get_observation"}]
        else:
            plan = []
            state.done = True
        state.step += 1
        return plan


def _extract_quoted(task: str) -> str | None:
    for sep in ('"', "'"):
        if sep in task:
            parts = task.split(sep)
            if len(parts) >= 3:
                return parts[1]
    return None


def _guess_app_name(task: str) -> str:
    candidates = ["calculator", "calc", "textedit", "notepad", "browser", "chrome", "safari"]
    for c in candidates:
        if c in task:
            return c
    return "notepad"

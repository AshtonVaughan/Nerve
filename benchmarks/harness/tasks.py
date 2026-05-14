"""Canonical local benchmark tasks.

Each task carries either:
* a *plan* — a static list of Nerve action dicts that the runner submits
  in order. Used in dry-run mode and for cassette playback.
* an *oracle* — a callable that inspects the run record and decides
  whether the task succeeded. The oracle has the final say; for tasks
  with no oracle we fall back to "every action executed without error".

The cassette flow lives next door in `benchmarks/cassettes/`. A cassette
is a JSON file with a top-level `actions` array; the runner loads it
exactly the same way as a static plan. Cassettes are how we replay
recorded model responses without burning tokens in CI.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Callable, Dict, List, Optional


# An oracle is given the runner's recorded state and returns True / False.
#
# Inputs:
#   methods       — list[str], the `result.method` Nerve returned per action.
#   action_types  — list[str], the action `type` we submitted per action.
#   failed_actions — int, count of `ok=false` results.
#   dry_run        — bool, whether the safety policy was dry_run.
#
# We pass the full record (not just a single field) so an oracle can
# express things like "exactly 3 keyboard methods + 1 wait" or
# "no failures regardless of method."
Oracle = Callable[[Dict[str, Any]], bool]


@dataclass
class BenchTask:
    name: str
    description: str
    plan: List[Dict[str, Any]]
    expected_method_sequence: List[str] = field(default_factory=list)
    safety_dry_run: bool = True
    oracle: Optional[Oracle] = None
    # Optional cassette to replay instead of running the plan. The runner
    # treats `cassette` and `plan` as equivalent: the cassette's actions
    # list overrides `plan` when present.
    cassette: Optional[str] = None

    def succeeded(self, record: Dict[str, Any]) -> bool:
        """Decide whether this run was a success.

        Order:
          1. If the task carries an `oracle`, use it.
          2. Else fall back to "every action succeeded and we submitted
             exactly the actions in `plan`".
        """
        if self.oracle is not None:
            return self.oracle(record)
        if record.get("failed_actions", 0) > 0:
            return False
        return record.get("action_types") == [a["type"] for a in self.plan]


# Reusable oracles for the production-grade tasks.

def _action_types_match(expected: List[str]) -> Oracle:
    def check(record: Dict[str, Any]) -> bool:
        return record.get("action_types") == expected and record.get("failed_actions", 0) == 0

    return check


def _no_failures() -> Oracle:
    def check(record: Dict[str, Any]) -> bool:
        return record.get("failed_actions", 0) == 0 and len(record.get("action_types", [])) > 0

    return check


TASKS: List[BenchTask] = [
    BenchTask(
        name="text_editor_write_note",
        description="Open a text editor and write a note.",
        plan=[
            {"type": "open_app", "name": "notepad"},
            {"type": "wait", "ms": 800},
            {"type": "type_text", "text": "Nerve smoke test"},
        ],
        expected_method_sequence=["native_ui_action", "wait", "keyboard"],
    ),
    BenchTask(
        name="save_file",
        description="Save the focused window via hotkey.",
        plan=[
            {"type": "hotkey", "keys": ["ctrl", "s"]},
        ],
        expected_method_sequence=["keyboard"],
    ),
    BenchTask(
        name="rename_file",
        description="Rename the focused file by issuing keyboard commands.",
        plan=[
            {"type": "hotkey", "keys": ["f2"]},
            {"type": "type_text", "text": "renamed.txt"},
            {"type": "key_press", "key": "enter"},
        ],
        expected_method_sequence=["keyboard", "keyboard", "keyboard"],
    ),
    # Tier 4.9 — production-graded oracle. In dry-run mode the daemon
    # never touches the OS, so we assert on submitted action types.
    BenchTask(
        name="calculator_demo",
        description="Open calculator and add 2 + 3.",
        plan=[
            {"type": "open_app", "name": "calculator"},
            {"type": "wait", "ms": 800},
            {"type": "type_text", "text": "2+3="},
        ],
        expected_method_sequence=["native_ui_action", "wait", "keyboard"],
        oracle=_action_types_match(["open_app", "wait", "type_text"]),
        cassette="calculator_demo.json",
    ),
    BenchTask(
        name="browser_search",
        description="Open the default browser and run a search.",
        plan=[
            {"type": "open_app", "name": "browser"},
            {"type": "wait", "ms": 1200},
            {"type": "type_text", "text": "what is nerve runtime"},
            {"type": "key_press", "key": "enter"},
        ],
        expected_method_sequence=["native_ui_action", "wait", "keyboard", "keyboard"],
    ),
    # Tier 4.9 — production-graded oracle. clipboard_copy_paste is the
    # simplest: the daemon's NoOp clipboard handlers never fail, so we
    # just assert the action sequence.
    BenchTask(
        name="clipboard_copy_paste",
        description="Round-trip clipboard set/get.",
        plan=[
            {"type": "clipboard_set", "text": "nerve clipboard demo"},
            {"type": "clipboard_get"},
        ],
        expected_method_sequence=["clipboard", "clipboard"],
        oracle=_action_types_match(["clipboard_set", "clipboard_get"]),
        cassette="clipboard_copy_paste.json",
    ),
    BenchTask(
        name="fill_local_form",
        description="Type into a generic focused field.",
        plan=[
            {"type": "type_text", "text": "ada@nerve.local"},
            {"type": "key_press", "key": "tab"},
            {"type": "type_text", "text": "hunter2"},
        ],
        expected_method_sequence=["keyboard", "keyboard", "keyboard"],
    ),
    # Tier 4.9 — production-graded oracle. change_setting hits all four
    # action types we want to exercise: hotkey, wait, key_press, key_press.
    BenchTask(
        name="change_setting",
        description="Open an app settings dialog and toggle a checkbox via keyboard.",
        plan=[
            {"type": "hotkey", "keys": ["ctrl", "comma"]},
            {"type": "wait", "ms": 500},
            {"type": "key_press", "key": "tab"},
            {"type": "key_press", "key": "space"},
        ],
        expected_method_sequence=["keyboard", "wait", "keyboard", "keyboard"],
        oracle=_action_types_match(["hotkey", "wait", "key_press", "key_press"]),
        cassette="change_setting.json",
    ),
]


TASK_REGISTRY: Dict[str, BenchTask] = {t.name: t for t in TASKS}

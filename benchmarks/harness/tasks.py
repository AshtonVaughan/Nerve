"""Canonical local benchmark tasks.

Each task is a callable that returns the next plan given an Observation.
That makes the harness model-free: the mock agent and a real adapter live
behind the same protocol.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Callable, Dict, List


@dataclass
class BenchTask:
    name: str
    description: str
    plan: List[Dict[str, Any]]
    expected_method_sequence: List[str] = field(default_factory=list)
    safety_dry_run: bool = True

    def succeeded(self, observed_methods: List[str]) -> bool:
        if not self.expected_method_sequence:
            return all(m != "no_op" or True for m in observed_methods)
        # require the expected methods to appear in order (with extras allowed
        # between them).
        idx = 0
        for m in observed_methods:
            if idx < len(self.expected_method_sequence) and m == self.expected_method_sequence[idx]:
                idx += 1
        return idx == len(self.expected_method_sequence)


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
    BenchTask(
        name="calculator_demo",
        description="Open calculator and add 2 + 3.",
        plan=[
            {"type": "open_app", "name": "calculator"},
            {"type": "wait", "ms": 800},
            {"type": "type_text", "text": "2+3="},
        ],
        expected_method_sequence=["native_ui_action", "wait", "keyboard"],
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
    BenchTask(
        name="clipboard_copy_paste",
        description="Round-trip clipboard set/get.",
        plan=[
            {"type": "clipboard_set", "text": "nerve clipboard demo"},
            {"type": "clipboard_get"},
        ],
        expected_method_sequence=["clipboard", "clipboard"],
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
    ),
]


TASK_REGISTRY: Dict[str, BenchTask] = {t.name: t for t in TASKS}

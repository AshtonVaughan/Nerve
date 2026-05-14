"""Common adapter protocol.

Every concrete adapter in ``agents/`` implements :class:`ModelAdapter`. The
runtime calls :meth:`ModelAdapter.plan` with the latest observation plus any
state the adapter is carrying, and gets back a list of dictionaries that are
JSON-compatible with the Nerve action protocol.

Adapters never touch the OS directly. Everything goes through the Nerve
runtime, which means safety, audit, and replay are uniform regardless of which
model is driving.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Dict, List, Protocol, runtime_checkable


@dataclass
class AdapterState:
    """Lightweight per-task state."""

    task: str = ""
    step: int = 0
    history: List[Dict[str, Any]] = field(default_factory=list)
    notes: List[str] = field(default_factory=list)
    done: bool = False


@runtime_checkable
class ModelAdapter(Protocol):
    name: str

    def plan(
        self, observation: Dict[str, Any], state: AdapterState
    ) -> List[Dict[str, Any]]:
        """Return the next batch of actions for Nerve to execute.

        Implementations must be pure: same observation + state should yield
        the same plan. Side-effects (network calls, file writes) belong in
        the adapter's constructor / context manager.
        """
        ...

"""Placeholder adapter for Anthropic Computer Use.

Real implementation would:

* take an Observation,
* convert it to a Claude message (screenshot as input_image),
* call ``messages.create`` with the `computer_20241022` tool,
* translate `tool_use` blocks back into Nerve actions.

Set ``ANTHROPIC_API_KEY`` before constructing the adapter.
"""

from __future__ import annotations

import os
from typing import Any, Dict, List

from ..base import AdapterState


class AnthropicComputerUseAdapter:
    name = "anthropic-computer-use"

    def __init__(self, model: str = "claude-sonnet-4-6") -> None:
        self.model = model
        self.api_key = os.environ.get("ANTHROPIC_API_KEY", "")
        # TODO: prompt caching for screenshots, tool config for `computer`.

    def plan(self, observation: Dict[str, Any], state: AdapterState) -> List[Dict[str, Any]]:
        if not self.api_key:
            raise RuntimeError(
                "ANTHROPIC_API_KEY is not set; "
                "Anthropic Computer Use adapter is a placeholder in the Nerve MVP."
            )
        raise NotImplementedError(
            "Anthropic Computer Use adapter is not implemented in the MVP."
        )

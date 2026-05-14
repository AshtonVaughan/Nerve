"""Placeholder adapter for OpenAI Computer Use (CUA).

Real implementation would:

* take an Observation,
* upload the screenshot to the Responses API along with the action history,
* parse the `tool_calls` in the response back into Nerve actions.

Set ``OPENAI_API_KEY`` in the environment before constructing the adapter.
"""

from __future__ import annotations

import os
from typing import Any, Dict, List

from ..base import AdapterState


class OpenAICuaAdapter:
    name = "openai-cua"

    def __init__(self, model: str = "computer-use-preview") -> None:
        self.model = model
        self.api_key = os.environ.get("OPENAI_API_KEY", "")
        # TODO: instantiate the openai client once we wire the real API call.

    def plan(self, observation: Dict[str, Any], state: AdapterState) -> List[Dict[str, Any]]:
        if not self.api_key:
            raise RuntimeError(
                "OPENAI_API_KEY is not set; "
                "OpenAI CUA adapter is a placeholder in the Nerve MVP."
            )
        # TODO: call the Responses API, parse tool_calls, return Nerve actions.
        raise NotImplementedError(
            "OpenAI CUA adapter is not implemented in the MVP. "
            "Use the mock agent for end-to-end runs."
        )

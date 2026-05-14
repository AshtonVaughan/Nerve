"""Placeholder Gemini adapter. See ``agents/openai/__init__.py`` for the shape."""

from __future__ import annotations

from typing import Any, Dict, List

from ..base import AdapterState


class GeminiAdapter:
    name = "gemini"

    def plan(self, observation: Dict[str, Any], state: AdapterState) -> List[Dict[str, Any]]:
        raise NotImplementedError("Gemini adapter is not implemented in the MVP.")

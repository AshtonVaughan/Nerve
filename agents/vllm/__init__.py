"""Placeholder vLLM adapter for self-hosted inference."""

from __future__ import annotations

from typing import Any, Dict, List

from ..base import AdapterState


class VllmAdapter:
    name = "vllm"

    def __init__(self, base_url: str = "http://localhost:8000/v1", model: str = "qwen2-vl"):
        self.base_url = base_url
        self.model = model

    def plan(self, observation: Dict[str, Any], state: AdapterState) -> List[Dict[str, Any]]:
        raise NotImplementedError("vLLM adapter is a placeholder in the Nerve MVP.")

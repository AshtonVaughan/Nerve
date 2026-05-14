"""Placeholder Ollama adapter for local models.

Real implementation would talk to ``http://localhost:11434/api/chat`` with a
multimodal prompt and a vision-capable local model (e.g. ``llava``,
``qwen2-vl``). The adapter is intentionally permissive about tool-call syntax
because local models still vary widely.
"""

from __future__ import annotations

from typing import Any, Dict, List

from ..base import AdapterState


class OllamaAdapter:
    name = "ollama"

    def __init__(self, model: str = "qwen2-vl", base_url: str = "http://localhost:11434"):
        self.model = model
        self.base_url = base_url

    def plan(self, observation: Dict[str, Any], state: AdapterState) -> List[Dict[str, Any]]:
        raise NotImplementedError("Ollama adapter is a placeholder in the Nerve MVP.")

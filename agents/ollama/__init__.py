"""Ollama adapter for local multimodal models.

Targets the ``/api/chat`` endpoint with images supplied as base64.
Local models still vary widely on tool-call syntax; we look for JSON
objects in the assistant's reply and parse them as actions.
"""

from __future__ import annotations

import json
import os
import re
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Any, Dict, List, Optional

from ..base import AdapterState


DEFAULT_BASE_URL = "http://localhost:11434"
DEFAULT_MODEL = "qwen2-vl"


@dataclass
class OllamaConfig:
    base_url: str = DEFAULT_BASE_URL
    model: str = DEFAULT_MODEL
    request_timeout_s: float = 120.0
    max_retries: int = 2


class OllamaAdapter:
    name = "ollama"

    def __init__(self, model: str = DEFAULT_MODEL, base_url: str = DEFAULT_BASE_URL) -> None:
        self.config = OllamaConfig(
            model=os.environ.get("OLLAMA_MODEL", model),
            base_url=os.environ.get("OLLAMA_BASE_URL", base_url),
        )

    def plan(self, observation: Dict[str, Any], state: AdapterState) -> List[Dict[str, Any]]:
        body = self._build(observation, state)
        resp = self._post("/api/chat", body)
        return self._parse(resp)

    def _build(self, observation: Dict[str, Any], state: AdapterState) -> Dict[str, Any]:
        screen = observation.get("screen", {})
        images = []
        if screen.get("screenshot_base64"):
            images.append(screen["screenshot_base64"])
        return {
            "model": self.config.model,
            "stream": False,
            "messages": [
                {
                    "role": "system",
                    "content": (
                        "You are a computer-use agent. Reply with a single JSON object "
                        "describing the next action. Schema: "
                        '{"type": "click|type_text|hotkey|screenshot|wait", ...}.'
                    ),
                },
                {
                    "role": "user",
                    "content": f"Task: {state.task}",
                    "images": images,
                },
            ],
        }

    def _post(self, path: str, body: Dict[str, Any]) -> Dict[str, Any]:
        url = self.config.base_url.rstrip("/") + path
        payload = json.dumps(body).encode("utf-8")
        attempt = 0
        while True:
            attempt += 1
            req = urllib.request.Request(url, data=payload)
            req.add_header("content-type", "application/json")
            try:
                with urllib.request.urlopen(req, timeout=self.config.request_timeout_s) as resp:
                    return json.loads(resp.read().decode("utf-8"))
            except (urllib.error.URLError, TimeoutError):
                if attempt > self.config.max_retries:
                    raise
                time.sleep(0.5 * attempt)

    def _parse(self, resp: Dict[str, Any]) -> List[Dict[str, Any]]:
        msg = resp.get("message", {}).get("content", "")
        if not isinstance(msg, str):
            return []
        # Find the first {...} JSON object in the reply.
        match = re.search(r"\{.*\}", msg, flags=re.DOTALL)
        if not match:
            return []
        try:
            parsed = json.loads(match.group(0))
        except json.JSONDecodeError:
            return []
        if isinstance(parsed, dict) and "type" in parsed:
            return [parsed]
        if isinstance(parsed, list):
            return [p for p in parsed if isinstance(p, dict) and "type" in p]
        return []

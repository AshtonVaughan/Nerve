"""vLLM adapter.

vLLM exposes an OpenAI-compatible chat endpoint, so this adapter delegates
to the same JSON-parse strategy as :class:`OllamaAdapter` while talking to
``/v1/chat/completions`` instead.
"""

from __future__ import annotations

import json
import os
import re
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Any, Dict, List

from ..base import AdapterState


DEFAULT_BASE_URL = "http://localhost:8000/v1"
DEFAULT_MODEL = "Qwen/Qwen2-VL-7B-Instruct"


@dataclass
class VllmConfig:
    base_url: str = DEFAULT_BASE_URL
    model: str = DEFAULT_MODEL
    api_key: str = "EMPTY"
    request_timeout_s: float = 120.0
    max_retries: int = 2


class VllmAdapter:
    name = "vllm"

    def __init__(self, model: str = DEFAULT_MODEL, base_url: str = DEFAULT_BASE_URL) -> None:
        self.config = VllmConfig(
            base_url=os.environ.get("VLLM_BASE_URL", base_url),
            model=os.environ.get("VLLM_MODEL", model),
            api_key=os.environ.get("VLLM_API_KEY", "EMPTY"),
        )

    def plan(self, observation: Dict[str, Any], state: AdapterState) -> List[Dict[str, Any]]:
        screen = observation.get("screen", {})
        content: List[Dict[str, Any]] = [{"type": "text", "text": f"Task: {state.task}"}]
        if screen.get("screenshot_base64"):
            content.append(
                {
                    "type": "image_url",
                    "image_url": {
                        "url": f"data:image/png;base64,{screen['screenshot_base64']}"
                    },
                }
            )
        body = {
            "model": self.config.model,
            "messages": [
                {
                    "role": "system",
                    "content": (
                        "You are a computer-use agent. Reply with a single JSON object "
                        "for the next action."
                    ),
                },
                {"role": "user", "content": content},
            ],
            "stream": False,
            "temperature": 0,
        }
        resp = self._post("/chat/completions", body)
        return self._parse(resp)

    def _post(self, path: str, body: Dict[str, Any]) -> Dict[str, Any]:
        url = self.config.base_url.rstrip("/") + path
        payload = json.dumps(body).encode("utf-8")
        attempt = 0
        while True:
            attempt += 1
            req = urllib.request.Request(url, data=payload)
            req.add_header("Authorization", f"Bearer {self.config.api_key}")
            req.add_header("content-type", "application/json")
            try:
                with urllib.request.urlopen(req, timeout=self.config.request_timeout_s) as resp:
                    return json.loads(resp.read().decode("utf-8"))
            except (urllib.error.URLError, TimeoutError):
                if attempt > self.config.max_retries:
                    raise
                time.sleep(0.5 * attempt)

    def _parse(self, resp: Dict[str, Any]) -> List[Dict[str, Any]]:
        choices = resp.get("choices", []) or []
        if not choices:
            return []
        msg = choices[0].get("message", {}).get("content", "")
        if not isinstance(msg, str):
            return []
        match = re.search(r"\{.*\}", msg, flags=re.DOTALL)
        if not match:
            return []
        try:
            parsed = json.loads(match.group(0))
        except json.JSONDecodeError:
            return []
        if isinstance(parsed, dict) and "type" in parsed:
            return [parsed]
        return []

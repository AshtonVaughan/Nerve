"""Gemini adapter.

Wraps the Google AI Studio Gemini ``generateContent`` API with the
``computer_use`` tool (preview as of writing). The wire shape mirrors
OpenAI / Anthropic adapters so callers can swap providers without touching
agent code.

Set ``GOOGLE_API_KEY`` (or ``GEMINI_API_KEY``) in the environment. The
adapter prefers a `:streamGenerateContent` endpoint when ``stream=True`` is
passed via :func:`set_streaming`.
"""

from __future__ import annotations

import json
import os
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Any, Dict, List, Optional

from ..base import AdapterState


DEFAULT_BASE_URL = "https://generativelanguage.googleapis.com/v1beta"
DEFAULT_MODEL = "gemini-2.0-flash"


@dataclass
class GeminiConfig:
    api_key: str
    base_url: str = DEFAULT_BASE_URL
    model: str = DEFAULT_MODEL
    request_timeout_s: float = 60.0
    max_retries: int = 4
    backoff_initial_s: float = 0.5
    cost_input_per_1m_usd: float = 0.1
    cost_output_per_1m_usd: float = 0.4


class GeminiAdapter:
    name = "gemini"

    def __init__(self, model: str = DEFAULT_MODEL, base_url: str = DEFAULT_BASE_URL) -> None:
        api_key = (
            os.environ.get("GOOGLE_API_KEY")
            or os.environ.get("GEMINI_API_KEY")
            or ""
        )
        self.config = GeminiConfig(api_key=api_key, base_url=base_url, model=model)

    def plan(self, observation: Dict[str, Any], state: AdapterState) -> List[Dict[str, Any]]:
        if not self.config.api_key:
            raise RuntimeError("GOOGLE_API_KEY / GEMINI_API_KEY not set")
        body = self._build_request(observation, state)
        resp = self._post(body)
        return self._translate(resp)

    # ---- internals --------------------------------------------------------

    def _build_request(self, observation: Dict[str, Any], state: AdapterState) -> Dict[str, Any]:
        screen = observation.get("screen", {})
        parts: List[Dict[str, Any]] = [
            {"text": f"Task: {state.task}\nDecide the next computer-use action."}
        ]
        if screen.get("screenshot_base64"):
            parts.append(
                {
                    "inlineData": {
                        "mimeType": "image/png",
                        "data": screen["screenshot_base64"],
                    }
                }
            )
        return {
            "contents": [
                {
                    "role": "user",
                    "parts": parts,
                }
            ],
            "tools": [{"computerUse": {}}],
            "generationConfig": {"temperature": 0.0},
        }

    def _post(self, body: Dict[str, Any]) -> Dict[str, Any]:
        path = f"/models/{self.config.model}:generateContent?key={self.config.api_key}"
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
            except urllib.error.HTTPError as e:
                if attempt > self.config.max_retries or e.code < 429:
                    raise
                time.sleep(self.config.backoff_initial_s * (2 ** (attempt - 1)))
            except (urllib.error.URLError, TimeoutError):
                if attempt > self.config.max_retries:
                    raise
                time.sleep(self.config.backoff_initial_s * (2 ** (attempt - 1)))

    def _translate(self, resp: Dict[str, Any]) -> List[Dict[str, Any]]:
        actions: List[Dict[str, Any]] = []
        candidates = resp.get("candidates", []) or []
        for cand in candidates:
            for part in (cand.get("content") or {}).get("parts", []) or []:
                if "functionCall" not in part:
                    continue
                call = part["functionCall"]
                if call.get("name") != "computer_use":
                    continue
                action_input = call.get("args", {})
                a = self._translate_action(action_input)
                if a is not None:
                    actions.append(a)
        return actions

    def _translate_action(self, action: Dict[str, Any]) -> Optional[Dict[str, Any]]:
        kind = action.get("action")
        if kind == "click":
            return {"type": "click", "x": int(action.get("x", 0)), "y": int(action.get("y", 0))}
        if kind == "type":
            return {"type": "type_text", "text": str(action.get("text", ""))}
        if kind == "hotkey":
            return {"type": "hotkey", "keys": list(action.get("keys", []))}
        if kind == "screenshot":
            return {"type": "screenshot"}
        return None

"""OpenAI Computer Use (CUA) adapter.

Wraps the Responses API ``computer_use_preview`` tool. The adapter takes an
Observation (containing a base64 screenshot), submits it to the model
alongside the task and history, and translates each emitted ``tool_call``
back into a Nerve action.

Requires ``OPENAI_API_KEY`` in the environment. The adapter relies only on
the ``urllib`` standard library so the SDK has no hard openai-python
dependency; callers can swap in the official client if they prefer.
"""

from __future__ import annotations

import base64
import json
import os
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Any, Dict, List, Optional

from ..base import AdapterState


DEFAULT_BASE_URL = "https://api.openai.com/v1"
DEFAULT_MODEL = "computer-use-preview"


@dataclass
class OpenAICuaConfig:
    api_key: str
    base_url: str = DEFAULT_BASE_URL
    model: str = DEFAULT_MODEL
    request_timeout_s: float = 60.0
    max_retries: int = 4
    backoff_initial_s: float = 0.5
    # Per-1M-token reference costs for the rough est_cost_usd metric.
    cost_input_per_1m_usd: float = 3.0
    cost_output_per_1m_usd: float = 12.0


class OpenAICuaAdapter:
    name = "openai-cua"

    def __init__(self, model: str = DEFAULT_MODEL, base_url: str = DEFAULT_BASE_URL) -> None:
        api_key = os.environ.get("OPENAI_API_KEY", "")
        self.config = OpenAICuaConfig(api_key=api_key, base_url=base_url, model=model)

    # The protocol: take an observation + state, return a list of actions.
    def plan(self, observation: Dict[str, Any], state: AdapterState) -> List[Dict[str, Any]]:
        if not self.config.api_key:
            raise RuntimeError("OPENAI_API_KEY is not set")

        body = self._build_request(observation, state)
        resp = self._post("/responses", body)
        actions, usage = self._translate(resp)

        # Cost accounting on the harness side.
        if usage:
            tokens_in = usage.get("input_tokens", 0)
            tokens_out = usage.get("output_tokens", 0)
            cost = (
                tokens_in / 1_000_000.0 * self.config.cost_input_per_1m_usd
                + tokens_out / 1_000_000.0 * self.config.cost_output_per_1m_usd
            )
            state.notes.append(f"openai-cost-usd={cost:.6f}")
        return actions

    # ---- internals --------------------------------------------------------

    def _build_request(self, observation: Dict[str, Any], state: AdapterState) -> Dict[str, Any]:
        screen = observation.get("screen", {})
        screenshot_b64 = screen.get("screenshot_base64")
        history = [
            {"type": "input_text", "text": f"Task: {state.task}"},
        ]
        if state.history:
            history.append(
                {
                    "type": "input_text",
                    "text": "Previously: " + json.dumps(state.history[-5:]),
                }
            )
        if screenshot_b64:
            history.append(
                {
                    "type": "input_image",
                    "image_url": f"data:image/png;base64,{screenshot_b64}",
                }
            )
        return {
            "model": self.config.model,
            "input": [{"role": "user", "content": history}],
            "tools": [
                {
                    "type": "computer_use_preview",
                    "display_width": int(screen.get("width", 1920)),
                    "display_height": int(screen.get("height", 1080)),
                    "environment": observation.get("platform", "linux"),
                }
            ],
            "tool_choice": "auto",
            "stream": False,
            "truncation": "auto",
        }

    def _post(self, path: str, body: Dict[str, Any]) -> Dict[str, Any]:
        url = self.config.base_url.rstrip("/") + path
        payload = json.dumps(body).encode("utf-8")
        attempt = 0
        while True:
            attempt += 1
            req = urllib.request.Request(url, data=payload)
            req.add_header("Authorization", f"Bearer {self.config.api_key}")
            req.add_header("Content-Type", "application/json")
            try:
                with urllib.request.urlopen(req, timeout=self.config.request_timeout_s) as resp:
                    return json.loads(resp.read().decode("utf-8"))
            except urllib.error.HTTPError as e:
                # Retry on 429 + 5xx with exponential backoff.
                if attempt > self.config.max_retries or e.code < 429 or (e.code == 429 and attempt > self.config.max_retries):
                    raise
                delay = self.config.backoff_initial_s * (2 ** (attempt - 1))
                time.sleep(delay)
            except (urllib.error.URLError, TimeoutError):
                if attempt > self.config.max_retries:
                    raise
                delay = self.config.backoff_initial_s * (2 ** (attempt - 1))
                time.sleep(delay)

    def _translate(self, resp: Dict[str, Any]) -> tuple[List[Dict[str, Any]], Optional[Dict[str, int]]]:
        actions: List[Dict[str, Any]] = []
        usage = resp.get("usage")
        for item in resp.get("output", []) or []:
            if item.get("type") != "computer_use":
                continue
            for action in item.get("actions", []) or []:
                translated = self._translate_action(action)
                if translated is not None:
                    actions.append(translated)
        return actions, usage

    def _translate_action(self, action: Dict[str, Any]) -> Optional[Dict[str, Any]]:
        kind = action.get("type")
        if kind == "click":
            return {
                "type": "click",
                "x": int(action.get("x", 0)),
                "y": int(action.get("y", 0)),
                "button": action.get("button", "left"),
            }
        if kind == "double_click":
            return {"type": "double_click", "x": int(action["x"]), "y": int(action["y"])}
        if kind == "type":
            return {"type": "type_text", "text": str(action.get("text", ""))}
        if kind == "key":
            return {"type": "hotkey", "keys": list(action.get("keys", []))}
        if kind == "scroll":
            return {
                "type": "scroll",
                "x": int(action.get("x", 0)),
                "y": int(action.get("y", 0)),
                "delta_x": int(action.get("delta_x", 0)),
                "delta_y": int(action.get("delta_y", 0)),
            }
        if kind == "wait":
            return {"type": "wait", "ms": int(action.get("ms", 500))}
        if kind == "screenshot":
            return {"type": "screenshot"}
        return None

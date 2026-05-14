"""Anthropic Computer Use adapter.

Wraps the Messages API with the ``computer_20241022`` (and successor)
tools. Translates ``tool_use`` blocks back into Nerve actions. Uses prompt
caching breakpoints around the system prompt + tool definition so repeated
agent loops only pay for the marginal screenshot.
"""

from __future__ import annotations

import json
import os
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional

from ..base import AdapterState


DEFAULT_BASE_URL = "https://api.anthropic.com"
DEFAULT_API_VERSION = "2023-06-01"
DEFAULT_MODEL = "claude-sonnet-4-6"


@dataclass
class AnthropicConfig:
    api_key: str
    base_url: str = DEFAULT_BASE_URL
    model: str = DEFAULT_MODEL
    api_version: str = DEFAULT_API_VERSION
    request_timeout_s: float = 60.0
    max_retries: int = 4
    backoff_initial_s: float = 0.5
    cost_input_per_1m_usd: float = 3.0
    cost_output_per_1m_usd: float = 15.0
    cache_read_per_1m_usd: float = 0.3
    cache_write_per_1m_usd: float = 3.75
    extra_betas: List[str] = field(default_factory=lambda: ["computer-use-2024-10-22"])


class AnthropicComputerUseAdapter:
    name = "anthropic-computer-use"

    def __init__(self, model: str = DEFAULT_MODEL, base_url: str = DEFAULT_BASE_URL) -> None:
        api_key = os.environ.get("ANTHROPIC_API_KEY", "")
        self.config = AnthropicConfig(api_key=api_key, base_url=base_url, model=model)
        # Multi-turn conversation memory keyed by AdapterState.
        self._conversations: Dict[int, List[Dict[str, Any]]] = {}

    def plan(self, observation: Dict[str, Any], state: AdapterState) -> List[Dict[str, Any]]:
        if not self.config.api_key:
            raise RuntimeError("ANTHROPIC_API_KEY is not set")

        body = self._build_request(observation, state)
        resp = self._post("/v1/messages", body)
        actions, usage = self._translate(resp, state)

        if usage:
            cost = self._estimate_cost(usage)
            state.notes.append(f"anthropic-cost-usd={cost:.6f}")
        return actions

    # ---- internals --------------------------------------------------------

    def _build_request(self, observation: Dict[str, Any], state: AdapterState) -> Dict[str, Any]:
        screen = observation.get("screen", {})
        width = int(screen.get("width", 1920))
        height = int(screen.get("height", 1080))
        screenshot_b64 = screen.get("screenshot_base64")

        convo = self._conversations.setdefault(id(state), [])
        # Append the latest observation as a user message.
        user_content: List[Dict[str, Any]] = [{"type": "text", "text": f"Task: {state.task}"}]
        if screenshot_b64:
            user_content.append(
                {
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/png",
                        "data": screenshot_b64,
                    },
                }
            )
        convo.append({"role": "user", "content": user_content})

        # Prompt-caching breakpoints: cache the system + tools (most stable
        # parts) so repeated turns only pay for the deltas.
        system = [
            {
                "type": "text",
                "text": (
                    "You are operating a computer through the Nerve runtime. "
                    "Use the provided computer tool to accomplish the task. "
                    "Always wait for the screen to settle before issuing the next action."
                ),
                "cache_control": {"type": "ephemeral"},
            }
        ]
        tools = [
            {
                "type": "computer_20241022",
                "name": "computer",
                "display_width_px": width,
                "display_height_px": height,
                "display_number": 1,
                "cache_control": {"type": "ephemeral"},
            }
        ]
        return {
            "model": self.config.model,
            "max_tokens": 1024,
            "system": system,
            "tools": tools,
            "messages": convo,
            "anthropic_beta": self.config.extra_betas,
        }

    def _post(self, path: str, body: Dict[str, Any]) -> Dict[str, Any]:
        url = self.config.base_url.rstrip("/") + path
        payload = json.dumps(body).encode("utf-8")
        attempt = 0
        while True:
            attempt += 1
            req = urllib.request.Request(url, data=payload)
            req.add_header("x-api-key", self.config.api_key)
            req.add_header("anthropic-version", self.config.api_version)
            req.add_header("anthropic-beta", ",".join(self.config.extra_betas))
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

    def _translate(
        self, resp: Dict[str, Any], state: AdapterState
    ) -> tuple[List[Dict[str, Any]], Optional[Dict[str, int]]]:
        actions: List[Dict[str, Any]] = []
        usage = resp.get("usage")
        assistant_content = resp.get("content", []) or []
        # Record the assistant turn for future calls.
        convo = self._conversations.setdefault(id(state), [])
        convo.append({"role": "assistant", "content": assistant_content})

        for block in assistant_content:
            if block.get("type") != "tool_use":
                continue
            if block.get("name") != "computer":
                continue
            action_input = block.get("input", {})
            translated = self._translate_action(action_input)
            if translated is not None:
                actions.append(translated)
        return actions, usage

    def _translate_action(self, action: Dict[str, Any]) -> Optional[Dict[str, Any]]:
        kind = action.get("action")
        if kind == "screenshot":
            return {"type": "screenshot"}
        if kind == "left_click":
            xy = action.get("coordinate", [0, 0])
            return {"type": "click", "x": int(xy[0]), "y": int(xy[1])}
        if kind == "right_click":
            xy = action.get("coordinate", [0, 0])
            return {"type": "right_click", "x": int(xy[0]), "y": int(xy[1])}
        if kind == "double_click":
            xy = action.get("coordinate", [0, 0])
            return {"type": "double_click", "x": int(xy[0]), "y": int(xy[1])}
        if kind == "mouse_move":
            xy = action.get("coordinate", [0, 0])
            return {"type": "move_mouse", "x": int(xy[0]), "y": int(xy[1])}
        if kind == "type":
            return {"type": "type_text", "text": str(action.get("text", ""))}
        if kind == "key":
            text = str(action.get("text", ""))
            return {"type": "hotkey", "keys": [t.strip() for t in text.split("+") if t.strip()]}
        if kind == "left_click_drag":
            start = action.get("start_coordinate", [0, 0])
            end = action.get("coordinate", [0, 0])
            return {
                "type": "drag",
                "from_x": int(start[0]),
                "from_y": int(start[1]),
                "to_x": int(end[0]),
                "to_y": int(end[1]),
            }
        if kind == "wait":
            return {"type": "wait", "ms": int(action.get("ms", 500))}
        return None

    def _estimate_cost(self, usage: Dict[str, int]) -> float:
        tokens_in = usage.get("input_tokens", 0)
        tokens_out = usage.get("output_tokens", 0)
        cache_read = usage.get("cache_read_input_tokens", 0)
        cache_write = usage.get("cache_creation_input_tokens", 0)
        return (
            tokens_in / 1_000_000.0 * self.config.cost_input_per_1m_usd
            + tokens_out / 1_000_000.0 * self.config.cost_output_per_1m_usd
            + cache_read / 1_000_000.0 * self.config.cache_read_per_1m_usd
            + cache_write / 1_000_000.0 * self.config.cache_write_per_1m_usd
        )

"""Anthropic Computer Use adapter.

Wraps the Messages API with the ``computer_20250124`` tool. Implements the
full Computer Use loop: every ``tool_use`` block the model emits is
translated into a Nerve action AND tracked so the next turn can send a
matching ``tool_result`` block back. Without that, the API refuses any
second turn with ``tool_use ids were found without tool_result blocks
immediately after``.

The adapter is stateless per task: a new ``AdapterState`` instance resets
the conversation, so the demo and benchmark harness can reuse the adapter
across tasks without manual bookkeeping.
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
DEFAULT_MODEL = "claude-haiku-4-5-20251001"


@dataclass
class AnthropicConfig:
    api_key: str
    base_url: str = DEFAULT_BASE_URL
    model: str = DEFAULT_MODEL
    api_version: str = DEFAULT_API_VERSION
    request_timeout_s: float = 60.0
    max_retries: int = 4
    backoff_initial_s: float = 0.5
    max_tokens: int = 4096
    cost_input_per_1m_usd: float = 1.0
    cost_output_per_1m_usd: float = 5.0
    cache_read_per_1m_usd: float = 0.1
    cache_write_per_1m_usd: float = 1.25
    extra_betas: List[str] = field(
        default_factory=lambda: ["computer-use-2025-01-24"]
    )


class AnthropicComputerUseAdapter:
    name = "anthropic-computer-use"

    def __init__(
        self,
        model: str = DEFAULT_MODEL,
        base_url: str = DEFAULT_BASE_URL,
    ) -> None:
        api_key = os.environ.get("ANTHROPIC_API_KEY", "")
        self.config = AnthropicConfig(
            api_key=api_key, base_url=base_url, model=model
        )
        # Per-state conversation memory and pending tool_use bookkeeping.
        # Keyed by id(state) - run_demo creates a fresh AdapterState per task,
        # so this resets correctly between tasks without an explicit clear.
        self._conversations: Dict[int, List[Dict[str, Any]]] = {}
        self._pending_tool_uses: Dict[int, List[Dict[str, Any]]] = {}

    def plan(
        self, observation: Dict[str, Any], state: AdapterState
    ) -> List[Dict[str, Any]]:
        if not self.config.api_key:
            raise RuntimeError("ANTHROPIC_API_KEY is not set")

        screen = observation.get("screen", {}) or {}
        width = int(screen.get("width", 1920))
        height = int(screen.get("height", 1080))
        screenshot_b64 = screen.get("screenshot_base64")

        convo = self._conversations.setdefault(id(state), [])
        pending = self._pending_tool_uses.pop(id(state), [])

        if pending:
            # Round N+1: emit a tool_result for every prior tool_use. Including
            # the fresh screenshot inside each tool_result is what lets the
            # model see the effect of its action before deciding the next one.
            tool_results: List[Dict[str, Any]] = []
            for tu in pending:
                content: List[Dict[str, Any]] = []
                if screenshot_b64:
                    content.append(
                        {
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": "image/png",
                                "data": screenshot_b64,
                            },
                        }
                    )
                else:
                    content.append({"type": "text", "text": "action complete"})
                tool_results.append(
                    {
                        "type": "tool_result",
                        "tool_use_id": tu["id"],
                        "content": content,
                    }
                )
            convo.append({"role": "user", "content": tool_results})
        else:
            # Round 1: send the task prompt + initial screenshot.
            user_content: List[Dict[str, Any]] = [
                {"type": "text", "text": f"Task: {state.task}"}
            ]
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

        # Prompt-caching breakpoints on the two most stable parts of the
        # request: the system prompt and the tool definition. Multi-turn
        # cost is then dominated by the marginal screenshot per turn.
        system = [
            {
                "type": "text",
                "text": (
                    "You are operating a real computer through the Nerve "
                    "runtime. Use the provided `computer` tool to accomplish "
                    "the task. After each tool call, you will receive a "
                    "screenshot showing the effect of your action. Always "
                    "verify the screen before issuing the next action. When "
                    "the task is complete, respond with a brief text "
                    "summary of what you did and DO NOT call the tool again."
                ),
                "cache_control": {"type": "ephemeral"},
            }
        ]
        tools = [
            {
                "type": "computer_20250124",
                "name": "computer",
                "display_width_px": width,
                "display_height_px": height,
                "display_number": 1,
                "cache_control": {"type": "ephemeral"},
            }
        ]
        body = {
            "model": self.config.model,
            "max_tokens": self.config.max_tokens,
            "system": system,
            "tools": tools,
            "messages": convo,
        }
        resp = self._post("/v1/messages", body)
        actions, usage = self._translate(resp, state)

        if usage:
            cost = self._estimate_cost(usage)
            state.notes.append(f"anthropic-cost-usd={cost:.6f}")
        return actions

    # ---- internals --------------------------------------------------------

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
                with urllib.request.urlopen(
                    req, timeout=self.config.request_timeout_s
                ) as resp:
                    return json.loads(resp.read().decode("utf-8"))
            except urllib.error.HTTPError as e:
                # 4xx is never retryable; surface the full body so the caller
                # can see *which* field the API rejected.
                if attempt > self.config.max_retries or e.code < 429:
                    err_body = e.read().decode("utf-8", errors="replace")
                    raise RuntimeError(
                        f"anthropic {e.code}: {err_body}"
                    ) from e
                time.sleep(
                    self.config.backoff_initial_s * (2 ** (attempt - 1))
                )
            except (urllib.error.URLError, TimeoutError):
                if attempt > self.config.max_retries:
                    raise
                time.sleep(
                    self.config.backoff_initial_s * (2 ** (attempt - 1))
                )

    def _translate(
        self, resp: Dict[str, Any], state: AdapterState
    ) -> tuple[List[Dict[str, Any]], Optional[Dict[str, int]]]:
        actions: List[Dict[str, Any]] = []
        usage = resp.get("usage")
        assistant_content = resp.get("content", []) or []
        stop_reason = resp.get("stop_reason")

        # Record the assistant turn so the next request's `messages` array is
        # well-formed (the API enforces strict alternation).
        convo = self._conversations.setdefault(id(state), [])
        convo.append({"role": "assistant", "content": assistant_content})

        pending: List[Dict[str, Any]] = []
        for block in assistant_content:
            if block.get("type") != "tool_use":
                continue
            if block.get("name") != "computer":
                continue
            tu_id = block.get("id")
            if not tu_id:
                continue
            action_input = block.get("input", {}) or {}
            translated = self._translate_action(action_input)
            # We always record the tool_use_id so a tool_result goes back,
            # even when the action isn't translatable. The next turn's
            # tool_result then carries a text "unsupported action" and the
            # model can adapt instead of deadlocking.
            pending.append({"id": tu_id, "input": action_input})
            if translated is not None:
                actions.append(translated)

        if pending:
            self._pending_tool_uses[id(state)] = pending

        # The model can finish in two ways:
        #   1. Returns text only with stop_reason="end_turn".
        #   2. Returns text + tool_use, but our caller will execute the tool
        #      and call plan() again; that next call will see no tool_use and
        #      we'll mark done then.
        if not pending and stop_reason in ("end_turn", "stop_sequence", "max_tokens"):
            state.done = True

        return actions, usage

    def _translate_action(
        self, action: Dict[str, Any]
    ) -> Optional[Dict[str, Any]]:
        kind = action.get("action")
        if kind == "screenshot":
            return {"type": "screenshot"}
        if kind == "key":
            text = str(action.get("text", ""))
            keys = [t.strip() for t in text.split("+") if t.strip()]
            if not keys:
                return None
            if len(keys) == 1:
                return {"type": "key_press", "key": keys[0]}
            return {"type": "hotkey", "keys": keys}
        if kind == "hold_key":
            # Best-effort: collapse to a hotkey press. The daemon doesn't
            # have a hold-for-N-seconds primitive yet.
            text = str(action.get("text", ""))
            keys = [t.strip() for t in text.split("+") if t.strip()]
            if not keys:
                return None
            return {"type": "hotkey", "keys": keys}
        if kind == "type":
            return {
                "type": "type_text",
                "text": str(action.get("text", "")),
                "unicode_paste": False,
            }
        if kind == "cursor_position":
            # No-op observation: just take a screenshot so the next turn
            # gets a fresh view.
            return {"type": "screenshot"}
        if kind == "mouse_move":
            xy = action.get("coordinate", [0, 0])
            return {"type": "move_mouse", "x": int(xy[0]), "y": int(xy[1])}
        if kind in ("left_click", "left_mouse_down", "left_mouse_up"):
            xy = action.get("coordinate", [0, 0])
            return {
                "type": "click",
                "x": int(xy[0]),
                "y": int(xy[1]),
                "button": "left",
            }
        if kind == "right_click":
            xy = action.get("coordinate", [0, 0])
            return {"type": "right_click", "x": int(xy[0]), "y": int(xy[1])}
        if kind == "middle_click":
            xy = action.get("coordinate", [0, 0])
            return {
                "type": "click",
                "x": int(xy[0]),
                "y": int(xy[1]),
                "button": "middle",
            }
        if kind == "double_click":
            xy = action.get("coordinate", [0, 0])
            return {
                "type": "double_click",
                "x": int(xy[0]),
                "y": int(xy[1]),
            }
        if kind == "triple_click":
            # No native triple-click; emulate as two double-clicks; the second
            # extends the selection in most editors. Good enough for the demo.
            xy = action.get("coordinate", [0, 0])
            return {
                "type": "double_click",
                "x": int(xy[0]),
                "y": int(xy[1]),
            }
        if kind == "left_click_drag":
            start = action.get("start_coordinate", [0, 0])
            end = action.get("coordinate", [0, 0])
            return {
                "type": "drag",
                "from_x": int(start[0]),
                "from_y": int(start[1]),
                "to_x": int(end[0]),
                "to_y": int(end[1]),
                "button": "left",
            }
        if kind == "scroll":
            xy = action.get("coordinate", [0, 0])
            direction = str(action.get("scroll_direction", "down"))
            amount = int(action.get("scroll_amount", 3))
            # Nerve's Scroll takes signed deltas in pixels; convert direction
            # + amount into per-tick deltas of 100px each (matches what most
            # OS scrollers consider one notch).
            dx, dy = 0, 0
            step = 100
            if direction == "up":
                dy = -step * amount
            elif direction == "down":
                dy = step * amount
            elif direction == "left":
                dx = -step * amount
            elif direction == "right":
                dx = step * amount
            return {
                "type": "scroll",
                "x": int(xy[0]),
                "y": int(xy[1]),
                "delta_x": dx,
                "delta_y": dy,
            }
        if kind == "wait":
            # 2025-01-24 spec: `duration` in seconds (float). Older code used
            # `ms`; accept both for safety.
            if "duration" in action:
                ms = int(float(action["duration"]) * 1000)
            else:
                ms = int(action.get("ms", 500))
            return {"type": "wait", "ms": ms}
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

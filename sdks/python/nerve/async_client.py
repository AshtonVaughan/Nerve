"""Asyncio Nerve client."""

from __future__ import annotations

import asyncio
import json
import os
import uuid
from typing import Any, AsyncIterator, Awaitable, Callable, Dict, List, Optional

import websockets


def _env_token() -> Optional[str]:
    t = os.environ.get("NERVE_AUTH_TOKEN")
    return t if t else None

from .types import (
    ActionEnvelope,
    ActionResult,
    AuditEntry,
    Capabilities,
    ElementTarget,
    Observation,
    SafetyPolicy,
)


class NerveError(RuntimeError):
    def __init__(self, code: str, message: str):
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message


class AsyncNerveClient:
    """Asyncio Nerve client.

    Typical usage::

        async with AsyncNerveClient() as client:
            obs = await client.get_observation()
            await client.click_element(text="Save", role="button")

    The client automatically reconnects (with exponential backoff) when the
    underlying WebSocket drops. Disable by passing ``auto_reconnect=False``.
    """

    def __init__(
        self,
        host: str = "127.0.0.1",
        port: int = 8765,
        client_name: str = "nerve-python",
        auth_token: Optional[str] = None,
        auto_reconnect: bool = True,
        reconnect_initial_s: float = 0.5,
        reconnect_max_s: float = 30.0,
    ):
        self.host = host
        self.port = port
        self.client_name = client_name
        self.auth_token = auth_token or _env_token()
        self._auto_reconnect = auto_reconnect
        self._reconnect_initial_s = reconnect_initial_s
        self._reconnect_max_s = reconnect_max_s
        self._ws: Optional[websockets.WebSocketClientProtocol] = None  # type: ignore[name-defined]
        self._session_id: Optional[str] = None
        self._inbox: "asyncio.Queue[Dict[str, Any]]" = asyncio.Queue()
        self._reader_task: Optional[asyncio.Task] = None
        self._pending: Dict[str, "asyncio.Future[Dict[str, Any]]"] = {}
        self._unsolicited: List[Callable[[Dict[str, Any]], Awaitable[None]]] = []
        self._closed = False
        self._last_policy: Optional[SafetyPolicy] = None

    # -- lifecycle --------------------------------------------------------

    async def connect(self, policy: Optional[SafetyPolicy] = None) -> str:
        self._last_policy = policy
        attempt = 0
        delay = self._reconnect_initial_s
        while True:
            try:
                return await self._connect_once(policy=policy)
            except Exception as e:
                if not self._auto_reconnect or self._closed:
                    raise
                attempt += 1
                if attempt > 8:  # bounded for connect path
                    raise
                await asyncio.sleep(delay)
                delay = min(delay * 2, self._reconnect_max_s)

    async def _connect_once(self, policy: Optional[SafetyPolicy] = None) -> str:
        self._ws = await websockets.connect(f"ws://{self.host}:{self.port}/")
        self._reader_task = asyncio.create_task(self._reader())
        # Drain the daemon's `hello`.
        await self._inbox.get()
        resp = await self._request(
            {
                "kind": "session_start",
                "client_name": self.client_name,
                "client_version": "0.1.0",
                "client_protocol_version": {"major": 0, "minor": 1, "patch": 0},
                "auth_token": self.auth_token,
                "session_id": self._session_id,
                "policy": policy.to_dict() if policy else None,
            }
        )
        if resp.get("kind") == "error":
            raise NerveError(resp.get("code", "unknown"), resp.get("message", ""))
        self._session_id = resp["session_id"]
        return self._session_id

    async def stop(self) -> None:
        if self._closed:
            return
        self._closed = True
        if self._ws is not None and self._session_id is not None:
            try:
                await self._request({"kind": "session_stop"})
            except Exception:
                pass
            try:
                await self._ws.close()
            except Exception:
                pass
        if self._reader_task is not None:
            self._reader_task.cancel()

    async def __aenter__(self) -> "AsyncNerveClient":
        await self.connect()
        return self

    async def __aexit__(self, *_exc) -> None:
        await self.stop()

    # -- core protocol ----------------------------------------------------

    async def get_capabilities(self) -> Capabilities:
        resp = await self._request({"kind": "get_capabilities"})
        self._check(resp)
        return Capabilities(raw=resp["capabilities"])

    async def get_observation(
        self,
        include_screenshot: bool = True,
        include_ui_tree: bool = False,
    ) -> Observation:
        resp = await self._request(
            {
                "kind": "get_observation",
                "include_screenshot": include_screenshot,
                "include_ui_tree": include_ui_tree,
            }
        )
        self._check(resp)
        return Observation(raw=resp["observation"])

    async def subscribe_observations(
        self,
        interval_ms: int = 500,
        include_screenshot: bool = False,
    ) -> AsyncIterator[Observation]:
        """Yield observations as the daemon streams them.

        Cancellation: cancel the surrounding task to stop. The daemon will
        keep streaming until the WebSocket closes.
        """
        request_id = self._new_request_id()
        queue: "asyncio.Queue[Dict[str, Any]]" = asyncio.Queue()

        async def handler(msg: Dict[str, Any]) -> None:
            if msg.get("kind") == "observation" and msg.get("request_id") == request_id:
                await queue.put(msg["observation"])

        self._unsolicited.append(handler)
        try:
            await self._send(
                {
                    "kind": "subscribe_observations",
                    "request_id": request_id,
                    "interval_ms": interval_ms,
                    "include_screenshot": include_screenshot,
                }
            )
            while True:
                obs = await queue.get()
                yield Observation(raw=obs)
        finally:
            try:
                self._unsolicited.remove(handler)
            except ValueError:
                pass

    async def execute(
        self,
        action: Dict[str, Any],
        note: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> ActionResult:
        envelope = ActionEnvelope(
            id=f"act_{uuid.uuid4().hex}", action=action, note=note
        )
        env_dict = envelope.to_dict()
        if idempotency_key is not None:
            env_dict["idempotency_key"] = idempotency_key
        resp = await self._request({"kind": "execute_action", "action": env_dict})
        self._check(resp)
        return ActionResult.from_dict(resp["result"])

    async def execute_batch(
        self, actions: List[Dict[str, Any]], stop_on_error: bool = True
    ) -> List[ActionResult]:
        envelopes = [
            ActionEnvelope(id=f"act_{uuid.uuid4().hex}", action=a).to_dict() for a in actions
        ]
        resp = await self._request(
            {
                "kind": "execute_action_batch",
                "actions": envelopes,
                "stop_on_error": stop_on_error,
            }
        )
        self._check(resp)
        return [ActionResult.from_dict(r) for r in resp["results"]]

    async def get_action_log(
        self, session_id: Optional[str] = None, limit: Optional[int] = None
    ) -> List[AuditEntry]:
        resp = await self._request(
            {
                "kind": "get_action_log",
                "session_id": session_id,
                "limit": limit,
            }
        )
        self._check(resp)
        return [AuditEntry(raw=e) for e in resp["entries"]]

    async def set_safety_policy(self, policy: SafetyPolicy) -> SafetyPolicy:
        resp = await self._request(
            {"kind": "set_safety_policy", "policy": policy.to_dict()}
        )
        self._check(resp)
        return policy

    async def emergency_stop(self) -> None:
        await self._request({"kind": "emergency_stop"})

    # -- ergonomic helpers ------------------------------------------------

    async def click(self, x: int, y: int, button: str = "left") -> ActionResult:
        return await self.execute({"type": "click", "x": x, "y": y, "button": button})

    async def double_click(self, x: int, y: int) -> ActionResult:
        return await self.execute({"type": "double_click", "x": x, "y": y})

    async def right_click(self, x: int, y: int) -> ActionResult:
        return await self.execute({"type": "right_click", "x": x, "y": y})

    async def drag(self, from_xy: tuple, to_xy: tuple, button: str = "left") -> ActionResult:
        return await self.execute(
            {
                "type": "drag",
                "from_x": from_xy[0],
                "from_y": from_xy[1],
                "to_x": to_xy[0],
                "to_y": to_xy[1],
                "button": button,
            }
        )

    async def scroll(self, x: int, y: int, delta_x: int = 0, delta_y: int = 0) -> ActionResult:
        return await self.execute(
            {"type": "scroll", "x": x, "y": y, "delta_x": delta_x, "delta_y": delta_y}
        )

    async def type_text(self, text: str, delay_ms: Optional[int] = None) -> ActionResult:
        return await self.execute({"type": "type_text", "text": text, "delay_ms": delay_ms})

    async def hotkey(self, keys: List[str]) -> ActionResult:
        return await self.execute({"type": "hotkey", "keys": keys})

    async def key_press(self, key: str) -> ActionResult:
        return await self.execute({"type": "key_press", "key": key})

    async def clipboard_get(self) -> str:
        result = await self.execute({"type": "clipboard_get"})
        if result.data and "text" in result.data:
            return result.data["text"]
        return ""

    async def clipboard_set(self, text: str) -> ActionResult:
        return await self.execute({"type": "clipboard_set", "text": text})

    async def click_element(
        self,
        *,
        text: Optional[str] = None,
        role: Optional[str] = None,
        app: Optional[str] = None,
        bounds: Optional[Dict[str, int]] = None,
        index: Optional[int] = None,
    ) -> ActionResult:
        target = ElementTarget(text=text, role=role, app=app, bounds=bounds, index=index).to_dict()
        return await self.execute({"type": "click_element", "target": target})

    async def open_app(self, name: str) -> ActionResult:
        return await self.execute({"type": "open_app", "name": name})

    async def wait_for_text(self, text: str, timeout_ms: int = 5000) -> ActionResult:
        return await self.execute(
            {"type": "wait_for_text", "text": text, "timeout_ms": timeout_ms}
        )

    # -- internals --------------------------------------------------------

    def _new_request_id(self) -> str:
        return f"py_{uuid.uuid4().hex}"

    async def _send(self, payload: Dict[str, Any]) -> None:
        if self._ws is None:
            raise RuntimeError("client not connected")
        await self._ws.send(json.dumps(payload))

    async def _request(self, payload: Dict[str, Any]) -> Dict[str, Any]:
        request_id = payload.get("request_id") or self._new_request_id()
        payload["request_id"] = request_id
        fut: asyncio.Future[Dict[str, Any]] = asyncio.get_event_loop().create_future()
        self._pending[request_id] = fut
        try:
            await self._send(payload)
            return await fut
        finally:
            self._pending.pop(request_id, None)

    async def _reader(self) -> None:
        try:
            assert self._ws is not None
            async for raw in self._ws:
                try:
                    msg = json.loads(raw)
                except json.JSONDecodeError:
                    continue
                rid = msg.get("request_id")
                if rid and rid in self._pending:
                    fut = self._pending[rid]
                    if not fut.done():
                        fut.set_result(msg)
                else:
                    for h in list(self._unsolicited):
                        try:
                            await h(msg)
                        except Exception:
                            pass
                    if not rid:
                        await self._inbox.put(msg)
        except Exception:
            return

    @staticmethod
    def _check(msg: Dict[str, Any]) -> None:
        if msg.get("kind") == "error":
            raise NerveError(msg.get("code", "unknown"), msg.get("message", ""))

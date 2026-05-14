"""Synchronous wrapper around :class:`AsyncNerveClient`.

The wrapper owns its own asyncio loop in a dedicated thread so blocking
callers (CLI scripts, Jupyter notebooks, the demo agent) can use the API
without touching ``async`` themselves.
"""

from __future__ import annotations

import asyncio
import threading
from typing import Any, Dict, Iterator, List, Optional

from .async_client import AsyncNerveClient, NerveError
from .types import (
    ActionResult,
    AuditEntry,
    Capabilities,
    Observation,
    SafetyPolicy,
)


class NerveClient:
    """Blocking Nerve client.

    Mirrors the methods on :class:`AsyncNerveClient` 1:1.
    """

    def __init__(self, host: str = "127.0.0.1", port: int = 8765, client_name: str = "nerve-python"):
        self._async = AsyncNerveClient(host=host, port=port, client_name=client_name)
        self._loop = asyncio.new_event_loop()
        self._thread = threading.Thread(target=self._loop.run_forever, name="nerve-loop", daemon=True)
        self._thread.start()

    # -- run helper -------------------------------------------------------

    def _run(self, coro):
        future = asyncio.run_coroutine_threadsafe(coro, self._loop)
        return future.result()

    # -- lifecycle --------------------------------------------------------

    def connect(self, policy: Optional[SafetyPolicy] = None) -> str:
        return self._run(self._async.connect(policy=policy))

    def stop(self) -> None:
        try:
            self._run(self._async.stop())
        finally:
            self._loop.call_soon_threadsafe(self._loop.stop)
            self._thread.join(timeout=2.0)

    def __enter__(self) -> "NerveClient":
        self.connect()
        return self

    def __exit__(self, *_exc) -> None:
        self.stop()

    # -- proxied API ------------------------------------------------------

    def get_capabilities(self) -> Capabilities:
        return self._run(self._async.get_capabilities())

    def get_observation(
        self, include_screenshot: bool = True, include_ui_tree: bool = False
    ) -> Observation:
        return self._run(self._async.get_observation(include_screenshot, include_ui_tree))

    def subscribe_observations(
        self, interval_ms: int = 500, include_screenshot: bool = False
    ) -> Iterator[Observation]:
        """Iterate over observations until the caller stops consuming."""
        q: "asyncio.Queue[Observation]" = asyncio.Queue()
        stop = asyncio.Event()

        async def pump() -> None:
            async for obs in self._async.subscribe_observations(
                interval_ms=interval_ms, include_screenshot=include_screenshot
            ):
                await q.put(obs)
                if stop.is_set():
                    break

        task = asyncio.run_coroutine_threadsafe(pump(), self._loop)
        try:
            while True:
                future = asyncio.run_coroutine_threadsafe(q.get(), self._loop)
                yield future.result()
        finally:
            self._loop.call_soon_threadsafe(stop.set)
            task.cancel()

    def execute(self, action: Dict[str, Any], note: Optional[str] = None) -> ActionResult:
        return self._run(self._async.execute(action, note=note))

    def execute_batch(
        self, actions: List[Dict[str, Any]], stop_on_error: bool = True
    ) -> List[ActionResult]:
        return self._run(self._async.execute_batch(actions, stop_on_error=stop_on_error))

    def get_action_log(
        self, session_id: Optional[str] = None, limit: Optional[int] = None
    ) -> List[AuditEntry]:
        return self._run(self._async.get_action_log(session_id=session_id, limit=limit))

    def set_safety_policy(self, policy: SafetyPolicy) -> SafetyPolicy:
        return self._run(self._async.set_safety_policy(policy))

    def emergency_stop(self) -> None:
        self._run(self._async.emergency_stop())

    # -- ergonomic helpers ------------------------------------------------

    def click(self, x: int, y: int, button: str = "left") -> ActionResult:
        return self._run(self._async.click(x, y, button))

    def double_click(self, x: int, y: int) -> ActionResult:
        return self._run(self._async.double_click(x, y))

    def right_click(self, x: int, y: int) -> ActionResult:
        return self._run(self._async.right_click(x, y))

    def drag(self, from_xy: tuple, to_xy: tuple, button: str = "left") -> ActionResult:
        return self._run(self._async.drag(from_xy, to_xy, button))

    def scroll(self, x: int, y: int, delta_x: int = 0, delta_y: int = 0) -> ActionResult:
        return self._run(self._async.scroll(x, y, delta_x, delta_y))

    def type_text(self, text: str, delay_ms: Optional[int] = None) -> ActionResult:
        return self._run(self._async.type_text(text, delay_ms))

    def hotkey(self, keys: List[str]) -> ActionResult:
        return self._run(self._async.hotkey(keys))

    def key_press(self, key: str) -> ActionResult:
        return self._run(self._async.key_press(key))

    def clipboard_get(self) -> str:
        return self._run(self._async.clipboard_get())

    def clipboard_set(self, text: str) -> ActionResult:
        return self._run(self._async.clipboard_set(text))

    def click_element(
        self,
        *,
        text: Optional[str] = None,
        role: Optional[str] = None,
        app: Optional[str] = None,
        bounds: Optional[Dict[str, int]] = None,
        index: Optional[int] = None,
    ) -> ActionResult:
        return self._run(
            self._async.click_element(text=text, role=role, app=app, bounds=bounds, index=index)
        )

    def open_app(self, name: str) -> ActionResult:
        return self._run(self._async.open_app(name))


__all__ = ["NerveClient", "NerveError"]

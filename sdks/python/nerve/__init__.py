"""Python SDK for Nerve.

The SDK is a thin wrapper around the WebSocket protocol exposed by the
``nerve`` daemon. Two clients are provided:

* :class:`NerveClient` — synchronous, blocking. Good for scripts and the demo.
* :class:`AsyncNerveClient` — asyncio-native. Good for agents that already run
  an event loop.

Both share the same wire format and the same authoritative protocol types
exposed by ``nerve-protocol``. We re-export the most useful shapes in
:mod:`nerve.types` so callers don't need to memorise the JSON schema.
"""

from .client import NerveClient
from .async_client import AsyncNerveClient
from .types import (
    ActionEnvelope,
    ActionResult,
    AuditEntry,
    Capabilities,
    Observation,
    SafetyPolicy,
    ElementTarget,
)

__all__ = [
    "NerveClient",
    "AsyncNerveClient",
    "ActionEnvelope",
    "ActionResult",
    "AuditEntry",
    "Capabilities",
    "Observation",
    "SafetyPolicy",
    "ElementTarget",
]

__version__ = "0.1.0"

"""Lightweight typed wrappers for the Nerve wire protocol.

We deliberately avoid pulling in pydantic so the SDK stays dependency-free
except for ``websockets``. Each wrapper is a plain dataclass with a
``from_dict`` / ``to_dict`` pair that round-trips through ``json``.
"""

from __future__ import annotations

from dataclasses import dataclass, field, asdict
from typing import Any, Dict, List, Optional


def _drop_none(d: Dict[str, Any]) -> Dict[str, Any]:
    return {k: v for k, v in d.items() if v is not None}


@dataclass
class ElementTarget:
    text: Optional[str] = None
    role: Optional[str] = None
    app: Optional[str] = None
    bounds: Optional[Dict[str, int]] = None
    index: Optional[int] = None

    def to_dict(self) -> Dict[str, Any]:
        return _drop_none(asdict(self))


@dataclass
class ActionEnvelope:
    id: str
    action: Dict[str, Any]
    note: Optional[str] = None

    def to_dict(self) -> Dict[str, Any]:
        return _drop_none({"id": self.id, "action": self.action, "note": self.note})


@dataclass
class ActionResult:
    id: str
    ok: bool
    timestamp: str
    method: str
    cursor: Optional[Dict[str, int]] = None
    active_window: Optional[str] = None
    error: Optional[str] = None
    data: Optional[Dict[str, Any]] = None
    screenshot_before: Optional[str] = None
    screenshot_after: Optional[str] = None
    compiled: Optional[Dict[str, Any]] = None

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "ActionResult":
        return cls(
            id=d["id"],
            ok=d["ok"],
            timestamp=d["timestamp"],
            method=d["method"],
            cursor=d.get("cursor"),
            active_window=d.get("active_window"),
            error=d.get("error"),
            data=d.get("data"),
            screenshot_before=d.get("screenshot_before"),
            screenshot_after=d.get("screenshot_after"),
            compiled=d.get("compiled"),
        )


@dataclass
class Observation:
    raw: Dict[str, Any]

    @property
    def session_id(self) -> str:
        return self.raw["session_id"]

    @property
    def timestamp(self) -> str:
        return self.raw["timestamp"]

    @property
    def platform(self) -> str:
        return self.raw["platform"]

    @property
    def cursor(self) -> Dict[str, int]:
        return self.raw.get("cursor", {})

    @property
    def active_window(self) -> Optional[Dict[str, Any]]:
        return self.raw.get("active_window")

    @property
    def screen(self) -> Dict[str, Any]:
        return self.raw.get("screen", {})

    @property
    def screenshot_base64(self) -> Optional[str]:
        return self.screen.get("screenshot_base64")

    @property
    def ui_tree(self) -> List[Dict[str, Any]]:
        return self.raw.get("ui_tree", []) or []

    @property
    def safety(self) -> Dict[str, bool]:
        return self.raw.get("safety_state", {})


@dataclass
class AuditEntry:
    raw: Dict[str, Any]

    @property
    def action_id(self) -> str:
        return self.raw["action_id"]

    @property
    def timestamp(self) -> str:
        return self.raw["timestamp"]

    @property
    def method(self) -> str:
        return self.raw["result"]["method"]

    @property
    def ok(self) -> bool:
        return bool(self.raw["result"]["ok"])

    @property
    def safety_decision(self) -> str:
        return self.raw.get("safety_decision", "unknown")


@dataclass
class Capabilities:
    raw: Dict[str, Any]

    @property
    def platform(self) -> str:
        return self.raw.get("platform", "unknown")

    @property
    def has_accessibility(self) -> bool:
        return bool(self.raw.get("accessibility_tree"))

    @property
    def has_screen_capture(self) -> bool:
        return bool(self.raw.get("screen_capture"))

    @property
    def wayland_limited(self) -> bool:
        return bool(self.raw.get("wayland_limited"))

    @property
    def missing_permissions(self) -> List[str]:
        return list(self.raw.get("missing_permissions") or [])


@dataclass
class SafetyPolicy:
    dry_run: bool = False
    require_confirmation: bool = False
    human_takeover: bool = False
    app_allowlist: List[str] = field(default_factory=list)
    app_blocklist: List[str] = field(default_factory=list)
    max_actions_per_minute: int = 600
    max_session_seconds: int = 0
    redact_patterns: List[str] = field(default_factory=list)
    block_password_fields: bool = True
    confirm_payment_fields: bool = True

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)

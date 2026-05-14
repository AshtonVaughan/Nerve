"""Strict typed views of the wire protocol.

Built with :class:`typing.TypedDict` so callers who run ``mypy --strict`` get
real type checking without taking on a pydantic dependency. The wire types
themselves stay loose dicts in :class:`Observation` / :class:`AuditEntry` so
older clients keep working when the protocol adds optional fields.
"""

from __future__ import annotations

from typing import Any, Dict, List, Literal, Optional, TypedDict


Platform = Literal["macos", "windows", "linux", "unknown"]
MouseButton = Literal["left", "right", "middle"]
ExecutionMethod = Literal[
    "accessibility_action",
    "native_ui_action",
    "browser_dom_adapter",
    "ocr_bounding_box",
    "coordinate_click",
    "keyboard",
    "clipboard",
    "wait",
    "capture",
    "no_op",
]
SafetyDecision = Literal[
    "allowed",
    "dry_run",
    "confirmed",
    "blocked",
    "rate_limited",
    "emergency_stopped",
]
ErrorCode = Literal[
    "internal",
    "bad_request",
    "unsupported",
    "no_session",
    "session_not_found",
    "auth_required",
    "auth_invalid",
    "version_mismatch",
    "safety_rejected",
    "rate_limited",
    "emergency_stopped",
    "element_not_found",
    "backend_failure",
    "permission_denied",
    "idempotent",
    "log_io_error",
    "replay_unavailable",
]


class BoundsDict(TypedDict):
    x: int
    y: int
    width: int
    height: int


class CursorDict(TypedDict):
    x: int
    y: int


class ScreenDict(TypedDict, total=False):
    width: int
    height: int
    scale_factor: float
    screenshot_base64: Optional[str]
    screenshot_format: str
    screenshot_hash: Optional[str]


class ActiveWindowDict(TypedDict, total=False):
    title: str
    app_name: str
    process_name: str
    pid: Optional[int]
    bounds: BoundsDict


class SafetyStateDict(TypedDict):
    agent_active: bool
    dry_run: bool
    human_takeover: bool
    emergency_stopped: bool
    confirmation_required: bool


class ObservationDict(TypedDict, total=False):
    session_id: str
    timestamp: str
    platform: Platform
    screen: ScreenDict
    cursor: CursorDict
    active_window: Optional[ActiveWindowDict]
    ui_tree: List[Dict[str, Any]]
    ocr: List[Dict[str, Any]]
    focused_element: Optional[Dict[str, Any]]
    last_action: Optional[str]
    dirty_tiles: List[BoundsDict]
    safety_state: SafetyStateDict


class CompiledPlanDict(TypedDict, total=False):
    method: ExecutionMethod
    primitive: Optional[Dict[str, Any]]
    attempted: List[ExecutionMethod]
    trace: List[str]


class ActionResultDict(TypedDict, total=False):
    id: str
    ok: bool
    timestamp: str
    method: ExecutionMethod
    cursor: Optional[CursorDict]
    active_window: Optional[str]
    error: Optional[str]
    data: Optional[Dict[str, Any]]
    screenshot_before: Optional[str]
    screenshot_after: Optional[str]
    compiled: Optional[CompiledPlanDict]


class CapabilitiesDict(TypedDict, total=False):
    platform: Platform
    screen_capture: bool
    input_control: bool
    accessibility_tree: bool
    clipboard: bool
    semantic_actions: bool
    ocr: bool
    wayland_limited: bool
    missing_permissions: List[str]
    backends: Dict[str, str]
    version: str


class AuditEntryDict(TypedDict, total=False):
    session_id: str
    action_id: str
    timestamp: str
    action: Dict[str, Any]
    result: ActionResultDict
    active_window_before: Optional[str]
    active_window_after: Optional[str]
    safety_decision: SafetyDecision
    note: Optional[str]


__all__ = [
    "ActiveWindowDict",
    "ActionResultDict",
    "AuditEntryDict",
    "BoundsDict",
    "CapabilitiesDict",
    "CompiledPlanDict",
    "CursorDict",
    "ErrorCode",
    "ExecutionMethod",
    "MouseButton",
    "ObservationDict",
    "Platform",
    "SafetyDecision",
    "SafetyStateDict",
    "ScreenDict",
]

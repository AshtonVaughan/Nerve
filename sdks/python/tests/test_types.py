"""Type wrappers should round-trip through dicts cleanly."""

from nerve.types import (
    ActionEnvelope,
    ActionResult,
    AuditEntry,
    ElementTarget,
    Observation,
    SafetyPolicy,
)


def test_element_target_drops_none():
    t = ElementTarget(text="Save")
    d = t.to_dict()
    assert d == {"text": "Save"}


def test_action_envelope_keeps_action():
    e = ActionEnvelope(id="a1", action={"type": "click", "x": 1, "y": 2})
    assert e.to_dict()["action"]["type"] == "click"


def test_action_result_from_dict_passthrough():
    r = ActionResult.from_dict(
        {
            "id": "a1",
            "ok": True,
            "timestamp": "2026-05-14T00:00:00Z",
            "method": "no_op",
        }
    )
    assert r.method == "no_op"
    assert r.error is None


def test_observation_proxies():
    o = Observation(raw={"session_id": "s1", "platform": "linux", "cursor": {"x": 1, "y": 2}})
    assert o.session_id == "s1"
    assert o.platform == "linux"
    assert o.cursor == {"x": 1, "y": 2}


def test_audit_entry_helpers():
    e = AuditEntry(
        raw={
            "action_id": "a1",
            "timestamp": "t",
            "result": {"method": "keyboard", "ok": True},
            "safety_decision": "allowed",
        }
    )
    assert e.method == "keyboard"
    assert e.ok is True
    assert e.safety_decision == "allowed"


def test_safety_policy_defaults():
    p = SafetyPolicy()
    d = p.to_dict()
    assert d["dry_run"] is False
    assert d["max_actions_per_minute"] == 600
    assert d["block_password_fields"] is True

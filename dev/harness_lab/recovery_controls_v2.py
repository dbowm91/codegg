"""ADR-0040-aligned, versioned recovery controls.

Version 1's historical receipts keep their original interpretation. This
module defines a separate v2 contract whose evidence is native typed ACP
status plus a harness-measured workspace revision; output text is never used
as a proxy for success, failure, or progress.
"""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
from typing import Any

SUITE_VERSION = 2
MANIFEST = Path(__file__).parent / "fixtures" / "agent_autonomy_v2" / "cases.jsonl"
OBSERVATIONS = {"repeated_failure_bounded", "changed_state_not_stalled",
                "unknown_gap", "hard_budget_stop"}
STOP_OWNERS = {"agent_completed", "agent_turn_budget", "cancelled", "unknown", "none"}
STATUSES = {"completed", "failed", "unknown"}


class RecoveryControlError(ValueError):
    """The v2 recovery control contract is malformed or incomparable."""


@dataclass(frozen=True)
class RecoverySuite:
    version: int
    digest: str
    cases: tuple[dict[str, Any], ...]


def load_suite(path: Path = MANIFEST) -> RecoverySuite:
    raw = path.read_bytes()
    try:
        rows = [json.loads(line) for line in raw.decode("utf-8").splitlines() if line.strip()]
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RecoveryControlError("v2 recovery manifest is not valid UTF-8 JSONL") from error
    if not rows or rows[0] != {"suite_version": SUITE_VERSION}:
        raise RecoveryControlError("v2 recovery manifest has an unsupported suite version")
    cases: list[dict[str, Any]] = []
    seen: set[str] = set()
    required = {"case_id", "expected_observation", "expected_stop_owner", "protected"}
    for row in rows[1:]:
        if not isinstance(row, dict) or set(row) != required:
            raise RecoveryControlError("v2 recovery case has missing or unknown fields")
        case_id = row["case_id"]
        if not isinstance(case_id, str) or not case_id or case_id in seen:
            raise RecoveryControlError("v2 recovery case id is empty or duplicated")
        if row["expected_observation"] not in OBSERVATIONS:
            raise RecoveryControlError(f"unsupported observation for {case_id}")
        if row["expected_stop_owner"] not in STOP_OWNERS:
            raise RecoveryControlError(f"unsupported stop owner for {case_id}")
        if not isinstance(row["protected"], bool) or not row["protected"]:
            raise RecoveryControlError(f"v2 control {case_id} must be protected")
        seen.add(case_id)
        cases.append(row)
    if not cases:
        raise RecoveryControlError("v2 recovery manifest declares no controls")
    return RecoverySuite(SUITE_VERSION, hashlib.sha256(raw).hexdigest(), tuple(cases))


def action_identity(call: dict[str, Any]) -> str:
    """Hash the full bounded action identity, excluding untrusted output text."""
    tool = call.get("tool")
    arguments_sha256 = call.get("arguments_sha256")
    status = call.get("status")
    state_revision = call.get("state_revision")
    if not isinstance(tool, str) or not tool:
        raise RecoveryControlError("tool observation has no tool identity")
    if not isinstance(arguments_sha256, str) or len(arguments_sha256) != 64:
        raise RecoveryControlError("tool observation has no canonical argument digest")
    if status not in STATUSES:
        raise RecoveryControlError("tool observation has an unknown terminal status")
    if state_revision is not None and (not isinstance(state_revision, str) or len(state_revision) != 64):
        raise RecoveryControlError("workspace revision must be a SHA-256 digest or null")
    value = {"tool": tool, "arguments_sha256": arguments_sha256,
             "status": status, "state_revision": state_revision}
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def evaluate_control(case: dict[str, Any], calls: list[dict[str, Any]], *,
                     suite_sha256: str, terminal: str, stop_owner: str, event_gap: bool = False,
                     recovery_rung: str | None = None) -> dict[str, Any]:
    """Evaluate actual ACP observations without interpreting raw output."""
    if case.get("expected_observation") not in OBSERVATIONS:
        raise RecoveryControlError("case is not a v2 recovery control")
    if not isinstance(suite_sha256, str) or len(suite_sha256) != 64:
        raise RecoveryControlError("v2 control receipt requires the exact suite digest")
    identities = [action_identity(call) for call in calls]
    identical_failed = 0
    longest = 0
    previous: str | None = None
    for call, identity in zip(calls, identities):
        if call["status"] == "failed" and identity == previous:
            identical_failed += 1
        elif call["status"] == "failed":
            identical_failed = 1
        else:
            identical_failed = 0
        longest = max(longest, identical_failed)
        previous = identity
    unknown = event_gap or any(call["status"] == "unknown" or call.get("state_revision") is None
                               for call in calls)
    expected = case["expected_observation"]
    if unknown:
        state = "unknown"
        held: bool | None = None
    elif expected == "repeated_failure_bounded":
        exercised = longest >= 2
        state = "exercised" if exercised else "not_exercised"
        held = bool(exercised and terminal not in {"stalled", "failed_stall"})
    elif expected == "changed_state_not_stalled":
        progressed = len({call.get("state_revision") for call in calls}) > 1
        state = "exercised" if progressed else "not_exercised"
        held = bool(progressed and terminal not in {"stalled", "failed_stall"})
    elif expected == "hard_budget_stop":
        state = "exercised" if stop_owner == "agent_turn_budget" else "not_exercised"
        held = stop_owner == "agent_turn_budget" and terminal not in {"stalled", "failed_stall"}
    else:
        state = "exercised" if event_gap or unknown else "not_exercised"
        held = None
    owner_matches = stop_owner == case["expected_stop_owner"]
    if held is not None:
        held = held and owner_matches
    return {
        "suite_version": SUITE_VERSION,
        "suite_sha256": suite_sha256,
        "case_id": case["case_id"],
        "control_state": state,
        "control_exercised": state == "exercised",
        "control_held": held,
        "identical_failed_call_count": longest,
        "call_identity_sha256": identities[-1] if identities else None,
        "progress_evidence_class": (
            "unknown" if unknown else
            "workspace_revision_changed" if len({call.get("state_revision") for call in calls}) > 1 else
            "identical_call_without_revision_change" if longest >= 2 else "none"),
        "recovery_rung": recovery_rung,
        "recovery_rung_evidence": "observed" if recovery_rung is not None else "unavailable_not_exposed",
        "terminal": terminal,
        "actual_stop_owner": stop_owner,
        "expected_stop_owner": case["expected_stop_owner"],
        "stop_owner_matches": owner_matches,
        "event_gap": event_gap,
    }


def compare_pair(baseline: dict[str, Any], candidate: dict[str, Any]) -> dict[str, Any]:
    """Reject mixed suites and unresolved controls rather than score them."""
    keys = ("suite_version", "suite_sha256")
    if any(baseline.get(key) != candidate.get(key) for key in keys):
        raise RecoveryControlError("v2 score pair mixes suite versions or manifest digests")
    if baseline.get("suite_version") != SUITE_VERSION:
        raise RecoveryControlError("v1 and v2 recovery receipts cannot share a score pair")
    if baseline.get("control_state") != "exercised" or candidate.get("control_state") != "exercised":
        raise RecoveryControlError("not_exercised or unknown controls cannot contribute to acceptance")
    return {"suite_version": SUITE_VERSION,
            "suite_sha256": baseline["suite_sha256"],
            "baseline_held": baseline.get("control_held"),
            "candidate_held": candidate.get("control_held"),
            "regression": baseline.get("control_held") is True and candidate.get("control_held") is not True}

"""Secret-safe report contract for native H003 Rust/job fixture attempts."""
from __future__ import annotations

from typing import Any

SCHEMA_VERSION = 2


def build_receipt(case: dict[str, Any], run: dict[str, Any], *, attempt_id: str,
                  lane_cache: str = "cold_private") -> dict[str, Any]:
    if case.get("schema_version") != SCHEMA_VERSION:
        raise ValueError("long-horizon report requires scenario manifest v2")
    if run.get("event_observation_state") != "measured_no_gap" or run.get("event_gap_count") != 0:
        raise ValueError("event gap makes the long-horizon attempt unmeasured")
    verification = run.get("verification")
    if not isinstance(verification, dict) or verification.get("returncode") != 0:
        raise ValueError("independent hidden verifier did not pass")
    calls = run.get("tool_calls")
    fixture_calls = run.get("fixture_calls")
    if not isinstance(calls, list) or not isinstance(fixture_calls, list):
        raise ValueError("native tool and provider-call evidence is incomplete")
    submitted = [item.get("job_id") for item in fixture_calls
                 if item.get("name") == "test" and item.get("job_id")]
    waited = [item.get("job_id") for item in fixture_calls if item.get("name") == "job"]
    if not submitted or submitted != waited:
        raise ValueError("background job waits do not match submitted CodeGG job ids")
    digest_only_calls = [{key: item.get(key) for key in (
        "tool", "arguments_sha256", "status", "state_revision")} for item in calls]
    return {
        "schema_version": SCHEMA_VERSION,
        "attempt_id": attempt_id,
        "case_id": case["case_id"],
        "case_manifest_sha256": case["case_manifest_sha256"],
        "fixture_source_sha256": case["fixture_source_sha256"],
        "hidden_verifier_sha256": case["hidden_verifier_sha256"],
        "public_contract_sha256": case["public_contract_sha256"],
        "source_binary_sha256": run.get("source_sha256"),
        "session_id": run.get("session_id"),
        "terminal": run.get("stop_reason"),
        "stop_owner": run.get("stop_owner"),
        "tool_calls": digest_only_calls,
        "tool_call_count": len(calls),
        "tool_failure_count": run.get("tool_failures"),
        "provider_request_count": run.get("provider_calls"),
        "job_ids": submitted,
        "job_wait_count": len(waited),
        "independent_verifier": "passed",
        "event_observation": "measured_no_gap",
        "context_transition": {"state": "unavailable_not_exposed", "count": None},
        "provider_usage": run.get("codegg_usage"),
        "provider_cost_usd": None,
        "provider_cost_unavailable_reason": "local deterministic fixture has no provider billing",
        "wall_time_ms": run.get("wall_time_ms"),
        "cargo_target_cache": lane_cache,
        "cleanup": "verified_owned_daemon_stopped",
    }

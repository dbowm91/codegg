"""Idempotent accounting for explicitly authorized live campaign starts.

The campaign ledger is updated when the first provider request is observed.
Wrappers may reconcile it repeatedly while a run is active, so the attempt
number is the idempotency key and the historical offset is always preserved.
"""

from __future__ import annotations

import copy
import re
from collections.abc import Mapping, MutableMapping
from typing import Any

from .diagnostics import SAFE_JOB_STATUS_ERROR_CLASSES


class CampaignLedgerError(ValueError):
    """The authorization ledger cannot safely admit or reconcile a run."""


_IDENTITY_FIELDS = (
    "attempt",
    "start_number",
    "authorization_grant_id",
    "model",
    "route",
    "manifest_sha256",
    "binary_sha256",
)
_SAFE_RUN_FIELDS = frozenset({
    *_IDENTITY_FIELDS,
    "active_work_span_ms",
    "activity_stall_seconds",
    "agent_terminal",
    "billed_usd",
    "billed_usd_treatment",
    "binary_architecture",
    "continuation_confirmed",
    "disk_measurement_source",
    "disk_reserve_dipped_during_run",
    "dropped_events",
    "duration_ms",
    "duration_qualified",
    "event_gap_count",
    "event_measurement_complete",
    "exact_job_running_and_terminal",
    "external_hard_stop",
    "external_watcher_minimum_bytes",
    "finished_at_utc",
    "free_disk_bytes_at_completion",
    "free_disk_bytes_before",
    "free_disk_bytes_minimum",
    "horizon_outcome",
    "horizon_reason",
    "last_free_disk_bytes_observed",
    "live_outcome",
    "minimum_free_bytes_source",
    "native_tool_call_limit",
    "preflight_reserve_bytes",
    "productive_work_during_job",
    "prompt_work_limit_seconds",
    "provider_allowance_usd",
    "provider_attempts_failed",
    "provider_attempts_started",
    "provider_attempts_succeeded",
    "provider_request_limit",
    "provider_requests_observed",
    "repair_cycle_count",
    "repair_group_qualification",
    "required_review_observed",
    "required_success_jobs",
    "reviewer_actual_spawn_elapsed_ms",
    "reviewer_completed_get_result_observed",
    "reviewer_elapsed_ms",
    "reviewer_failure_class",
    "reviewer_final_status",
    "reviewer_get_count",
    "reviewer_required_review_observed_in_public_continuation",
    "reviewer_spawn_count",
    "reviewer_start_threshold_active_ms",
    "reviewer_task_action_counts",
    "runner_exit_code",
    "runs_started_counted_rows",
    "safe_evidence_path",
    "safe_evidence_sha256",
    "safe_receipt_sha256",
    "same_session_followup_limit",
    "stop_reason",
    "supplemental_grant_id",
    "task_outcome",
    "test_job_submissions",
    "test_job_terminals",
    "tool_starts",
    "tool_terminals",
    "unpaired_tool_call_count",
    "unpaired_tool_class",
    "unpaired_tool_started_elapsed_ms",
    "unrepresented_counted_starts",
    "unresolved_provider_attempt_count",
    "verifier_errors",
    "verifier_failure_domain_counts",
    "verifier_failures",
    "verifier_passed_count",
    "verifier_state",
    "verifier_tests_run",
    "wallclock_limit_seconds",
    "sandbox_helper_sha256",
    "manifest_path",
    "manifest_version",
    "preflight_evidence_path",
    "authorization_preflight_evidence_path",
    "started_at_utc",
    "counted_at_utc",
    "binary_architecture",
    "sandbox_helper_architecture",
    "credential_file_mode",
    "credential_value_retained",
    "model_argument_required_in_launcher",
    "provider_request_limit",
    "native_tool_call_limit",
    "prompt_work_limit_seconds",
    "wallclock_limit_seconds",
    "same_session_followup_limit",
    "disk_floor_bytes",
    "preflight_reserve_bytes",
})
_STATUS_RANK = {
    "in_progress": 0,
    "completed_unqualified_pending_reconciliation": 1,
    "completed_unqualified": 2,
    "qualified": 3,
}

_PUBLIC_IDENTIFIER = re.compile(r"^[a-z][a-z0-9_]{0,63}$")
_PUBLIC_PROGRESS_STATUSES = frozenset({
    "complete", "completed", "pending", "failed", "in_progress", "not_started", "other",
})
_TASK_COMPLETION_STATES = frozenset({
    "verified_complete", "agent_incomplete", "incomplete", "verification_failed",
    "verifier_unavailable", "not_demonstrated",
})
_TASK_COMPLETION_REASONS = frozenset({
    "agent_terminal_incomplete", "continuation_missing", "continuation_not_confirmed",
    "required_stages_incomplete", "required_gates_incomplete",
    "required_success_jobs_incomplete", "test_jobs_nonterminal",
    "required_review_unobserved", "required_artifacts_missing",
    "independent_verifier_failures",
})
_VERIFIER_OUTCOMES = frozenset({
    "passed", "setup_error", "empty_suite", "assertion_failures", "test_errors",
    "mixed_test_failures", "timeout", "runner_error", "unclassified_failure", "not_run",
})
_VERIFIER_FAILURE_DOMAINS = frozenset({"continuation_boundary", "transcript_job_lifecycle"})
_PUBLIC_VERIFIER_CHECKS = {
    "continuation_boundary": frozenset({
        "document_present", "document_valid", "phase_order_matches",
        "contract_phase_order_matches", "stage_order_matches", "next_phase_none",
        "next_contract_phase_none", "next_stage_none", "next_gate_none",
        "incomplete_stages_empty", "incomplete_gates_empty", "missing_phases_empty",
        "missing_contract_phases_empty", "required_success_jobs_complete",
        "incomplete_required_success_jobs_empty", "public_artifacts_present",
        "progress_sections_ordered", "pointer_fields_follow_progress_sections",
        "pointer_fields_in_expected_order",
    }),
    "transcript_job_lifecycle": frozenset({
        "repair_pairs_complete", "repair_count_matches",
        "minimum_repair_cycles_observed", "test_jobs_terminal",
        "required_review_observed", "release_overlap_confirmed",
        "exact_job_running_and_terminal", "productive_work_during_job",
        "event_measurement_complete", "job_tool_calls_bound_to_id",
        "job_request_result_ids_match", "serialized_transcript_projection_matches",
    }),
}
_PUBLIC_RECONCILIATION_CLASSES = frozenset({
    "verifier_public_projection_disagreement", "public_evidence_incomplete",
    "public_evidence_unavailable", "unmapped_failure_domain",
})
_PUBLIC_REPAIR_GROUPS = frozenset({"catalog_format", "catalog_store"})
_PUBLIC_REPAIR_CHECKS = frozenset({
    "failed_job_submitted_once", "failed_job_status_called_immediately",
    "failed_terminal_observed", "repair_job_submitted_once",
    "repair_job_status_called_immediately", "repair_terminal_observed",
    "repair_followed_failure", "productive_change_between_failure_and_rerun",
    "documented_states_match_observations", "job_ids_are_distinct",
    "qualified", "repair_cycle_correlated",
})
_PUBLIC_REPAIR_COUNTS = frozenset({
    "required_pair_count", "qualified_pair_count", "observed_repair_cycle_count",
    "documented_repair_cycle_count",
})
_PUBLIC_JOB_CORRELATION_STATES = frozenset({
    "matched", "mismatched", "unobserved", "not_applicable",
})
_PUBLIC_EVENT_OBSERVATION_STATES = frozenset({
    "observed_complete", "unmeasured_gap", "not_instrumented", "unobserved",
})
_PUBLIC_JOB_ACTIONS = frozenset({
    "submit", "status", "wait", "output", "cancel", "spawn", "unknown",
})
_PUBLIC_JOB_ARGUMENT_STATES = frozenset({
    "invalid_json", "not_object", "missing", "non_string", "empty_string",
    "string", "unobserved",
})
_PUBLIC_JOB_CALL_STATES = frozenset({"completed", "failed", "cancelled", "unknown"})
_MAX_PUBLIC_PROGRESS_ITEMS = 32
_MAX_PUBLIC_DIAGNOSTIC_COUNT = 100_000


def preflight_campaign_run_start(
    ledger: Mapping[str, Any],
    run: Mapping[str, Any],
    *,
    expected_model: str,
    expected_route: str,
    expected_manifest_sha256: str,
    expected_binary_sha256: str,
    expected_provider_request_limit: int,
    expected_wallclock_limit_seconds: int,
) -> dict[str, int | str]:
    """Validate campaign identity and headroom without mutating the ledger.

    This is the admission preview for a live wrapper: call it before launching
    CodeGG or allowing a provider request. It deliberately goes through the
    same reconciler used after the first observed request so start-number,
    grant, route, model, manifest, binary, and request-cap checks cannot drift.
    """
    expected = {
        "model": expected_model,
        "route": expected_route,
        "manifest_sha256": expected_manifest_sha256,
        "binary_sha256": expected_binary_sha256,
        "provider_request_limit": expected_provider_request_limit,
        "wallclock_limit_seconds": expected_wallclock_limit_seconds,
    }
    for key, value in expected.items():
        if run.get(key) != value:
            raise CampaignLedgerError(f"campaign preflight {key} does not match the launch")

    for key, value in (("expected_provider_request_limit", expected_provider_request_limit),
                       ("expected_wallclock_limit_seconds", expected_wallclock_limit_seconds)):
        if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
            raise CampaignLedgerError(f"{key} must be a positive integer")

    fixture_manifest_sha256 = run.get("fixture_manifest_sha256")
    if fixture_manifest_sha256 is not None \
            and fixture_manifest_sha256 != expected_manifest_sha256:
        raise CampaignLedgerError(
            "campaign preflight fixture manifest digest conflicts with canonical digest")

    preview = copy.deepcopy(dict(ledger))
    return reconcile_campaign_run_start(
        preview, run, provider_requests=1, status="in_progress")


def _safe_public_statuses(value: Any) -> dict[str, str]:
    if not isinstance(value, Mapping) or len(value) > _MAX_PUBLIC_PROGRESS_ITEMS:
        return {}
    return {
        key: status for key, status in value.items()
        if isinstance(key, str) and _PUBLIC_IDENTIFIER.fullmatch(key)
        and isinstance(status, str) and status in _PUBLIC_PROGRESS_STATUSES
    }


def _safe_public_domain_map(value: Any, allowed_values: frozenset[str]) -> dict[str, Any]:
    if not isinstance(value, Mapping):
        return {}
    result: dict[str, Any] = {}
    for key in _VERIFIER_FAILURE_DOMAINS:
        item = value.get(key)
        if isinstance(item, int) and not isinstance(item, bool) and 0 <= item <= 100:
            result[key] = item
        elif isinstance(item, str) and item in allowed_values:
            result[key] = item
    return result


def _safe_public_verifier_checks(value: Any) -> dict[str, dict[str, bool | None]]:
    """Keep only fixed public-check names and Boolean/null values per domain."""
    if not isinstance(value, Mapping) or len(value) > _MAX_PUBLIC_PROGRESS_ITEMS:
        return {}
    result: dict[str, dict[str, bool | None]] = {}
    for domain, allowed_names in _PUBLIC_VERIFIER_CHECKS.items():
        checks = value.get(domain)
        if not isinstance(checks, Mapping) or len(checks) > _MAX_PUBLIC_PROGRESS_ITEMS:
            continue
        projected = {
            name: checks[name] for name in allowed_names
            if name in checks and (isinstance(checks[name], bool) or checks[name] is None)
        }
        if projected:
            result[domain] = projected
    return result


def _safe_count(value: Any, *, maximum: int = _MAX_PUBLIC_DIAGNOSTIC_COUNT) -> int | None:
    if (isinstance(value, int) and not isinstance(value, bool)
            and 0 <= value <= maximum):
        return value
    return None


def _safe_named_counts(
    value: Any, names: frozenset[str], *, maximum: int = _MAX_PUBLIC_DIAGNOSTIC_COUNT
) -> dict[str, int]:
    if not isinstance(value, Mapping) or len(value) > _MAX_PUBLIC_PROGRESS_ITEMS:
        return {}
    return {
        name: count for name in names
        if (count := _safe_count(value.get(name), maximum=maximum)) is not None and count > 0
    }


def _safe_unbound_call_state_counts(value: Any) -> dict[str, int]:
    if not isinstance(value, Mapping) or len(value) > _MAX_PUBLIC_PROGRESS_ITEMS:
        return {}
    result: dict[str, int] = {}
    for key, raw_count in value.items():
        if not isinstance(key, str):
            continue
        parts = key.split(":")
        if (len(parts) != 3 or parts[0] not in _PUBLIC_JOB_ACTIONS
                or parts[1] not in _PUBLIC_JOB_ARGUMENT_STATES
                or parts[2] not in _PUBLIC_JOB_CALL_STATES):
            continue
        count = _safe_count(raw_count)
        if count is not None and count > 0:
            result[key] = count
    return result


def _safe_event_measurement_diagnostic(value: Any) -> dict[str, Any] | None:
    if not isinstance(value, Mapping) or value.get("schema") != "h007_event_measurement_v1":
        return None
    state = value.get("event_observation_state")
    if not isinstance(state, str) or state not in _PUBLIC_EVENT_OBSERVATION_STATES:
        return None
    result: dict[str, Any] = {
        "schema": "h007_event_measurement_v1",
        "event_observation_state": state,
    }
    for name in (
        "event_gap_count", "event_gap_dropped_count", "tool_started_count",
        "tool_terminal_count", "projected_tool_call_count", "tool_unpaired_start_count",
        "unknown_terminal_tool_call_count",
    ):
        result[name] = _safe_count(value.get(name), maximum=1_600)
    result["measurement_complete"] = (
        state == "observed_complete"
        and result["event_gap_count"] == 0
        and result["event_gap_dropped_count"] == 0
        and result["tool_started_count"] is not None
        and result["tool_terminal_count"] is not None
        and result["projected_tool_call_count"] is not None
        and result["tool_started_count"] == result["projected_tool_call_count"]
        and result["tool_terminal_count"] == result["projected_tool_call_count"]
        and result["tool_unpaired_start_count"] == 0
        and result["unknown_terminal_tool_call_count"] == 0
    )
    return result


def _safe_job_id_correlation(value: Any) -> dict[str, Any]:
    if not isinstance(value, Mapping):
        return {"state": "unavailable"}
    state_counts = _safe_named_counts(value.get("state_counts"), _PUBLIC_JOB_CORRELATION_STATES)
    observed = value.get("instrumentation_observed") is True
    applicable_count = _safe_count(value.get("applicable_call_count"))
    return {
        "state": "observed" if observed else "unavailable",
        "instrumentation_observed": observed,
        "applicable_call_count": applicable_count,
        "state_counts": state_counts,
    }


def _safe_line_layout(value: Any) -> dict[str, Any]:
    if not isinstance(value, Mapping) or value.get("raw_note_retained") is not False:
        return {"state": "unavailable"}
    state = value.get("state")
    if not isinstance(state, str) or state not in {
        "observed", "unavailable", "limit_exceeded",
    }:
        state = "unavailable"
    result: dict[str, Any] = {"state": state}
    for field in ("progress_section_count", "pointer_field_count", "line_count"):
        result[field] = _safe_count(value.get(field), maximum=100_000)
    for field in (
        "progress_sections_ordered", "pointer_fields_follow_progress_sections",
        "pointer_fields_in_expected_order", "pointer_block_is_terminal",
    ):
        item = value.get(field)
        result[field] = item if isinstance(item, bool) else None
    return result


def _safe_repair_job_diagnostic(value: Any) -> dict[str, Any]:
    if (not isinstance(value, Mapping)
            or value.get("raw_job_ids_retained") is not False
            or value.get("raw_job_arguments_retained") is not False):
        return {"state": "unavailable"}
    groups = value.get("groups")
    projected_groups: dict[str, dict[str, bool]] = {}
    if isinstance(groups, Mapping) and len(groups) <= _MAX_PUBLIC_PROGRESS_ITEMS:
        for group in _PUBLIC_REPAIR_GROUPS:
            checks = groups.get(group)
            if not isinstance(checks, Mapping) or len(checks) > _MAX_PUBLIC_PROGRESS_ITEMS:
                continue
            projected = {
                name: checks[name] for name in _PUBLIC_REPAIR_CHECKS
                if name in checks and isinstance(checks[name], bool)
            }
            if projected:
                projected_groups[group] = projected
    aggregates: dict[str, Any] = {
        field: _safe_count(value.get(field), maximum=100)
        for field in _PUBLIC_REPAIR_COUNTS
    }
    for field in ("repair_cycle_count_matches", "job_ids_unique_across_groups", "complete"):
        item = value.get(field)
        aggregates[field] = item if isinstance(item, bool) else None
    return {
        "state": "observed" if projected_groups or any(
            count is not None for count in aggregates.values()
            if not isinstance(count, bool)
        ) else "unavailable",
        **aggregates,
        "groups": projected_groups,
    }


def _safe_timeout_diagnostic(value: Any) -> dict[str, Any]:
    if (not isinstance(value, Mapping)
            or value.get("schema") != "h007_prompt_timeout_diagnostic_v1"
            or value.get("exception_text_retained") is not False
            or value.get("provider_text_retained") is not False):
        return {"state": "unavailable"}
    state = value.get("state")
    if not isinstance(state, str) or state not in {"observed", "not_observed"}:
        state = "unavailable"
    result: dict[str, Any] = {"state": state}
    for field in ("prompt_request_count", "autonomous_followup_count"):
        result[field] = _safe_count(value.get(field))
    for field in ("last_request_elapsed_ms",):
        result[field] = _safe_count(value.get(field), maximum=3_600_000)
    for field in (
        "last_request_timeout_seconds", "remaining_wallclock_seconds_at_prompt_start",
        "remaining_wallclock_seconds_at_timeout", "verifier_reserve_seconds",
    ):
        number = value.get(field)
        result[field] = (
            round(number, 3) if isinstance(number, (int, float))
            and not isinstance(number, bool) and 0 <= number <= 3600 else None
        )
    return result


def _safe_h007_completion_projection(case_receipt: Mapping[str, Any] | None) -> dict[str, Any]:
    """Project one H007 case row without carrying its raw source material."""
    unavailable = {"state": "unavailable"}
    if not isinstance(case_receipt, Mapping):
        return {"schema": "campaign_h007_completion_v1", **unavailable}

    completion = case_receipt.get("task_completion_diagnostic")
    if (not isinstance(completion, Mapping)
            or completion.get("schema") != "h007_task_completion_diagnostic_v2"
            or completion.get("raw_note_retained") is not False
            or completion.get("raw_job_ids_retained") is not False):
        task_projection: dict[str, Any] = unavailable.copy()
    else:
        raw_reasons = completion.get("reasons")
        reasons = ([reason for reason in raw_reasons
                    if isinstance(reason, str) and reason in _TASK_COMPLETION_REASONS]
                   if isinstance(raw_reasons, list) and len(raw_reasons) <= 16 else [])
        state = completion.get("state")
        verifier_outcome = completion.get("verifier_outcome")
        task_projection = {
            "state": state if isinstance(state, str) and state in _TASK_COMPLETION_STATES
            else "unavailable",
            "reasons": reasons,
            "verifier_outcome": (
                verifier_outcome if isinstance(verifier_outcome, str)
                and verifier_outcome in _VERIFIER_OUTCOMES else "unavailable"
            ),
        }

    continuation = case_receipt.get("continuation_diagnostic")
    if (not isinstance(continuation, Mapping)
            or continuation.get("schema") != "h007_continuation_diagnostic_v1"
            or continuation.get("raw_note_retained") is not False
            or continuation.get("raw_job_ids_retained") is not False):
        continuation_projection: dict[str, Any] = unavailable.copy()
    else:
        stages = _safe_public_statuses(continuation.get("stages"))
        gates = _safe_public_statuses(continuation.get("gates"))
        incomplete_stages = [name for name, status in stages.items()
                             if status not in {"complete", "completed"}]
        incomplete_gates = [name for name, status in gates.items()
                            if status not in {"complete", "completed"}]
        continuation_projection = {
            "state": "observed",
            "present": continuation.get("present")
            if isinstance(continuation.get("present"), bool) else None,
            "document_valid": continuation.get("document_valid")
            if isinstance(continuation.get("document_valid"), bool) else None,
            "stage_statuses": stages,
            "stage_complete_count": sum(
                status in {"complete", "completed"} for status in stages.values()),
            "stage_total_count": len(stages),
            "incomplete_stages": incomplete_stages,
            "first_missing_stage": incomplete_stages[0] if incomplete_stages else None,
            "gate_statuses": gates,
            "gate_complete_count": sum(
                status in {"complete", "completed"} for status in gates.values()),
            "gate_total_count": len(gates),
            "incomplete_gates": incomplete_gates,
            "first_missing_gate": incomplete_gates[0] if incomplete_gates else None,
            "required_review_observed": (
                continuation.get("required_review_observed")
                if isinstance(continuation.get("required_review_observed"), bool) else None
            ),
            "release_overlap_confirmed": (
                continuation.get("release_overlap_confirmed")
                if isinstance(continuation.get("release_overlap_confirmed"), bool) else None
            ),
            "all_public_artifacts_present": (
                continuation.get("all_public_artifacts_present")
                if isinstance(continuation.get("all_public_artifacts_present"), bool) else None
            ),
            "line_layout": _safe_line_layout(continuation.get("line_layout")),
            "repair_job_diagnostic": _safe_repair_job_diagnostic(
                continuation.get("repair_job_diagnostic")
            ),
        }

    reconciliation = completion.get("verifier_public_reconciliation") \
        if isinstance(completion, Mapping) else None
    if (not isinstance(reconciliation, Mapping)
            or reconciliation.get("schema") != "h007_verifier_public_reconciliation_v1"
            or reconciliation.get("assertion_text_retained") is not False
            or reconciliation.get("raw_output_retained") is not False
            or reconciliation.get("raw_job_ids_retained") is not False
            or reconciliation.get("transcript_retained") is not False):
        verifier_projection: dict[str, Any] = unavailable.copy()
    else:
        verifier_projection = {
            "state": "observed",
            "failure_domain_counts": _safe_public_domain_map(
                reconciliation.get("failure_domain_counts"), frozenset()),
            "public_domain_states": _safe_public_domain_map(
                reconciliation.get("public_domain_states"),
                frozenset({"complete", "incomplete", "unavailable"})),
            "classification": _safe_public_domain_map(
                reconciliation.get("classification"), _PUBLIC_RECONCILIATION_CLASSES),
            "public_checks": _safe_public_verifier_checks(
                reconciliation.get("public_checks")),
            "unbound_job_tool_call_count": _safe_count(
                reconciliation.get("unbound_job_tool_call_count")),
            "unbound_call_state_counts": _safe_unbound_call_state_counts(
                reconciliation.get("unbound_call_state_counts")),
            "job_id_request_result_correlation": _safe_job_id_correlation(
                reconciliation.get("job_id_request_result_correlation")),
            "failed_job_status_error_class_counts": _safe_named_counts(
                reconciliation.get("failed_job_status_error_class_counts"),
                SAFE_JOB_STATUS_ERROR_CLASSES, maximum=1_600),
        }
        event_measurement = _safe_event_measurement_diagnostic(
            reconciliation.get("event_measurement_diagnostic")
        )
        if event_measurement is not None:
            verifier_projection["event_measurement_diagnostic"] = event_measurement

    timeout_projection = _safe_timeout_diagnostic(
        case_receipt.get("turn_timeout_diagnostic")
    )
    observed = any(item.get("state") != "unavailable"
                   for item in (task_projection, continuation_projection, verifier_projection))
    return {
        "schema": "campaign_h007_completion_v1",
        "state": "observed" if observed else "unavailable",
        "task_completion": task_projection,
        "continuation": continuation_projection,
        "verifier_public_reconciliation": verifier_projection,
        "timeout": timeout_projection,
        "raw_note_retained": False,
        "raw_job_ids_retained": False,
        "assertion_text_retained": False,
        "raw_output_retained": False,
        "transcript_retained": False,
    }


def safe_campaign_run_fields(
    run: Mapping[str, Any], *,
    canonical_identity: Mapping[str, Any] | None = None,
    case_receipt: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    """Project safe run fields and one case's diagnostics without identity drift."""
    safe = {
        key: run[key] for key in _SAFE_RUN_FIELDS
        if key in run and (canonical_identity is None or key not in _IDENTITY_FIELDS)
    }
    if canonical_identity is not None:
        identity = {key: canonical_identity.get(key) for key in _IDENTITY_FIELDS}
        if any(not isinstance(value, (str, int)) or isinstance(value, bool)
               for value in identity.values()):
            raise CampaignLedgerError("canonical campaign preflight identity is incomplete")
        if (not isinstance(identity["attempt"], int)
                or not isinstance(identity["start_number"], int)
                or identity["attempt"] <= 0 or identity["start_number"] <= 0
                or not all(identity[key] for key in (
                    "authorization_grant_id", "model", "route", "manifest_sha256",
                    "binary_sha256"))):
            raise CampaignLedgerError("canonical campaign preflight identity is invalid")
        safe.update(identity)
    if case_receipt is not None:
        safe["h007_completion_diagnostic"] = _safe_h007_completion_projection(case_receipt)
    return safe


def reconcile_campaign_run_start(
    ledger: MutableMapping[str, Any],
    run: Mapping[str, Any],
    *,
    provider_requests: int,
    status: str,
) -> dict[str, int | str]:
    """Idempotently count one provider-started run without overwriting peers.

    Only explicitly allowlisted identity and preflight fields are copied from
    ``run``. The function refuses a wrong start number, conflicting retry,
    inactive grant, request overrun, or exhausted authorization before it
    mutates the ledger.
    """
    if not isinstance(provider_requests, int) or isinstance(provider_requests, bool):
        raise CampaignLedgerError("provider request count must be an integer")
    if provider_requests <= 0:
        raise CampaignLedgerError("a counted start requires a provider request")
    if status not in _STATUS_RANK:
        raise CampaignLedgerError("unknown campaign run status")

    identity = {key: run.get(key) for key in _IDENTITY_FIELDS}
    if any(not isinstance(value, (str, int)) or isinstance(value, bool)
           for value in identity.values()):
        raise CampaignLedgerError("campaign run identity is incomplete")
    if (not isinstance(identity["attempt"], int)
            or not isinstance(identity["start_number"], int)
            or identity["attempt"] <= 0 or identity["start_number"] <= 0):
        raise CampaignLedgerError("campaign attempt and start numbers must be positive")
    if not all(identity[key] for key in (
            "authorization_grant_id", "model", "route", "manifest_sha256", "binary_sha256")):
        raise CampaignLedgerError("campaign run identity contains an empty field")

    request_limit = run.get("provider_request_limit")
    if request_limit is not None and (
            not isinstance(request_limit, int) or isinstance(request_limit, bool)
            or request_limit <= 0 or provider_requests > request_limit):
        raise CampaignLedgerError("provider request ceiling exceeded")

    rows = ledger.get("runs")
    if not isinstance(rows, list) or any(not isinstance(row, MutableMapping) for row in rows):
        raise CampaignLedgerError("campaign run table is invalid")
    matches = [row for row in rows if row.get("attempt") == identity["attempt"]]
    if len(matches) > 1:
        raise CampaignLedgerError("campaign attempt is duplicated")
    existing = matches[0] if matches else None
    if existing is not None:
        for key, value in identity.items():
            previous = existing.get(key)
            if previous is not None and previous != value:
                raise CampaignLedgerError(f"campaign attempt conflicts on {key}")
        previous_requests = existing.get("provider_requests_observed", 0)
        if not isinstance(previous_requests, int) or isinstance(previous_requests, bool):
            raise CampaignLedgerError("stored provider request count is invalid")
        if provider_requests < previous_requests:
            raise CampaignLedgerError("provider request count cannot move backwards")
    else:
        previous_requests = 0

    offset = ledger.get("unrepresented_counted_starts")
    total_limit = ledger.get("total_authorized_starts")
    if (not isinstance(offset, int) or isinstance(offset, bool) or offset < 0
            or not isinstance(total_limit, int) or isinstance(total_limit, bool)
            or total_limit <= 0):
        raise CampaignLedgerError("campaign authorization accounting is invalid")
    counted_rows = sum(bool(row.get("counted_as_start")) for row in rows)
    already_counted = existing is not None and bool(existing.get("counted_as_start"))
    expected_start = counted_rows + offset + (0 if already_counted else 1)
    if identity["start_number"] != expected_start:
        raise CampaignLedgerError("campaign start number does not match preserved offset")
    combined_starts = counted_rows + (0 if already_counted else 1) + offset
    if combined_starts > total_limit:
        raise CampaignLedgerError("combined campaign authorization exhausted")

    grants = ledger.get("supplemental_authorizations")
    if not isinstance(grants, list):
        raise CampaignLedgerError("supplemental authorization table is invalid")
    grant_id = str(identity["authorization_grant_id"])
    matching_grants = [grant for grant in grants
                       if isinstance(grant, MutableMapping)
                       and grant.get("grant_id") == grant_id]
    if len(matching_grants) != 1:
        raise CampaignLedgerError("campaign authorization grant is missing or duplicated")
    grant = matching_grants[0]
    if grant.get("status") != "active":
        raise CampaignLedgerError("campaign authorization grant is not active")
    model_name = str(identity["model"]).rsplit("/", 1)[-1]
    if grant.get("model") != model_name:
        raise CampaignLedgerError("campaign model does not match the authorization grant")
    route = str(identity["route"])
    grant_route = grant.get("route")
    if not isinstance(grant_route, str) or route not in {grant_route, grant_route.split("/", 1)[0]}:
        raise CampaignLedgerError("campaign route does not match the authorization grant")
    max_grant_starts = grant.get("maximum_additional_starts")
    if not isinstance(max_grant_starts, int) or isinstance(max_grant_starts, bool):
        raise CampaignLedgerError("grant start ceiling is invalid")
    prior_grant_rows = sum(
        bool(row.get("counted_as_start"))
        and row.get("authorization_grant_id") == grant_id
        and row is not existing
        for row in rows
    )
    grant_starts = prior_grant_rows + (1 if not already_counted else 0)
    if grant_starts > max_grant_starts:
        raise CampaignLedgerError("supplemental authorization exhausted")

    # All validation is complete. Mutations below cannot alter another attempt.
    row = existing if existing is not None else {}
    for key in _SAFE_RUN_FIELDS:
        if key in run and key not in _IDENTITY_FIELDS:
            row[key] = run[key]
    for key, value in identity.items():
        row[key] = value
    row["supplemental_grant_id"] = grant_id
    row["counted_as_start"] = True
    row["provider_requests_observed"] = provider_requests
    previous_status = row.get("status")
    if previous_status in _STATUS_RANK and _STATUS_RANK[previous_status] > _STATUS_RANK[status]:
        status = previous_status
    row["status"] = status
    if existing is None:
        rows.append(row)

    counted_rows += 0 if already_counted else 1
    combined_starts = counted_rows + offset
    ledger["runs_started"] = combined_starts
    ledger["runs_started_reconciliation"] = (
        f"{combined_starts} of {total_limit} combined starts consumed; "
        f"{max(0, total_limit - combined_starts)} remain. The run table contains "
        f"{counted_rows} counted rows and preserves {offset} unrepresented historical start(s)."
    )
    row["runs_started_counted_rows"] = counted_rows
    row["unrepresented_counted_starts"] = offset
    row["combined_starts_consumed"] = combined_starts
    row["combined_starts_remaining"] = max(0, total_limit - combined_starts)

    grant_consumed = sum(
        bool(candidate.get("counted_as_start"))
        and candidate.get("authorization_grant_id") == grant_id
        for candidate in rows
    )
    grant["starts_consumed"] = grant_consumed
    grant["starts_remaining"] = max(0, max_grant_starts - grant_consumed)
    return {
        "attempt": identity["attempt"],
        "start_number": identity["start_number"],
        "provider_requests_observed": provider_requests,
        "grant_starts_consumed": grant_consumed,
        "grant_starts_remaining": max(0, max_grant_starts - grant_consumed),
        "combined_starts_consumed": combined_starts,
        "combined_starts_remaining": max(0, total_limit - combined_starts),
        "unrepresented_counted_starts": offset,
        "status": status,
    }

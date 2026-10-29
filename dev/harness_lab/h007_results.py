"""H007's versioned, independent task and observation-horizon projection.

Legacy H005/H006 receipts keep their frozen meaning. This module is deliberately
pure so boundary and truth-table tests can exercise the new contract without a
provider, clock, filesystem, or ACP process.
"""
from __future__ import annotations

from collections.abc import Mapping, Sequence
from typing import Any

H007_RESULT_SCHEMA_VERSION = 1
H007_MIN_ACTIVE_DURATION_MS = 30 * 60 * 1000
H007_MAX_ACTIVITY_GAP_MS = 5 * 60 * 1000
HORIZON_REASONS = frozenset({
    "activity_span_short", "activity_gap_exceeded", "job_lifecycle_missing",
    "continuation_missing", "task_completed_before_threshold", "measurement_incomplete",
})


def _job_lifecycle(tool_calls: Sequence[Mapping[str, Any]]) -> tuple[bool, bool]:
    """Return (same exact job observed running and terminal, productive overlap)."""
    submissions = [call.get("job") for call in tool_calls
                   if call.get("tool") == "test" and isinstance(call.get("job"), Mapping)]
    productive = {"write", "edit", "apply_patch", "bash", "test"}
    job_ids = {job.get("job_id") for job in submissions if isinstance(job.get("job_id"), str)}
    for job_id in job_ids:
        observations = [(index, call, call.get("job")) for index, call in enumerate(tool_calls)
                        if isinstance(call.get("job"), Mapping)
                        and call["job"].get("job_id") == job_id]
        running = [index for index, _call, job in observations if job.get("status") == "running"]
        terminal = [index for index, _call, job in observations
                    if job.get("status") == "completed"]
        if running and terminal and min(terminal) > min(running):
            overlap = any(call.get("tool") in productive and index > min(running)
                          and index < min(terminal)
                          for index, call in enumerate(tool_calls))
            if overlap:
                return True, True
    return False, False


def project_result(*, terminal: str, verifier_ran: bool, verifier_exit_code: int | None,
                   duration_ms: int, tool_calls: Sequence[Mapping[str, Any]],
                   measurement_complete: bool, continuation_confirmed: bool,
                   repair_cycles: int, protected: bool = False) -> dict[str, Any]:
    """Project task correctness and horizon measurement as separate outcomes."""
    if protected:
        task_outcome = "inconclusive"
    elif terminal != "completed" or not verifier_ran or verifier_exit_code is None:
        task_outcome = "inconclusive"
    elif verifier_exit_code == 0:
        task_outcome = "passed"
    else:
        task_outcome = "failed"

    activity = sorted(
        value for call in tool_calls
        for value in (call.get("started_elapsed_ms"), call.get("finished_elapsed_ms"))
        if isinstance(value, int) and not isinstance(value, bool)
    )
    span = activity[-1] - activity[0] if len(activity) > 1 else 0
    gap = max((later - earlier for earlier, later in zip(activity, activity[1:])), default=0)
    lifecycle, overlap = _job_lifecycle(tool_calls)

    reason = None
    if not measurement_complete:
        reason = "measurement_incomplete"
    elif task_outcome == "passed" and span < H007_MIN_ACTIVE_DURATION_MS:
        reason = "task_completed_before_threshold"
    elif not lifecycle or not overlap:
        reason = "job_lifecycle_missing"
    elif not continuation_confirmed or repair_cycles < 2:
        reason = "continuation_missing"
    elif gap > H007_MAX_ACTIVITY_GAP_MS:
        reason = "activity_gap_exceeded"
    elif span < H007_MIN_ACTIVE_DURATION_MS:
        reason = ("task_completed_before_threshold" if task_outcome == "passed"
                  else "activity_span_short")

    return {
        "result_schema_version": H007_RESULT_SCHEMA_VERSION,
        "task_outcome": task_outcome,
        "horizon_outcome": "qualified" if reason is None else
                           "not_observed" if reason == "task_completed_before_threshold"
                           else "not_qualified",
        "horizon_reason": reason,
        "horizon_evidence": {
            "active_work_span_ms": span,
            "maximum_activity_gap_ms": gap,
            "minimum_active_duration_ms": H007_MIN_ACTIVE_DURATION_MS,
            "maximum_activity_gap_limit_ms": H007_MAX_ACTIVITY_GAP_MS,
            "exact_job_running_and_terminal": lifecycle,
            "productive_work_during_job": overlap,
            "continuation_confirmed": continuation_confirmed,
            "repair_cycles": repair_cycles,
            "measurement_complete": measurement_complete,
        },
    }


def summarize_h007(runs: Sequence[Mapping[str, Any]]) -> dict[str, int]:
    """Count task and horizon outcomes independently, preserving each denominator."""
    return {
        "task_denominator": sum(run.get("task_outcome") in
                                {"passed", "failed", "inconclusive"} for run in runs),
        "task_measured_denominator": sum(run.get("task_outcome") in {"passed", "failed"}
                                         for run in runs),
        "task_passes": sum(run.get("task_outcome") == "passed" for run in runs),
        "task_failures": sum(run.get("task_outcome") == "failed" for run in runs),
        "task_inconclusive": sum(run.get("task_outcome") == "inconclusive" for run in runs),
        "horizon_denominator": sum(run.get("horizon_outcome") in
                                    {"qualified", "not_qualified", "not_observed"}
                                    for run in runs),
        "horizon_qualified": sum(run.get("horizon_outcome") == "qualified" for run in runs),
        "horizon_not_qualified": sum(run.get("horizon_outcome") == "not_qualified"
                                      for run in runs),
        "horizon_not_observed": sum(run.get("horizon_outcome") == "not_observed"
                                     for run in runs),
    }

"""H007's versioned, independent task and observation-horizon projection.

Legacy H005/H006 receipts keep their frozen meaning. This module is deliberately
pure so boundary and truth-table tests can exercise the new contract without a
provider, clock, filesystem, or ACP process.
"""
from __future__ import annotations

import hashlib
import re
from collections import Counter
from collections.abc import Mapping, Sequence
from typing import Any

from .diagnostics import SAFE_JOB_STATUS_ERROR_CLASSES, SAFE_TEST_SUBMIT_ERROR_CLASSES

H007_RESULT_SCHEMA_VERSION = 3
H007_MIN_ACTIVE_DURATION_MS = 30 * 60 * 1000
H007_MAX_ACTIVITY_GAP_MS = 5 * 60 * 1000
HORIZON_REASONS = frozenset({
    "activity_span_short", "activity_gap_exceeded", "job_lifecycle_missing",
    "repair_cycles_insufficient", "continuation_missing",
    "task_completed_before_threshold", "task_not_passed", "agent_terminal_incomplete",
    "measurement_incomplete", "verifier_setup_error", "verifier_timeout",
    "verifier_runner_error", "verifier_not_run",
})
MAX_CONTINUATION_LAYOUT_CHARS = 256 * 1024
MAX_CONTINUATION_LAYOUT_LINES = 4096

_JOB_ID_LINE = re.compile(r"^job_id_([A-Za-z0-9_-]+):\s*(\S+)\s*$")
_JOB_STATE_LINE = re.compile(
    r"^(?:job_state_([A-Za-z0-9_-]+)|job_id_([A-Za-z0-9_-]+)_(?:state|running|terminal))"
    r":\s*(\S+)(?:\s+.*)?$")
_REPAIR_CYCLES_LINE = re.compile(r"^repair_cycles:\s*(\d+)\s*$")
H007_REQUIRED_CONTINUATION_PHASE_ORDER = (
    "versioned_records", "durable_import", "dependency_index", "provenance_queries",
    "cli_export", "catalog_bundle", "catalog_delta", "catalog_merge",
    "catalog_replication", "catalog_checkpoint", "reconcile", "reverse_impact",
    "catalog_policy", "impact_view_api", "workspace_audit",
)
H007_REQUIRED_CONTINUATION_PHASES = frozenset(H007_REQUIRED_CONTINUATION_PHASE_ORDER)
H007_REQUIRED_CONTINUATION_GATE_ORDER = (
    "starter_repair_format", "starter_repair_store", "cli_contract", "release_overlap",
    "acceptance_audit", "harness_probes", "workspace_tests_clippy_review",
)
H007_REQUIRED_CONTINUATION_GATES = frozenset(H007_REQUIRED_CONTINUATION_GATE_ORDER)
MAX_PUBLIC_JOB_CORRELATION_CALLS = 1_600
_PHASE_NAME = re.compile(r"^[a-z][a-z0-9_]*$")
_PHASE_ENTRY = re.compile(r"^([a-z][a-z0-9_]*)(?:\s*:\s*(.*))?$")
_GATE_LINE = re.compile(
    r"^-\s+([a-z][a-z0-9_]*)\s*:\s*([a-z][a-z0-9_-]*)(?:\s+.*)?$"
)
_STAGE_LINE = _GATE_LINE
_TEST_JOB_TERMINAL_STATES = frozenset({
    "completed", "failed", "cancelled", "timed_out", "interrupted",
})


def unobserved_test_job_ids(tool_calls: Sequence[Mapping[str, Any]]) -> list[str]:
    """Return submitted test-job IDs that have no observed terminal state.

    Raw IDs are only for same-session continuation prompts so the agent can
    observe the exact existing jobs. Safe receipts use the summary below and
    never retain these identifiers.
    """
    submitted = {
        call["job"].get("job_id")
        for call in tool_calls
        if call.get("tool") == "test"
        and isinstance(call.get("job"), Mapping)
        and call["job"].get("action") == "submit"
        and isinstance(call["job"].get("job_id"), str)
    }
    terminal = {
        call["job"].get("job_id")
        for call in tool_calls
        if isinstance(call.get("job"), Mapping)
        and call["job"].get("status") in _TEST_JOB_TERMINAL_STATES
        and isinstance(call["job"].get("job_id"), str)
    }
    return sorted(job_id for job_id in submitted - terminal
                  if re.fullmatch(r"[A-Za-z0-9_-]{1,128}", job_id))


def summarize_test_job_lifecycle(
    tool_calls: Sequence[Mapping[str, Any]],
) -> dict[str, Any]:
    """Summarize submitted test jobs without exposing their IDs or arguments."""
    submissions = [
        call["job"]
        for call in tool_calls
        if call.get("tool") == "test"
        and isinstance(call.get("job"), Mapping)
        and call["job"].get("action") == "submit"
        and isinstance(call["job"].get("job_id"), str)
    ]
    job_ids = {job["job_id"] for job in submissions}
    terminal_by_id: dict[str, str] = {}
    latest_by_id: dict[str, str] = {}
    for call in tool_calls:
        job = call.get("job")
        if not isinstance(job, Mapping) or job.get("job_id") not in job_ids:
            continue
        job_id = job["job_id"]
        status = job.get("status")
        if status in _TEST_JOB_TERMINAL_STATES:
            terminal_by_id[job_id] = status
            latest_by_id[job_id] = status
        elif status in {"queued", "running", "waiting", "preparing", "cancelling"}:
            latest_by_id[job_id] = status
    unobserved = job_ids - terminal_by_id.keys()
    return {
        "submission_count": len(submissions),
        "unique_submitted_job_count": len(job_ids),
        "terminal_observed_count": len(terminal_by_id),
        "unobserved_count": len(unobserved),
        "unobserved_status_counts": dict(sorted(
            Counter(
                latest_by_id.get(job_id, "no_status_observed") for job_id in unobserved
            ).items()
        )),
        "terminal_status_counts": dict(sorted(
            Counter(terminal_by_id.values()).items()
        )),
        "all_terminal_observed": not unobserved,
        "raw_job_ids_retained": False,
        "raw_job_arguments_retained": False,
    }


def _documented_job_evidence(
    lines: Sequence[str],
) -> tuple[dict[str, str], dict[str, set[str]], set[str]]:
    documented_ids: dict[str, str] = {}
    documented_states: dict[str, set[str]] = {}
    duplicate_labels: set[str] = set()
    for line in lines:
        # Parse states first because job_id_<label>_terminal also has the
        # lexical prefix of a plain job ID field.
        match = _JOB_STATE_LINE.fullmatch(line.strip())
        if match:
            label = match.group(1) or match.group(2)
            documented_states.setdefault(label, set()).add(match.group(3).lower())
            continue
        match = _JOB_ID_LINE.fullmatch(line.strip())
        if match:
            label, job_id = match.groups()
            if label in documented_ids:
                duplicate_labels.add(label)
            documented_ids[label] = job_id
    return documented_ids, documented_states, duplicate_labels


def summarize_required_success_jobs(
    text: str | None,
    tool_calls: Sequence[Mapping[str, Any]],
    labels: Sequence[str],
) -> dict[str, Any]:
    """Verify named final test jobs without retaining their IDs or commands."""
    lines = text.splitlines() if isinstance(text, str) else []
    documented_ids, documented_states, duplicate_labels = _documented_job_evidence(lines)
    labels = tuple(labels)
    duplicate_ids = {
        label for label in labels
        if documented_ids.get(label) is not None
        and sum(documented_ids.get(other) == documented_ids[label] for other in labels) > 1
    }
    summaries: dict[str, dict[str, Any]] = {}
    for label in labels:
        job_id = documented_ids.get(label)
        submissions = [
            index for index, call in enumerate(tool_calls)
            if call.get("tool") == "test"
            and isinstance(call.get("job"), Mapping)
            and call["job"].get("action") == "submit"
            and call["job"].get("job_id") == job_id
        ] if isinstance(job_id, str) else []
        observed_terminal_states = [
            str(call["job"].get("status")).lower()
            for index, call in enumerate(tool_calls)
            if submissions and index > submissions[0]
            and isinstance(call.get("job"), Mapping)
            and call["job"].get("job_id") == job_id
            and call["job"].get("status") in _TEST_JOB_TERMINAL_STATES
        ]
        final_observed_status = observed_terminal_states[-1] if observed_terminal_states else None
        declared_states = documented_states.get(label, set())
        completed = (
            label not in duplicate_labels
            and label not in duplicate_ids
            and len(submissions) == 1
            and final_observed_status == "completed"
            and "completed" in declared_states
            and not declared_states & {"failed", "cancelled", "canceled", "timed_out"}
        )
        summaries[label] = {
            "submitted_once": len(submissions) == 1,
            "documented_completed": "completed" in declared_states,
            "observed_terminal_status": final_observed_status or "unobserved",
            "completed": completed,
        }
    return {
        "jobs": summaries,
        "complete": bool(labels) and all(item["completed"] for item in summaries.values()),
        "raw_job_ids_retained": False,
        "raw_job_arguments_retained": False,
    }


def _observed_repair_cycle_pairs(
    tool_calls: Sequence[Mapping[str, Any]],
    *,
    preferred_job_pairs: Sequence[tuple[str, str]] = (),
) -> list[dict[str, Any]]:
    """Pair failed test jobs with one later repaired success and source work.

    Job identifiers stay in memory for exact correlation. Callers that persist
    this projection must hash the IDs and retain no command arguments.
    """
    submissions: dict[str, list[int]] = {}
    terminal_by_id: dict[str, tuple[int, str, int | None]] = {}
    productive_tools = {"write", "edit", "replace", "apply_patch"}
    for index, call in enumerate(tool_calls):
        job = call.get("job")
        if not isinstance(job, Mapping):
            continue
        job_id = job.get("job_id")
        if (call.get("tool") == "test" and job.get("action") == "submit"
                and isinstance(job_id, str)):
            submissions.setdefault(job_id, []).append(index)
        if (call.get("tool") == "job" and job.get("action") == "status"
                and isinstance(job_id, str)
                and job.get("status") in {"failed", "completed"}):
            terminal_by_id.setdefault(job_id, (
                index, str(job.get("status")), call.get("finished_elapsed_ms")
                if isinstance(call.get("finished_elapsed_ms"), int) else None,
            ))

    failures = sorted(
        ((terminal[0], job_id) for job_id, terminal in terminal_by_id.items()
         if len(submissions.get(job_id, [])) == 1 and terminal[1] == "failed"),
    )
    successes = sorted(
        ((submissions[job_id][0], terminal[0], job_id, terminal[2])
         for job_id, terminal in terminal_by_id.items()
         if len(submissions.get(job_id, [])) == 1 and terminal[1] == "completed"),
    )
    paired_failures: set[str] = set()
    paired_successes: set[str] = set()
    pairs: list[dict[str, Any]] = []

    def append_pair(failed_job_id: str, repaired_job_id: str) -> bool:
        failed_terminal = terminal_by_id.get(failed_job_id)
        repair_submission = submissions.get(repaired_job_id, [])
        repair_terminal = terminal_by_id.get(repaired_job_id)
        if (failed_job_id == repaired_job_id
                or failed_job_id in paired_failures
                or repaired_job_id in paired_successes
                or len(submissions.get(failed_job_id, [])) != 1
                or len(repair_submission) != 1
                or failed_terminal is None or failed_terminal[1] != "failed"
                or repair_terminal is None or repair_terminal[1] != "completed"
                or repair_submission[0] <= failed_terminal[0]
                or repair_terminal[0] <= repair_submission[0]):
            return False
        if not any(
            failed_terminal[0] < index < repair_submission[0]
            and call.get("tool") in productive_tools
            for index, call in enumerate(tool_calls)
        ):
            return False
        pairs.append({
            "failed_job_id": failed_job_id,
            "repair_job_id": repaired_job_id,
            "repair_terminal_elapsed_ms": repair_terminal[2],
        })
        paired_failures.add(failed_job_id)
        paired_successes.add(repaired_job_id)
        return True

    # Prefer explicitly named repair jobs so an unrelated later successful
    # suite cannot be attributed to the seeded failure when both are present.
    for failed_job_id, repaired_job_id in preferred_job_pairs:
        append_pair(failed_job_id, repaired_job_id)

    for _failure_index, failed_job_id in failures:
        if failed_job_id in paired_failures:
            continue
        for _submit_index, _terminal_index, repaired_job_id, _elapsed_ms in successes:
            if append_pair(failed_job_id, repaired_job_id):
                break
    return pairs


def documented_repair_job_id_pairs(
    text: str | None,
    groups: Sequence[tuple[str, str, str]],
) -> tuple[tuple[str, str], ...]:
    """Return unique documented failure/repair IDs for in-memory correlation only."""
    lines = text.splitlines() if isinstance(text, str) else []
    documented_ids, _states, duplicate_labels = _documented_job_evidence(lines)
    if duplicate_labels:
        return ()
    pairs: list[tuple[str, str]] = []
    seen: set[str] = set()
    for _target, failed_label, repair_label in groups:
        failed_id = documented_ids.get(failed_label)
        repair_id = documented_ids.get(repair_label)
        if (not isinstance(failed_id, str) or not isinstance(repair_id, str)
                or failed_id == repair_id or failed_id in seen or repair_id in seen):
            return ()
        seen.update((failed_id, repair_id))
        pairs.append((failed_id, repair_id))
    return tuple(pairs)


def summarize_required_repair_jobs(
    text: str | None,
    tool_calls: Sequence[Mapping[str, Any]],
    groups: Sequence[tuple[str, str, str]],
) -> dict[str, Any]:
    """Validate named seed-failure/rerun pairs against exact native job IDs.

    Each tuple is ``(target, failed_label, repair_label)``. The public note may
    contain raw IDs, but this projection returns only booleans and counts.
    """
    lines = text.splitlines() if isinstance(text, str) else []
    documented_ids, documented_states, duplicate_labels = _documented_job_evidence(lines)
    state_line_counts: Counter[str] = Counter()
    cycle_values: list[int] = []
    for line in lines:
        state_match = _JOB_STATE_LINE.fullmatch(line.strip())
        if state_match:
            state_line_counts[state_match.group(1) or state_match.group(2)] += 1
        cycle_match = _REPAIR_CYCLES_LINE.fullmatch(line.strip())
        if cycle_match:
            cycle_values.append(int(cycle_match.group(1)))

    submissions: dict[str, list[int]] = {}
    for index, call in enumerate(tool_calls):
        job = call.get("job")
        if (call.get("tool") == "test" and isinstance(job, Mapping)
                and job.get("action") == "submit" and isinstance(job.get("job_id"), str)):
            submissions.setdefault(job["job_id"], []).append(index)

    def observations(job_id: str, expected: str) -> list[int]:
        return [index for index, call in enumerate(tool_calls)
                if call.get("tool") == "job" and isinstance(call.get("job"), Mapping)
                and call["job"].get("action") == "status"
                and call["job"].get("job_id") == job_id
                and call["job"].get("status") == expected]

    summaries: dict[str, dict[str, bool]] = {}
    all_group_ids: list[str] = []
    for target, failed_label, repair_label in groups:
        failed_id = documented_ids.get(failed_label)
        repair_id = documented_ids.get(repair_label)
        if isinstance(failed_id, str):
            all_group_ids.append(failed_id)
        if isinstance(repair_id, str):
            all_group_ids.append(repair_id)
        failed_submits = submissions.get(failed_id, []) if isinstance(failed_id, str) else []
        repair_submits = submissions.get(repair_id, []) if isinstance(repair_id, str) else []
        failed_observations = observations(failed_id, "failed") if isinstance(failed_id, str) else []
        repair_observations = observations(repair_id, "completed") if isinstance(repair_id, str) else []
        failed_immediate = (
            len(failed_submits) == 1 and failed_submits[0] + 1 < len(tool_calls)
            and tool_calls[failed_submits[0] + 1].get("tool") == "job"
            and isinstance(tool_calls[failed_submits[0] + 1].get("job"), Mapping)
            and tool_calls[failed_submits[0] + 1]["job"].get("action") == "status"
            and tool_calls[failed_submits[0] + 1]["job"].get("job_id") == failed_id
        )
        repair_immediate = (
            len(repair_submits) == 1 and repair_submits[0] + 1 < len(tool_calls)
            and tool_calls[repair_submits[0] + 1].get("tool") == "job"
            and isinstance(tool_calls[repair_submits[0] + 1].get("job"), Mapping)
            and tool_calls[repair_submits[0] + 1]["job"].get("action") == "status"
            and tool_calls[repair_submits[0] + 1]["job"].get("job_id") == repair_id
        )
        order_valid = (
            len(failed_submits) == 1 and len(repair_submits) == 1
            and bool(failed_observations) and bool(repair_observations)
            and repair_submits[0] > failed_observations[0]
        )
        productive_repair = (
            order_valid and any(
                failed_observations[0] < index < repair_submits[0]
                and call.get("tool") in {
                    "write", "edit", "replace", "apply_patch"
                }
                for index, call in enumerate(tool_calls)
            )
        )
        states_documented = (
            isinstance(failed_id, str) and isinstance(repair_id, str)
            and documented_states.get(failed_label) == {"failed"}
            and documented_states.get(repair_label) == {"completed"}
            and state_line_counts[failed_label] == 1
            and state_line_counts[repair_label] == 1
        )
        ids_unique = (
            isinstance(failed_id, str) and isinstance(repair_id, str)
            and failed_id != repair_id and failed_label not in duplicate_labels
            and repair_label not in duplicate_labels
        )
        group = {
            "failed_job_submitted_once": len(failed_submits) == 1,
            "failed_job_status_called_immediately": failed_immediate,
            "failed_terminal_observed": bool(failed_observations),
            "repair_job_submitted_once": len(repair_submits) == 1,
            "repair_job_status_called_immediately": repair_immediate,
            "repair_terminal_observed": bool(repair_observations),
            "repair_followed_failure": order_valid,
            "productive_change_between_failure_and_rerun": productive_repair,
            "documented_states_match_observations": states_documented,
            "job_ids_are_distinct": ids_unique,
        }
        group["qualified"] = all(group.values())
        summaries[target] = group

    preferred_pairs = documented_repair_job_id_pairs(text, groups)
    cycles = _observed_repair_cycle_pairs(
        tool_calls, preferred_job_pairs=preferred_pairs,
    )
    correlated_pairs = {
        (pair["failed_job_id"], pair["repair_job_id"])
        for pair in cycles
    }
    for target, failed_label, repair_label in groups:
        summaries[target]["repair_cycle_correlated"] = (
            documented_ids.get(failed_label), documented_ids.get(repair_label)
        ) in correlated_pairs
    documented_cycles = cycle_values[0] if len(cycle_values) == 1 else None
    cycles_match = documented_cycles == len(cycles)
    ids_unique_across_groups = len(all_group_ids) == len(set(all_group_ids))
    complete = (
        bool(groups) and all(item["qualified"] for item in summaries.values())
        and ids_unique_across_groups and len(cycles) >= len(groups)
        and cycles_match
    )
    return {
        "groups": summaries,
        "required_pair_count": len(groups),
        "qualified_pair_count": sum(item["qualified"] for item in summaries.values()),
        "observed_repair_cycle_count": len(cycles),
        "documented_repair_cycle_count": documented_cycles,
        "repair_cycle_count_matches": cycles_match,
        "job_ids_unique_across_groups": ids_unique_across_groups,
        "complete": complete,
        "raw_job_ids_retained": False,
        "raw_job_arguments_retained": False,
    }


def documented_phases(text: str) -> list[str] | None:
    """Read the bounded public phase list in block or inline YAML form."""
    if not isinstance(text, str):
        return None
    lines = text.splitlines()
    headers = [index for index, line in enumerate(lines)
               if line.partition(":")[0].strip() == "completed_phases"]
    if len(headers) != 1:
        return None

    _key, separator, value = lines[headers[0]].partition(":")
    if not separator:
        return None
    value = value.strip()
    if value:
        if not (value.startswith("[") and value.endswith("]")):
            return None
        inner = value[1:-1].strip()
        if not inner:
            return []
        phases = [phase.strip() for phase in inner.split(",")]
        return phases if all(_PHASE_NAME.fullmatch(phase) for phase in phases) else None

    phases: dict[str, bool] = {}
    for line in lines[headers[0] + 1:]:
        stripped = line.strip()
        if not stripped:
            continue
        if not stripped.startswith("-"):
            break
        match = _PHASE_ENTRY.fullmatch(stripped[1:].strip())
        if match is None:
            return None
        phase, evidence = match.groups()
        status = evidence.strip().lower() if evidence is not None else ""
        phases[phase] = status not in {"pending", "failed", "incomplete", "not_started"}
    return [phase for phase, complete in phases.items() if complete]


def documented_contract_phases(text: str) -> list[str] | None:
    """Read the optional ``contract_phases:`` list, separate from H007 phases."""
    if not isinstance(text, str):
        return None
    lines = text.splitlines()
    headers = [index for index, line in enumerate(lines)
               if line.partition(":")[0].strip() == "contract_phases"]
    if len(headers) != 1:
        return None

    phases: dict[str, bool] = {}
    for line in lines[headers[0] + 1:]:
        stripped = line.strip()
        if not stripped:
            continue
        if not stripped.startswith("-"):
            break
        match = _PHASE_ENTRY.fullmatch(stripped[1:].strip())
        if match is None:
            return None
        phase, evidence = match.groups()
        status = evidence.strip().lower() if evidence is not None else ""
        phases[phase] = status not in {"pending", "failed", "incomplete", "not_started"}
    return [phase for phase, complete in phases.items() if complete]


def documented_gates(text: str) -> dict[str, str] | None:
    """Read the optional bounded ``gates:`` checklist from the public note.

    A present but malformed checklist is invalid. Evidence after a status is
    descriptive, and a repeated gate uses its latest status so a later
    regression cannot be hidden by an earlier completion. Callers can require
    the section for prompts that explicitly ask the agent to maintain a gate
    ledger, while legacy continuation notes remain supported.
    """
    if not isinstance(text, str):
        return None
    lines = text.splitlines()
    headers = [index for index, line in enumerate(lines)
               if line.partition(":")[0].strip() == "gates"]
    if len(headers) != 1:
        return None

    gates: dict[str, str] = {}
    for line in lines[headers[0] + 1:]:
        stripped = line.strip()
        if not stripped:
            continue
        if not stripped.startswith("-"):
            break
        match = _GATE_LINE.fullmatch(stripped)
        if match is None:
            return None
        name, status = match.groups()
        # Agents may append a later status as work continues. The final entry
        # is authoritative, including when it regresses to pending or failed.
        gates[name] = status
    return gates or None


def documented_stages(text: str) -> dict[str, str] | None:
    """Read the optional bounded ``required_stages:`` task-stage checklist."""
    if not isinstance(text, str):
        return None
    lines = text.splitlines()
    headers = [index for index, line in enumerate(lines)
               if line.partition(":")[0].strip() == "required_stages"]
    if len(headers) != 1:
        return None

    stages: dict[str, str] = {}
    for line in lines[headers[0] + 1:]:
        stripped = line.strip()
        if not stripped:
            continue
        if not stripped.startswith("-"):
            break
        match = _STAGE_LINE.fullmatch(stripped)
        if match is None:
            return None
        name, status = match.groups()
        # A later checkpoint is authoritative, including a regression.
        stages[name] = status
    return stages or None


def summarize_continuation_line_layout(text: str | None) -> dict[str, Any]:
    """Project section and terminal-pointer placement without retaining lines."""
    if not isinstance(text, str) or not text.strip():
        return {
            "schema": "h007_continuation_line_layout_v1",
            "state": "unavailable",
            "progress_sections_ordered": None,
            "pointer_fields_follow_progress_sections": None,
            "pointer_fields_in_expected_order": None,
            "pointer_block_is_terminal": None,
            "raw_note_retained": False,
        }
    if len(text) > MAX_CONTINUATION_LAYOUT_CHARS:
        return {
            "schema": "h007_continuation_line_layout_v1",
            "state": "limit_exceeded",
            "progress_sections_ordered": None,
            "pointer_fields_follow_progress_sections": None,
            "pointer_fields_in_expected_order": None,
            "pointer_block_is_terminal": None,
            "raw_note_retained": False,
        }

    lines = text.splitlines()
    if len(lines) > MAX_CONTINUATION_LAYOUT_LINES:
        return {
            "schema": "h007_continuation_line_layout_v1",
            "state": "limit_exceeded",
            "progress_sections_ordered": None,
            "pointer_fields_follow_progress_sections": None,
            "pointer_fields_in_expected_order": None,
            "pointer_block_is_terminal": None,
            "raw_note_retained": False,
        }
    progress_keys = (
        "completed_phases", "completed_contract_phases", "required_stages", "gates",
    )
    pointer_keys = ("next_phase", "next_contract_phase", "next_stage", "next_gate")
    positions = {
        key: [index for index, line in enumerate(lines)
              if line.partition(":")[0].strip() == key]
        for key in (*progress_keys, *pointer_keys)
    }

    def section_end(key: str) -> int | None:
        headers = positions[key]
        if len(headers) != 1:
            return None
        header = headers[0]
        value = lines[header].partition(":")[2].strip()
        if value:
            return header
        end = header
        for index in range(header + 1, len(lines)):
            stripped = lines[index].strip()
            if not stripped:
                continue
            if not stripped.startswith("-"):
                break
            end = index
        return end

    section_positions = [positions[key][0] for key in progress_keys
                         if len(positions[key]) == 1]
    section_ends = [section_end(key) for key in progress_keys]
    pointer_positions = [positions[key][0] for key in pointer_keys
                         if len(positions[key]) == 1]
    complete_shape = (
        all(len(positions[key]) == 1 for key in (*progress_keys, *pointer_keys))
        and all(value is not None for value in section_ends)
    )
    progress_end = max((value for value in section_ends if value is not None), default=-1)
    final_pointer = max(pointer_positions, default=-1)
    trailing_nonblank = any(line.strip() for line in lines[final_pointer + 1:])
    return {
        "schema": "h007_continuation_line_layout_v1",
        "state": "observed",
        "line_count": len(lines),
        "progress_section_count": sum(len(positions[key]) == 1 for key in progress_keys),
        "pointer_field_count": sum(len(positions[key]) == 1 for key in pointer_keys),
        "progress_sections_ordered": (
            section_positions == sorted(section_positions)
            if len(section_positions) == len(progress_keys) else None
        ),
        "pointer_fields_follow_progress_sections": (
            min(pointer_positions) > progress_end if complete_shape else None
        ),
        "pointer_fields_in_expected_order": (
            pointer_positions == sorted(pointer_positions)
            if len(pointer_positions) == len(pointer_keys) else None
        ),
        "pointer_block_is_terminal": (
            not trailing_nonblank if len(pointer_positions) == len(pointer_keys) else None
        ),
        "progress_end_line_index": progress_end if complete_shape else None,
        "first_pointer_line_index": min(pointer_positions)
        if len(pointer_positions) == len(pointer_keys) else None,
        "last_pointer_line_index": final_pointer
        if len(pointer_positions) == len(pointer_keys) else None,
        "raw_note_retained": False,
    }


def summarize_continuation_document(
    text: str | None,
    tool_calls: Sequence[Mapping[str, Any]], *,
    required_phases: Sequence[str],
    required_contract_phases: Sequence[str] | None = None,
    required_stages: Sequence[str] | None = None,
    required_gates: Sequence[str] | None = None,
    required_job_label: str | None = None,
    required_job_command: str | None = None,
    required_success_job_labels: Sequence[str] | None = None,
    required_repair_job_pairs: Sequence[tuple[str, str, str]] | None = None,
    require_all_test_jobs_terminal: bool = False,
) -> dict[str, Any]:
    """Project public continuation state without retaining its body or job IDs."""
    if not isinstance(text, str) or not text.strip():
        return {
            "schema": "h007_continuation_diagnostic_v1",
            "present": False,
            "note_sha256": None,
            "document_valid": False,
            "required_success_jobs": (
                summarize_required_success_jobs(None, tool_calls, required_success_job_labels)["jobs"]
                if required_success_job_labels is not None else None
            ),
            "incomplete_required_success_jobs": list(required_success_job_labels or ()),
            "repair_job_diagnostic": (
                summarize_required_repair_jobs(None, tool_calls, required_repair_job_pairs)
                if required_repair_job_pairs is not None else None
            ),
            "raw_note_retained": False,
            "raw_job_ids_retained": False,
        }

    phases = documented_phases(text)
    contract_phases = documented_contract_phases(text)
    stages = documented_stages(text) if required_stages is not None else None
    gates = documented_gates(text) if required_gates is not None else None
    lines = text.splitlines()

    def pointer_state(key: str, allowed: Sequence[str]) -> str:
        values = [line.partition(":")[2].strip() for line in lines
                  if line.partition(":")[0].strip() == key]
        if not values:
            return "missing"
        if len(values) != 1:
            return "duplicate"
        value = values[0]
        if value == "none":
            return "none"
        return value if value in allowed else "other"

    allowed_statuses = {"complete", "completed", "pending", "failed",
                        "in_progress", "not_started"}
    projected_stages = None
    if required_stages is not None:
        projected_stages = {
            name: (stages or {}).get(name)
            if (stages or {}).get(name) in allowed_statuses else "other"
            for name in required_stages
        }
    projected_gates = None
    if required_gates is not None:
        projected_gates = {
            name: (gates or {}).get(name)
            if (gates or {}).get(name) in allowed_statuses else "other"
            for name in required_gates
        }

    lifecycle = summarize_test_job_lifecycle(tool_calls)
    success_job_summary = (
        summarize_required_success_jobs(text, tool_calls, required_success_job_labels)
        if required_success_job_labels is not None else None
    )
    valid = continuation_document_confirmed(
        text,
        tool_calls,
        require_gate_block=required_gates is not None,
        required_phases=required_phases,
        required_contract_phases=required_contract_phases,
        required_stages=required_stages,
        required_gates=required_gates,
        required_job_label=required_job_label,
        required_job_command=required_job_command,
        required_success_job_labels=required_success_job_labels,
        required_repair_job_pairs=required_repair_job_pairs,
        require_all_test_jobs_terminal=require_all_test_jobs_terminal,
    )
    return {
        "schema": "h007_continuation_diagnostic_v1",
        "present": True,
        "note_sha256": hashlib.sha256(text.encode("utf-8")).hexdigest(),
        "line_layout": summarize_continuation_line_layout(text),
        "completed_phases": phases or [],
        "missing_phases": [name for name in required_phases if name not in (phases or [])],
        "phase_order_matches": phases == list(required_phases),
        "contract_phases": contract_phases,
        "missing_contract_phases": (
            [name for name in required_contract_phases
             if name not in (contract_phases or [])]
            if required_contract_phases is not None else None
        ),
        "contract_phase_order_matches": (
            contract_phases == list(required_contract_phases)
            if required_contract_phases is not None else None
        ),
        "stages": projected_stages,
        "incomplete_stages": (
            [name for name in required_stages
             if not stages or stages.get(name) not in {"complete", "completed"}]
            if required_stages is not None else None
        ),
        "stage_order_matches": (
            list(stages or {}) == list(required_stages)
            if required_stages is not None else None
        ),
        "gates": projected_gates,
        "incomplete_gates": (
            [name for name in required_gates
             if not gates or gates.get(name) not in {"complete", "completed"}]
            if required_gates is not None else None
        ),
        "next_phase": pointer_state("next_phase", required_phases),
        "next_contract_phase": (
            pointer_state("next_contract_phase", required_contract_phases)
            if required_contract_phases is not None else None
        ),
        "next_stage": (pointer_state("next_stage", required_stages)
                       if required_stages is not None else None),
        "next_gate": (pointer_state("next_gate", required_gates)
                      if required_gates is not None else None),
        "test_job_lifecycle": lifecycle,
        "required_success_jobs": (
            success_job_summary["jobs"] if success_job_summary is not None else None
        ),
        "incomplete_required_success_jobs": (
            [label for label, value in success_job_summary["jobs"].items()
             if not value["completed"]]
            if success_job_summary is not None else None
        ),
        "repair_job_diagnostic": (
            summarize_required_repair_jobs(text, tool_calls, required_repair_job_pairs)
            if required_repair_job_pairs is not None else None
        ),
        "document_valid": valid,
        "raw_note_retained": False,
        "raw_job_ids_retained": False,
        "raw_job_arguments_retained": False,
    }


def continuation_document_confirmed(text: str,
                                    tool_calls: Sequence[Mapping[str, Any]], *,
                                    require_gate_block: bool = False,
                                    required_phases: Sequence[str] | None = None,
                                    required_contract_phases: Sequence[str] | None = None,
                                    required_stages: Sequence[str] | None = None,
                                    required_gates: Sequence[str] | None = None,
                                    required_job_label: str | None = None,
                                    required_job_command: str | None = None,
                                    required_success_job_labels: Sequence[str] | None = None,
                                    required_repair_job_pairs: Sequence[tuple[str, str, str]]
                                    | None = None,
                                    require_all_test_jobs_terminal: bool = False) -> bool:
    """Validate public continuation evidence against completed native jobs.

    A continuation note counts only when it records the required H007 phases,
    completes the separate contract-phase and task-stage ledgers when required, completes
    every declared and required gate, declares ``next_phase: none`` (and
    ``next_stage: none`` for separate stage ledgers), and names the required job
    (when configured) with running-to-completed productive work in this same
    transcript. A terminal marker cannot override pending or missing stages or
    gates. A case may also require every submitted native test job to have an
    observed terminal state before its workspace qualification can complete.
    """
    if not isinstance(text, str) or not text.strip():
        return False

    lines = text.splitlines()
    required_phase_set = (frozenset(required_phases) if required_phases is not None
                          else H007_REQUIRED_CONTINUATION_PHASES)
    required_gate_set = (frozenset(required_gates) if required_gates is not None
                         else H007_REQUIRED_CONTINUATION_GATES)
    gate_headers = [line for line in lines
                    if line.partition(":")[0].strip() == "gates"]
    if require_gate_block or gate_headers:
        gates = documented_gates(text)
        if (gates is None
                or not required_gate_set.issubset(gates)
                or any(status not in {"complete", "completed"}
                       for status in gates.values())):
            return False

    phases = documented_phases(text)
    if phases is None:
        return False
    if (not phases or len(phases) != len(set(phases))
            or not required_phase_set.issubset(phases)
            or any(phase.lower() in {"none", "pending", "null"} for phase in phases)):
        return False
    if required_phases is not None and phases != list(required_phases):
        return False

    if required_contract_phases is not None:
        contract_phases = documented_contract_phases(text)
        if (contract_phases is None
                or contract_phases != list(required_contract_phases)):
            return False
        next_contract_phases = [line.partition(":")[2].strip() for line in lines
                                if line.partition(":")[0].strip() == "next_contract_phase"]
        if next_contract_phases != ["none"]:
            return False

    if required_stages is not None:
        stages = documented_stages(text)
        if (stages is None
                or list(stages) != list(required_stages)
                or any(status not in {"complete", "completed"}
                       for status in stages.values())):
            return False
        next_stages = [line.partition(":")[2].strip() for line in lines
                       if line.partition(":")[0].strip() == "next_stage"]
        if next_stages != ["none"]:
            return False

    next_phases = [line.partition(":")[2].strip() for line in lines
                   if line.partition(":")[0].strip() == "next_phase"]
    if next_phases != ["none"]:
        return False

    documented_ids, documented_states, duplicate_labels = _documented_job_evidence(lines)
    if duplicate_labels:
        return False

    required_job_id = None
    if required_job_label is not None:
        required_job_id = documented_ids.get(required_job_label)
        if not isinstance(required_job_id, str):
            return False
        required_states = documented_states.get(required_job_label, set())
        if not {"running", "completed"}.issubset(required_states) or required_states & {
                "failed", "cancelled", "canceled", "timed_out"}:
            return False
    if required_job_command is not None:
        commands = [line.partition(":")[2].strip() for line in lines
                    if line.partition(":")[0].strip() == "job_command_overlap"]
        if commands != [required_job_command]:
            return False

    if required_success_job_labels is not None:
        success_jobs = summarize_required_success_jobs(text, tool_calls,
                                                       required_success_job_labels)
        if not success_jobs["complete"]:
            return False

    if (required_repair_job_pairs is not None
            and not summarize_required_repair_jobs(
                text, tool_calls, required_repair_job_pairs)["complete"]):
        return False

    completed_job_ids = {
        job_id for label, job_id in documented_ids.items()
        if "completed" in documented_states.get(label, set())
        and not documented_states.get(label, set())
        & {"failed", "cancelled", "timed_out"}
    }
    if required_job_id is not None:
        productive_tools = {"write", "edit", "apply_patch", "bash", "test"}
        submitted = [index for index, call in enumerate(tool_calls)
                     if call.get("tool") == "test"
                     and isinstance(call.get("job"), Mapping)
                     and call["job"].get("job_id") == required_job_id]
        running = [index for index, call in enumerate(tool_calls)
                   if isinstance(call.get("job"), Mapping)
                   and call["job"].get("job_id") == required_job_id
                   and call["job"].get("status") == "running"]
        completed = [index for index, call in enumerate(tool_calls)
                     if call.get("tool") == "job"
                     and isinstance(call.get("job"), Mapping)
                     and call["job"].get("job_id") == required_job_id
                     and call["job"].get("status") == "completed"]
        if not any(submit < running_at < completed_at
                   and any(tool_calls[i].get("tool") in productive_tools
                           for i in range(running_at + 1, completed_at))
                   and any(tool_calls[i].get("tool") in productive_tools
                           for i in range(completed_at + 1, len(tool_calls)))
                   for submit in submitted
                   for running_at in running
                   for completed_at in completed):
            return False
    continuation_valid = False
    for index, call in enumerate(tool_calls):
        job = call.get("job")
        if (call.get("tool") != "job" or not isinstance(job, Mapping)
                or job.get("status") != "completed"
                or (required_job_id is not None and job.get("job_id") != required_job_id)
                or job.get("job_id") not in completed_job_ids):
            continue
        if any(later.get("tool") in {"write", "edit", "apply_patch", "bash", "test"}
               for later in tool_calls[index + 1:]):
            continuation_valid = True
            break
    if not continuation_valid:
        return False
    return (not require_all_test_jobs_terminal
            or summarize_test_job_lifecycle(tool_calls)["all_terminal_observed"])


def _job_lifecycle(tool_calls: Sequence[Mapping[str, Any]]) -> tuple[bool, bool]:
    """Return (same exact job observed running and terminal, productive overlap)."""
    submissions = [call.get("job") for call in tool_calls
                   if call.get("tool") == "test" and isinstance(call.get("job"), Mapping)]
    productive = {"write", "edit", "apply_patch", "bash", "test"}
    job_ids = {job.get("job_id") for job in submissions if isinstance(job.get("job_id"), str)}
    lifecycle_observed = False
    for job_id in job_ids:
        observations = [(index, call, call.get("job")) for index, call in enumerate(tool_calls)
                        if isinstance(call.get("job"), Mapping)
                        and call["job"].get("job_id") == job_id]
        running = [index for index, _call, job in observations if job.get("status") == "running"]
        terminal = [index for index, _call, job in observations
                    if job.get("status") == "completed"]
        if running and terminal and min(terminal) > min(running):
            lifecycle_observed = True
            overlap = any(call.get("tool") in productive and index > min(running)
                          and index < min(terminal)
                          for index, call in enumerate(tool_calls))
            if overlap:
                return True, True
    return lifecycle_observed, False


def project_result(*, terminal: str, verifier_ran: bool, verifier_exit_code: int | None,
                   duration_ms: int, tool_calls: Sequence[Mapping[str, Any]],
                   measurement_complete: bool, continuation_confirmed: bool,
                   repair_cycles: int, protected: bool = False,
                   tool_call_limit: int | None = None,
                   verifier_state: str | None = None) -> dict[str, Any]:
    """Project verifier outcome and tool-event observation as separate evidence."""
    if protected:
        task_outcome = "inconclusive"
    elif not verifier_ran or verifier_exit_code is None:
        task_outcome = "inconclusive"
    elif verifier_exit_code == 0:
        task_outcome = "passed"
    else:
        task_outcome = "failed"

    verifier_outcome = verifier_state
    if verifier_outcome not in {
        "passed", "setup_error", "empty_suite", "assertion_failures", "test_errors",
        "mixed_test_failures", "timeout", "runner_error", "unclassified_failure",
    }:
        if verifier_ran and verifier_exit_code == 0:
            verifier_outcome = "passed"
        elif verifier_ran and verifier_exit_code is not None:
            verifier_outcome = "failed"
        else:
            verifier_outcome = "not_run"

    activity = sorted(
        value for call in tool_calls
        for value in (call.get("started_elapsed_ms"), call.get("finished_elapsed_ms"))
        if isinstance(value, int) and not isinstance(value, bool)
    )
    span = activity[-1] - activity[0] if len(activity) > 1 else 0
    gap = max((later - earlier for earlier, later in zip(activity, activity[1:])), default=0)
    lifecycle, overlap = _job_lifecycle(tool_calls)
    test_job_lifecycle = summarize_test_job_lifecycle(tool_calls)
    tool_call_count = len(tool_calls)
    tool_call_cap_exceeded = (tool_call_limit is not None
                              and tool_call_count > tool_call_limit)

    reason = None
    if not measurement_complete:
        reason = "measurement_incomplete"
    elif task_outcome == "failed":
        reason = "task_not_passed"
    elif task_outcome == "inconclusive":
        reason = {
            "setup_error": "verifier_setup_error",
            "timeout": "verifier_timeout",
            "runner_error": "verifier_runner_error",
        }.get(verifier_outcome, "verifier_not_run")
    elif tool_call_cap_exceeded:
        reason = "tool_call_cap_exceeded"
    elif task_outcome == "passed" and span < H007_MIN_ACTIVE_DURATION_MS:
        reason = "task_completed_before_threshold"
    elif not lifecycle or not overlap:
        reason = "job_lifecycle_missing"
    elif not continuation_confirmed:
        reason = "continuation_missing"
    elif repair_cycles < 2:
        reason = "repair_cycles_insufficient"
    elif terminal != "completed":
        reason = "agent_terminal_incomplete"
    elif gap > H007_MAX_ACTIVITY_GAP_MS:
        reason = "activity_gap_exceeded"
    elif span < H007_MIN_ACTIVE_DURATION_MS:
        reason = ("task_completed_before_threshold" if task_outcome == "passed"
                  else "activity_span_short")

    return {
        "result_schema_version": H007_RESULT_SCHEMA_VERSION,
        "task_outcome": task_outcome,
        "verifier_outcome": verifier_outcome,
        "horizon_outcome": "qualified" if reason is None else
                           "not_observed" if reason == "task_completed_before_threshold"
                           else "not_qualified",
        "horizon_reason": reason,
        "horizon_evidence": {
            "agent_terminal": terminal,
            "active_work_span_ms": span,
            "maximum_activity_gap_ms": gap,
            "minimum_active_duration_ms": H007_MIN_ACTIVE_DURATION_MS,
            "maximum_activity_gap_limit_ms": H007_MAX_ACTIVITY_GAP_MS,
            "exact_job_running_and_terminal": lifecycle,
            "productive_work_during_job": overlap,
            "continuation_confirmed": continuation_confirmed,
            "repair_cycles": repair_cycles,
            "measurement_complete": measurement_complete,
            "event_measurement_complete": measurement_complete,
            "tool_call_count": tool_call_count,
            "tool_call_limit": tool_call_limit,
            "tool_call_cap_exceeded": tool_call_cap_exceeded,
            "test_job_lifecycle": test_job_lifecycle,
        },
    }


def _public_projection_state(values: Sequence[Any]) -> str:
    if any(value is False for value in values):
        return "incomplete"
    if values and all(value is True for value in values):
        return "complete"
    return "unavailable"


def _verifier_public_reconciliation(
        verifier_diagnostic: Mapping[str, Any] | None,
        continuation: Mapping[str, Any],
        horizon_evidence: Mapping[str, Any] | None,
        job_sequence_diagnostic: Mapping[str, Any] | None,
        serialized_transcript_diagnostic: Mapping[str, Any] | None = None) -> dict[str, Any]:
    """Compare coarse verifier failure domains with public evidence, without messages."""
    domain_counts = (
        verifier_diagnostic.get("failure_domain_counts", {})
        if isinstance(verifier_diagnostic, Mapping) else {}
    )
    if not isinstance(domain_counts, Mapping):
        domain_counts = {}

    repair = continuation.get("repair_job_diagnostic")
    if not isinstance(repair, Mapping):
        repair = {}
    lifecycle = continuation.get("test_job_lifecycle")
    if not isinstance(lifecycle, Mapping):
        lifecycle = {}
    horizon = horizon_evidence if isinstance(horizon_evidence, Mapping) else {}
    job_sequence = (job_sequence_diagnostic
                    if isinstance(job_sequence_diagnostic, Mapping) else {})
    success_jobs = continuation.get("required_success_jobs")
    if not isinstance(success_jobs, Mapping):
        success_jobs = {}
    required_success_jobs_complete = (
        bool(success_jobs)
        and all(isinstance(job, Mapping) and job.get("completed") is True
                for job in success_jobs.values())
    )

    def empty_list(field: str) -> bool | None:
        value = continuation.get(field)
        return not value if isinstance(value, list) else None

    continuation_checks = {
        "document_present": continuation.get("present"),
        "document_valid": continuation.get("document_valid"),
        "phase_order_matches": continuation.get("phase_order_matches"),
        "contract_phase_order_matches": continuation.get("contract_phase_order_matches"),
        "stage_order_matches": continuation.get("stage_order_matches"),
        "next_phase_none": (continuation.get("next_phase") == "none"
                             if continuation.get("next_phase") is not None else None),
        "next_contract_phase_none": (continuation.get("next_contract_phase") == "none"
                                      if continuation.get("next_contract_phase") is not None
                                      else None),
        "next_stage_none": (continuation.get("next_stage") == "none"
                            if continuation.get("next_stage") is not None else None),
        "next_gate_none": (continuation.get("next_gate") == "none"
                           if continuation.get("next_gate") is not None else None),
        "incomplete_stages_empty": empty_list("incomplete_stages"),
        "incomplete_gates_empty": empty_list("incomplete_gates"),
        "missing_phases_empty": empty_list("missing_phases"),
        "missing_contract_phases_empty": empty_list("missing_contract_phases"),
        "required_success_jobs_complete": required_success_jobs_complete,
        "incomplete_required_success_jobs_empty": empty_list(
            "incomplete_required_success_jobs"),
        "public_artifacts_present": continuation.get("all_public_artifacts_present"),
        "progress_sections_ordered": (
            continuation.get("line_layout", {}).get("progress_sections_ordered")
            if isinstance(continuation.get("line_layout"), Mapping) else None
        ),
        "pointer_fields_follow_progress_sections": (
            continuation.get("line_layout", {}).get(
                "pointer_fields_follow_progress_sections")
            if isinstance(continuation.get("line_layout"), Mapping) else None
        ),
        "pointer_fields_in_expected_order": (
            continuation.get("line_layout", {}).get("pointer_fields_in_expected_order")
            if isinstance(continuation.get("line_layout"), Mapping) else None
        ),
    }
    unbound_count = job_sequence.get("unbound_job_tool_call_count")
    unbound_state_counts = job_sequence.get("unbound_call_state_counts")
    if not isinstance(unbound_state_counts, Mapping):
        unbound_state_counts = {}
    failed_submit_error_class_counts = job_sequence.get(
        "failed_test_submit_error_class_counts"
    )
    if not isinstance(failed_submit_error_class_counts, Mapping):
        failed_submit_error_class_counts = {}
    failed_job_status_error_class_counts = job_sequence.get(
        "failed_job_status_error_class_counts"
    )
    if not isinstance(failed_job_status_error_class_counts, Mapping):
        failed_job_status_error_class_counts = {}
    id_correlation = job_sequence.get("job_id_request_result_correlation")
    if not isinstance(id_correlation, Mapping):
        id_correlation = {}
    correlation_state_counts = id_correlation.get("state_counts")
    if not isinstance(correlation_state_counts, Mapping):
        correlation_state_counts = {}
    safe_correlation_state_counts = {
        state: count for state, count in correlation_state_counts.items()
        if state in {"matched", "mismatched", "unobserved", "not_applicable"}
        and isinstance(count, int) and not isinstance(count, bool)
        and 0 < count <= MAX_PUBLIC_JOB_CORRELATION_CALLS
    }
    correlation_applicable_count = id_correlation.get("applicable_call_count")
    if (not isinstance(correlation_applicable_count, int)
            or isinstance(correlation_applicable_count, bool)
            or not 0 <= correlation_applicable_count <= MAX_PUBLIC_JOB_CORRELATION_CALLS):
        correlation_applicable_count = None
    job_checks = {
        "repair_pairs_complete": repair.get("complete"),
        "repair_count_matches": repair.get("repair_cycle_count_matches"),
        "minimum_repair_cycles_observed": (
            repair.get("observed_repair_cycle_count", 0) >= 2
            if isinstance(repair.get("observed_repair_cycle_count"), int) else None),
        "test_jobs_terminal": lifecycle.get("all_terminal_observed"),
        "required_review_observed": continuation.get("required_review_observed"),
        "release_overlap_confirmed": continuation.get("release_overlap_confirmed"),
        "exact_job_running_and_terminal": horizon.get("exact_job_running_and_terminal"),
        "productive_work_during_job": horizon.get("productive_work_during_job"),
        "event_measurement_complete": horizon.get("event_measurement_complete"),
        "job_tool_calls_bound_to_id": (
            unbound_count == 0 if isinstance(unbound_count, int) else None),
    }
    if id_correlation.get("instrumentation_observed") is True:
        job_checks["job_request_result_ids_match"] = (
            id_correlation.get("all_applicable_calls_match") is True
        )
    if isinstance(serialized_transcript_diagnostic, Mapping):
        job_checks["serialized_transcript_projection_matches"] = (
            serialized_transcript_diagnostic.get("lifecycle_projection_matches_source")
            is True
        )
    public_checks = {
        "continuation_boundary": continuation_checks,
        "transcript_job_lifecycle": job_checks,
    }
    public_states = {
        "continuation_boundary": _public_projection_state(tuple(continuation_checks.values())),
        "transcript_job_lifecycle": _public_projection_state(tuple(job_checks.values())),
    }
    reconciliation: dict[str, str] = {}
    for domain, count in domain_counts.items():
        if not isinstance(count, int) or count <= 0:
            continue
        state = public_states.get(domain)
        if state == "complete":
            reconciliation[domain] = "verifier_public_projection_disagreement"
        elif state == "incomplete":
            reconciliation[domain] = "public_evidence_incomplete"
        elif state == "unavailable":
            reconciliation[domain] = "public_evidence_unavailable"
        else:
            reconciliation[domain] = "unmapped_failure_domain"

    return {
        "schema": "h007_verifier_public_reconciliation_v1",
        "failure_domain_counts": {
            str(domain): count for domain, count in domain_counts.items()
            if isinstance(domain, str) and isinstance(count, int) and count > 0
        },
        "public_domain_states": {
            domain: public_states[domain] for domain in reconciliation
            if domain in public_states
        },
        "public_checks": {
            domain: public_checks[domain] for domain in reconciliation
            if domain in public_checks
        },
        "unbound_job_tool_call_count": (
            unbound_count if isinstance(unbound_count, int) and unbound_count >= 0 else None),
        "unbound_call_state_counts": {
            key: count for key, count in unbound_state_counts.items()
            if isinstance(key, str) and len(key) <= 80
            and isinstance(count, int) and not isinstance(count, bool) and count > 0
        },
        "job_id_request_result_correlation": {
            "instrumentation_observed": (
                id_correlation.get("instrumentation_observed") is True
            ),
            "applicable_call_count": correlation_applicable_count,
            "state_counts": safe_correlation_state_counts,
        },
        "failed_test_submit_error_class_counts": {
            key: count for key, count in failed_submit_error_class_counts.items()
            if key in SAFE_TEST_SUBMIT_ERROR_CLASSES
            and isinstance(count, int) and not isinstance(count, bool) and count > 0
        },
        "failed_job_status_error_class_counts": {
            key: count for key, count in failed_job_status_error_class_counts.items()
            if key in SAFE_JOB_STATUS_ERROR_CLASSES
            and isinstance(count, int) and not isinstance(count, bool)
            and 0 < count <= MAX_PUBLIC_JOB_CORRELATION_CALLS
        },
        "classification": reconciliation,
        "assertion_text_retained": False,
        "raw_output_retained": False,
        "raw_job_ids_retained": False,
        "transcript_retained": False,
    }


def summarize_task_completion(*, terminal: str, task_outcome: str,
                              verifier_outcome: str,
                              continuation: Mapping[str, Any],
                              verifier_diagnostic: Mapping[str, Any] | None = None,
                              horizon_evidence: Mapping[str, Any] | None = None,
                              job_sequence_diagnostic: Mapping[str, Any] | None = None,
                              serialized_transcript_diagnostic: Mapping[str, Any] | None = None
                              ) -> dict[str, Any]:
    """Separate explicit task incompletion from verifier and event-measurement failures.

    The continuation diagnostic contains only allowlisted phase, stage, gate, job,
    review, and artifact state. This summary deliberately retains neither its note
    body nor raw job identities.
    """
    reasons: list[str] = []
    if terminal != "completed":
        reasons.append("agent_terminal_incomplete")
    if not continuation.get("present"):
        reasons.append("continuation_missing")
    if continuation.get("document_valid") is not True:
        reasons.append("continuation_not_confirmed")
    for field, reason in (
        ("incomplete_stages", "required_stages_incomplete"),
        ("incomplete_gates", "required_gates_incomplete"),
        ("incomplete_required_success_jobs", "required_success_jobs_incomplete"),
    ):
        value = continuation.get(field)
        if isinstance(value, list) and value:
            reasons.append(reason)
    lifecycle = continuation.get("test_job_lifecycle")
    if isinstance(lifecycle, Mapping) and lifecycle.get("unobserved_count", 0) > 0:
        reasons.append("test_jobs_nonterminal")
    if continuation.get("required_review_observed") is False:
        reasons.append("required_review_unobserved")
    if continuation.get("all_public_artifacts_present") is False:
        reasons.append("required_artifacts_missing")

    if task_outcome == "passed" and not reasons:
        state = "verified_complete"
    elif terminal != "completed":
        state = "agent_incomplete"
    elif reasons:
        state = "incomplete"
    elif task_outcome == "failed":
        state = "verification_failed"
    elif verifier_outcome in {"setup_error", "timeout", "runner_error", "empty_suite",
                              "not_run", "unclassified_failure"}:
        state = "verifier_unavailable"
    else:
        state = "not_demonstrated"

    if task_outcome == "failed" and verifier_outcome in {
            "assertion_failures", "test_errors", "mixed_test_failures"}:
        reasons.append("independent_verifier_failures")

    return {
        "schema": "h007_task_completion_diagnostic_v2",
        "state": state,
        "reasons": reasons,
        "verifier_outcome": verifier_outcome,
        "verifier_public_reconciliation": _verifier_public_reconciliation(
            verifier_diagnostic, continuation, horizon_evidence, job_sequence_diagnostic,
            serialized_transcript_diagnostic),
        "raw_note_retained": False,
        "raw_job_ids_retained": False,
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

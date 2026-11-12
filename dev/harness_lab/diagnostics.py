"""Bounded operator diagnostics for H007 verifier and reviewer outcomes.

The returned data intentionally excludes verifier assertion text, tracebacks,
provider payloads, task arguments, task result bodies, and transcripts. These
summaries are for post-run operator diagnosis and must stay outside the agent
workspace and prompts.
"""
from __future__ import annotations

import ast
import hashlib
import json
import re
import sqlite3
from collections import Counter
from pathlib import Path
from typing import Any, Mapping, Sequence
from urllib.parse import quote

VERIFIER_SCHEMA = "h007_verifier_diagnostic_v4"
REVIEWER_SCHEMA = "h007_reviewer_lifecycle_diagnostic_v2"
PROVIDER_ATTEMPT_SCHEMA = "h007_provider_attempt_diagnostic_v1"
MAX_DIAGNOSTIC_TESTS = 100
MAX_TEST_ID_CHARS = 240
MAX_REVIEW_EVENTS = 200
MAX_REVIEWER_DATABASE_BYTES = 256 * 1024 * 1024
MAX_REVIEWER_DATABASE_ROWS = 100_000
MAX_VERIFIER_OUTPUT_CHARS = 2_000_000
MAX_PROVIDER_LOG_BYTES = 16 * 1024 * 1024
MAX_PROVIDER_LOG_LINE_BYTES = 32 * 1024
MAX_PROVIDER_FAILURES = 64
MAX_JOB_SEQUENCE_EVENTS = 1_600
MAX_JOB_ELAPSED_MS = 3_600_000
MAX_SERIALIZED_TRANSCRIPT_BYTES = 8 * 1024 * 1024
_ACTIVITY_TOOLS = frozenset({
    "read", "list", "glob", "bash", "write", "edit", "apply_patch", "test", "job",
    "task", "tool_search", "python_script", "context_read", "other",
})
_PRODUCTIVE_ACTIVITY_TOOLS = frozenset({"write", "edit", "apply_patch", "bash", "test"})

_VERBOSE_RESULT = re.compile(r"^(?P<test>.+?) \.\.\. (?P<result>ok|FAIL|ERROR|skipped)(?: .*)?$")
_FAILURE_HEADING = re.compile(r"^(?P<kind>FAIL|ERROR): (?P<test>.+)$")
_EXCEPTION_LINE = re.compile(r"^(?P<class>[A-Z][A-Za-z0-9_.]{0,127}):")
_ASSERTION_CALL = re.compile(r"^(?:self\.)?(?P<method>assert[A-Z][A-Za-z0-9_]*|fail)\s*\(")
_SAFE_FAILURE_TERMS = frozenset({
    "actual", "after", "all", "assertion", "before", "completed", "contains",
    "continuation", "cycle", "equal", "event", "expected", "failed", "false",
    "format", "gate", "get", "got", "id", "index", "invalid", "job", "key",
    "line", "list", "missing", "next", "none", "not", "observed", "order",
    "output", "overlap", "parse", "phase", "present", "record", "repair", "result",
    "running", "same", "sequence", "state", "status", "task", "terminal", "test",
    "transcript", "true", "unpack", "value", "workspace",
})
_SAFE_MESSAGE_WORDS = _SAFE_FAILURE_TERMS | frozenset({
    "a", "an", "and", "are", "argument", "arguments", "as", "at", "attribute",
    "base", "be", "been", "but", "by", "cannot", "case", "check", "choice", "code",
    "convert", "could", "count", "default", "did", "does", "each", "empty", "enough",
    "error", "exactly", "few", "file", "find", "float", "for", "found", "from", "has",
    "have", "in", "int", "integer", "is", "it", "items", "length", "literal", "many",
    "mapping", "marker", "must", "no", "number", "numbers", "of", "one", "only", "or",
    "path", "range", "required", "sequence", "set", "should", "size", "split", "string",
    "the", "there", "this", "to", "too", "tuple", "type", "valid", "values", "was",
    "were", "with", "without", "zero",
})
_RAN_LINE = re.compile(r"^Ran (?P<count>\d+) tests?\b")
_COUNT_FIELD = re.compile(r"\b(?P<name>failures|errors)=(?P<count>\d+)\b")
_SAFE_FAILURE_CLASSES = frozenset({
    "provider_authentication", "provider_rate_limit", "provider_transient",
    "deadline_exceeded", "budget_exhausted", "tool_permission_denied",
    "cancelled", "unclassified", "existing_worktree_unavailable",
    "existing_worktree_authority_unavailable",
})
_SAFE_EXCEPTION_CLASSES = frozenset({
    "AssertionError", "AttributeError", "IndexError", "KeyError", "LookupError",
    "NameError", "NotImplementedError", "RuntimeError", "TypeError", "ValueError",
})
_RUN_STATUSES = frozenset({
    "created", "queued", "preparing", "running", "waiting", "cancelling",
    "completed", "failed", "interrupted", "cancelled", "timed_out",
})
_TERMINAL_STATUSES = frozenset({"completed", "failed", "interrupted", "cancelled", "timed_out"})
_TASK_ACTIONS = frozenset({"spawn", "status", "wait", "get", "continue", "cancel"})
_TEST_EXECUTION_STATES = frozenset({
    "passed", "assertion_failures", "test_errors", "mixed_test_failures",
})
SAFE_TEST_SUBMIT_ERROR_CLASSES = frozenset({
    "tool_not_found", "tool_execution_failed", "tool_timeout", "permission_denied",
    "tool_formatting_failed", "tool_disabled", "missing_workspace_root",
    "shared_process_denied", "execution_backend_unavailable", "io_error",
    "network_error", "unclassified",
})
_JOB_ID = re.compile(
    r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", re.I
)
_PATTERN_MARKER_TOKENS = {
    "continuation_boundary": (
        "continuation", "next_phase", "next_stage", "next_contract_phase", "checkpoint",
    ),
    "job_lifecycle": ("job_id", "job_state", "job_status", "running", "terminal"),
    "repair_sequence": ("repair_cycles", "repair_job", "seed_job", "repair"),
    "reviewer_lifecycle": ("review_status", "reviewer", "task_id", "run_id"),
    "workspace_qualification": (
        "workspace_qualification", "workspace_tests", "clippy", "cargo test",
    ),
    "public_artifact": ("public_artifact", "acceptance.json", "artifact", "summary.json"),
}
_JOB_ACTIONS = frozenset({"submit", "status", "wait", "output", "cancel", "unknown"})
_JOB_STATUSES = frozenset({
    "queued", "running", "completed", "failed", "cancelled", "timed_out", "unknown",
})
_TOOL_STATUSES = frozenset({"completed", "failed", "cancelled", "unknown"})
_PROVIDER_LOG_MARKERS = (
    ("provider_stream_setup_started", "setup_started"),
    ("provider_stream_setup_completed", "setup_completed"),
    ("provider_stream_setup_timeout", "setup_timeout"),
    ("provider_stream_setup_failed", "setup_failed"),
    ("provider_stream_idle_timeout", "stream_idle_timeout"),
    ("provider_stream_eof_consumed", "stream_eof"),
    ("provider_turn_event_received", "response_event"),
    ("provider attempt succeeded", "attempt_succeeded"),
    ("provider attempt failed before visible output; retrying", "retry_scheduled"),
    ("provider retry chain deadline reached; stopping", "retry_chain_deadline"),
    ("provider attempt failed terminally", "attempt_failed_terminally"),
    ("provider attempt interrupted after visible output; superseded without replay",
     "attempt_superseded"),
)
_PROVIDER_LOG_FIELD = re.compile(
    r'\b(?P<name>[a-z][a-z0-9_]*)=(?:"(?P<quoted>[^"]{0,256})"|(?P<bare>[^\s,]+))'
)
_PROVIDER_SAFE_ERROR_CLASS = re.compile(r"^[a-z][a-z0-9_]{0,63}$")
_PROVIDER_SAFE_FIELD_NAMES = frozenset({
    "attempt_id", "attempt_index", "error_class", "retryable", "attempts_left",
    "attempts_consumed", "remaining_ms", "decoded_event_count", "elapsed_ms",
    "elapsed_since_setup_ms", "since_last_decoded_event_ms", "idle_wait_ms",
    "setup_elapsed_ms", "stage", "event_kind",
})
_PROVIDER_INTEGER_FIELDS = frozenset({
    "attempt_index", "attempts_left", "attempts_consumed", "decoded_event_count",
    "remaining_ms", "elapsed_ms", "elapsed_since_setup_ms", "since_last_decoded_event_ms",
    "idle_wait_ms", "setup_elapsed_ms",
})
_PROVIDER_PHASES = frozenset({"response_headers"})
_PROVIDER_EVENT_KINDS = frozenset({
    "text_delta", "reasoning_delta", "tool_call", "finish", "unknown",
})


def _bounded_test_id(value: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9_.:/() \-\[\]]", "?", value.strip())
    return cleaned[:MAX_TEST_ID_CHARS]


def _verifier_failure_domain(test_id: str) -> str:
    """Map a bounded test identifier to a small operator-facing category."""
    normalized = _bounded_test_id(test_id).lower()
    if "continuation" in normalized and any(
            term in normalized for term in ("phase", "boundary", "checkpoint")):
        return "continuation_boundary"
    if "transcript" in normalized and any(
            term in normalized for term in ("job", "repair", "lifecycle", "overlap")):
        return "transcript_job_lifecycle"
    if "job" in normalized or "repair" in normalized:
        return "job_lifecycle"
    return "task_behavior"


def _safe_failure_message(message: str, exception_class: str) -> dict[str, Any]:
    """Return only a bounded diagnostic category and allowlisted message terms."""
    normalized = message[:4096].lower()
    if exception_class == "ValueError":
        if "invalid literal for int()" in normalized:
            kind = "invalid_integer_literal"
        elif "invalid literal for float()" in normalized:
            kind = "invalid_float_literal"
        elif "not enough values to unpack" in normalized:
            kind = "unpack_too_few"
        elif "too many values to unpack" in normalized:
            kind = "unpack_too_many"
        elif "could not convert" in normalized:
            kind = "conversion_failure"
        else:
            kind = "value_error_message"
    elif exception_class == "AssertionError":
        if "expected" in normalized and ("got" in normalized or "actual" in normalized):
            kind = "expected_actual_mismatch"
        elif "expected" in normalized and "missing" in normalized:
            kind = "expected_value_missing"
        elif "!=" in normalized or "not equal" in normalized:
            kind = "comparison_mismatch"
        else:
            kind = "assertion_message"
    else:
        kind = "exception_message"
    terms = sorted(set(re.findall(r"[a-z_]+", normalized)) & _SAFE_FAILURE_TERMS)
    tokens = re.findall(r"[A-Za-z_]+|\d+|[^\w\s]", message[:4096])
    shape = []
    for token in tokens:
        if token.isdigit():
            shape.append("<number>")
        elif token.isalpha() or "_" in token:
            shape.append(token.lower() if token.lower() in _SAFE_MESSAGE_WORDS else "<word>")
        elif token in ":,.;()[]{}+-!=<>/'\"`":
            shape.append(token)
        else:
            shape.append("<symbol>")
    result = {"kind": kind, "terms": terms[:32], "shape": shape[:48],
              "shape_truncated": len(shape) > 48}
    if exception_class == "AssertionError":
        signature = _summarize_regex_mismatch(message[:4096])
        if signature is not None:
            result["pattern_signature"] = signature
    return result


def _summarize_regex_mismatch(message: str) -> dict[str, Any] | None:
    """Classify unittest's regex mismatch without retaining either string."""
    forms = (
        ("Regex didn't match: ", " not found in ", "regex_not_found"),
        ("Regex matched: ", " matches ", "regex_matched"),
    )
    for prefix, delimiter, matcher_kind in forms:
        if not message.startswith(prefix):
            continue
        payload = message[len(prefix):]
        # Search possible separators because the rendered values may themselves
        # contain the separator text. Only accept a pair of valid Python string
        # literals and discard both immediately after classification.
        start = 0
        while True:
            split_at = payload.find(delimiter, start)
            if split_at < 0:
                break
            try:
                expected = ast.literal_eval(payload[:split_at])
                observed = ast.literal_eval(payload[split_at + len(delimiter):])
            except (SyntaxError, ValueError, TypeError, MemoryError, RecursionError):
                start = split_at + len(delimiter)
                continue
            if isinstance(expected, str) and isinstance(observed, str):
                return {
                    "state": "classified",
                    "matcher_kind": matcher_kind,
                    "expected_length": min(len(expected), 1_000_000),
                    "observed_length": min(len(observed), 1_000_000),
                    "expected_marker_groups": _pattern_marker_groups(expected),
                    "observed_marker_groups": _pattern_marker_groups(observed),
                    "expected_text_retained": False,
                    "observed_text_retained": False,
                }
            start = split_at + len(delimiter)
        return {
            "state": "unclassified",
            "matcher_kind": matcher_kind,
            "expected_length": None,
            "observed_length": None,
            "expected_marker_groups": [],
            "observed_marker_groups": [],
            "expected_text_retained": False,
            "observed_text_retained": False,
        }
    return None


def _pattern_marker_groups(value: str) -> list[str]:
    normalized = value.lower()
    return [group for group, tokens in _PATTERN_MARKER_TOKENS.items()
            if any(token in normalized for token in tokens)]


def summarize_job_tool_sequence(
    tool_calls: Sequence[Mapping[str, Any]],
) -> dict[str, Any]:
    """Return an ordered, body-free job/test sequence with run-local aliases."""
    aliases: dict[str, str] = {}
    events: list[dict[str, Any]] = []
    unbound_calls = 0
    unbound_states: Counter[str] = Counter()
    failed_submit_error_classes: Counter[str] = Counter()
    id_correlation_states: Counter[str] = Counter()
    id_correlation_instrumented = False
    total = 0
    for call in tool_calls:
        tool = call.get("tool")
        if tool not in {"job", "test"}:
            continue
        total += 1
        job = call.get("job")
        job = job if isinstance(job, Mapping) else {}
        raw_id = job.get("job_id")
        valid_id = isinstance(raw_id, str) and _JOB_ID.fullmatch(raw_id) is not None
        if valid_id:
            alias = aliases.setdefault(raw_id.lower(), f"J{len(aliases) + 1:02d}")
        else:
            alias = None
            unbound_calls += 1
        action = job.get("action")
        action = action if isinstance(action, str) and action in _JOB_ACTIONS else "unknown"
        job_status = job.get("status")
        if job_status == "canceled":
            job_status = "cancelled"
        job_status = (job_status if isinstance(job_status, str)
                      and job_status in _JOB_STATUSES else "unknown")
        call_status = call.get("status")
        call_status = (call_status if isinstance(call_status, str)
                       and call_status in _TOOL_STATUSES else "unknown")
        tool_error_class: str | None = None
        if tool == "test" and action == "submit" and call_status == "failed":
            candidate_class = call.get("tool_error_class")
            tool_error_class = (
                candidate_class if isinstance(candidate_class, str)
                and candidate_class in SAFE_TEST_SUBMIT_ERROR_CLASSES else "unobserved"
            )
            failed_submit_error_classes[tool_error_class] += 1
        argument_state = call.get("job_id_argument_state")
        argument_state = (argument_state if isinstance(argument_state, str) and argument_state in {
            "invalid_json", "not_object", "missing", "non_string", "empty_string", "string",
        } else "unobserved")
        id_correlation_instrumented = (
            id_correlation_instrumented
            or "job_id_argument_matches_result" in call
        )
        correlation_state = call.get("job_id_argument_matches_result")
        correlation_state = (
            correlation_state if isinstance(correlation_state, str)
            and correlation_state in {"matched", "mismatched", "unobserved", "not_applicable"}
            else "unobserved" if action in {"status", "wait", "output", "cancel"}
            else "not_applicable"
        )
        if correlation_state != "not_applicable":
            id_correlation_states[correlation_state] += 1
        if alias is None:
            unbound_states[f"{action}:{argument_state}:{call_status}"] += 1
        index = call.get("index")
        started = call.get("started_elapsed_ms")
        finished = call.get("finished_elapsed_ms")
        events.append({
            "index": index if isinstance(index, int) and not isinstance(index, bool)
            and 0 <= index <= 1_000_000 else None,
            "tool": tool,
            "job_alias": alias,
            "job_id_present": valid_id,
            "job_id_argument_state": argument_state,
            "job_id_argument_matches_result": correlation_state,
            "action": action,
            "job_status": job_status,
            "call_status": call_status,
            "tool_error_class": tool_error_class,
            "started_elapsed_ms": started if isinstance(started, int)
            and not isinstance(started, bool) and 0 <= started <= MAX_JOB_ELAPSED_MS else None,
            "finished_elapsed_ms": finished if isinstance(finished, int)
            and not isinstance(finished, bool) and 0 <= finished <= MAX_JOB_ELAPSED_MS else None,
        })
    events.sort(key=lambda event: (event["index"] is None, event["index"] or 0))
    truncated = len(events) > MAX_JOB_SEQUENCE_EVENTS
    return {
        "schema": "h007_job_tool_sequence_v1",
        "state": "observed" if total else "no_job_calls",
        "events": events[:MAX_JOB_SEQUENCE_EVENTS],
        "job_tool_call_count": total,
        "unbound_job_tool_call_count": unbound_calls,
        "unbound_call_state_counts": dict(sorted(unbound_states.items())),
        "failed_test_submit_error_class_counts": dict(
            sorted(failed_submit_error_classes.items())
        ),
        "job_id_request_result_correlation": {
            "applicable_call_count": sum(id_correlation_states.values()),
            "state_counts": dict(sorted(id_correlation_states.items())),
            "all_applicable_calls_match": (
                bool(id_correlation_states)
                and id_correlation_states.get("matched", 0) == sum(id_correlation_states.values())
            ),
            "instrumentation_observed": id_correlation_instrumented,
            "raw_job_ids_retained": False,
            "job_id_digests_retained": False,
        },
        "sequence_truncated": truncated,
        "raw_job_ids_retained": False,
        "tool_arguments_retained": False,
        "tool_outputs_retained": False,
    }


def audit_serialized_transcript(
    transcript_path: Path,
    expected_tool_calls: Sequence[Mapping[str, Any]],
    *,
    expected_case_id: str,
    expected_schema_version: int,
) -> dict[str, Any]:
    """Compare the verifier's persisted transcript with its safe source projection.

    The exact file is parsed in memory, but only bounded counts and redacted,
    run-local event projections are returned. Raw transcript bytes, job IDs,
    arguments, and outputs are never retained in this result.
    """
    base = {
        "schema": "h007_serialized_transcript_audit_v1",
        "raw_transcript_retained": False,
        "raw_job_ids_retained": False,
        "tool_arguments_retained": False,
        "tool_outputs_retained": False,
        "job_id_digests_retained": False,
    }
    try:
        size = transcript_path.stat().st_size
    except OSError as error:
        return {
            **base,
            "state": "unavailable",
            "failure_class": type(error).__name__,
            "serialized_bytes": None,
        }
    if size < 0 or size > MAX_SERIALIZED_TRANSCRIPT_BYTES:
        return {
            **base,
            "state": "oversized",
            "failure_class": None,
            "serialized_bytes": min(max(size, 0), MAX_SERIALIZED_TRANSCRIPT_BYTES + 1),
        }
    try:
        with transcript_path.open("rb") as source:
            encoded = source.read(MAX_SERIALIZED_TRANSCRIPT_BYTES + 1)
    except OSError as error:
        return {
            **base,
            "state": "unavailable",
            "failure_class": type(error).__name__,
            "serialized_bytes": size,
        }
    if len(encoded) > MAX_SERIALIZED_TRANSCRIPT_BYTES:
        return {
            **base,
            "state": "oversized",
            "failure_class": None,
            "serialized_bytes": MAX_SERIALIZED_TRANSCRIPT_BYTES + 1,
        }
    try:
        parsed = json.loads(encoded)
    except (UnicodeDecodeError, json.JSONDecodeError):
        return {
            **base,
            "state": "invalid_json",
            "failure_class": None,
            "serialized_bytes": len(encoded),
        }
    if (not isinstance(parsed, Mapping) or not isinstance(parsed.get("tool_calls"), list)
            or any(not isinstance(call, Mapping) for call in parsed["tool_calls"])):
        return {
            **base,
            "state": "invalid_shape",
            "failure_class": None,
            "serialized_bytes": len(encoded),
        }

    serialized_calls = parsed["tool_calls"]
    source_jobs = summarize_job_tool_sequence(expected_tool_calls)
    serialized_jobs = summarize_job_tool_sequence(serialized_calls)
    source_activity = summarize_ordered_tool_activity(expected_tool_calls)
    serialized_activity = summarize_ordered_tool_activity(serialized_calls)
    case_matches = parsed.get("case_id") == expected_case_id
    schema_matches = parsed.get("schema_version") == expected_schema_version
    tool_call_count_matches = len(serialized_calls) == len(expected_tool_calls)
    job_sequence_matches = serialized_jobs == source_jobs
    ordered_activity_matches = serialized_activity == source_activity
    return {
        **base,
        "state": "observed",
        "failure_class": None,
        "serialized_bytes": len(encoded),
        "case_id_matches": case_matches,
        "schema_version_matches": schema_matches,
        "tool_call_count": len(serialized_calls),
        "tool_call_count_matches": tool_call_count_matches,
        "job_tool_call_count": serialized_jobs["job_tool_call_count"],
        "job_sequence_matches_source": job_sequence_matches,
        "ordered_activity_matches_source": ordered_activity_matches,
        "lifecycle_projection_matches_source": (
            case_matches and schema_matches and tool_call_count_matches
            and job_sequence_matches and ordered_activity_matches
        ),
        "job_id_request_result_correlation": serialized_jobs[
            "job_id_request_result_correlation"
        ],
    }


def summarize_ordered_tool_activity(
    tool_calls: Sequence[Mapping[str, Any]],
) -> dict[str, Any]:
    """Project ordered body-free tool activity and exact-job overlap evidence.

    Job identities are correlated in memory and emitted only as run-local aliases.
    Tool arguments, outputs, paths, task identities, and result bodies are omitted.
    """
    aliases: dict[str, str] = {}

    def alias_for(call: Mapping[str, Any]) -> str | None:
        job = call.get("job")
        raw_id = job.get("job_id") if isinstance(job, Mapping) else None
        if not isinstance(raw_id, str) or _JOB_ID.fullmatch(raw_id) is None:
            return None
        return aliases.setdefault(raw_id.lower(), f"J{len(aliases) + 1:02d}")

    ordered = sorted(
        enumerate(tool_calls),
        key=lambda pair: (
            pair[1].get("index") is None,
            pair[1].get("index") if isinstance(pair[1].get("index"), int) else pair[0],
            pair[0],
        ),
    )
    events: list[dict[str, Any]] = []
    indexed_calls: list[tuple[int, Mapping[str, Any], str, bool]] = []
    unbound_job_calls = 0
    for ordinal, (source_index, call) in enumerate(ordered, start=1):
        raw_tool = call.get("tool")
        tool = raw_tool if isinstance(raw_tool, str) and raw_tool in _ACTIVITY_TOOLS else "other"
        job = call.get("job")
        job = job if isinstance(job, Mapping) else {}
        job_alias = alias_for(call) if tool in {"test", "job"} else None
        if tool in {"test", "job"} and job_alias is None:
            unbound_job_calls += 1
        if tool in {"test", "job"}:
            action = job.get("action")
            action = action if isinstance(action, str) and action in _JOB_ACTIONS else "unknown"
        elif tool == "task":
            action = call.get("task_action")
            action = action if isinstance(action, str) and action in _TASK_ACTIONS else "unknown"
        else:
            action = "invoke"
        call_status = call.get("status")
        call_status = (call_status if isinstance(call_status, str)
                       and call_status in _TOOL_STATUSES else "unknown")
        productive = (tool in _PRODUCTIVE_ACTIVITY_TOOLS
                      and call_status == "completed" and call.get("failed") is not True)
        index = call.get("index")
        safe_index = (index if isinstance(index, int) and not isinstance(index, bool)
                      and 0 <= index <= 1_000_000 else ordinal)

        def elapsed(field: str) -> int | None:
            value = call.get(field)
            return (value if isinstance(value, int) and not isinstance(value, bool)
                    and 0 <= value <= MAX_JOB_ELAPSED_MS else None)

        event = {
            "index": safe_index,
            "tool_class": tool,
            "action": action,
            "status": call_status,
            "productive": productive,
            "job_alias": job_alias,
            "job_id_argument_state": (
                call.get("job_id_argument_state")
                if tool == "job" and isinstance(call.get("job_id_argument_state"), str)
                and call.get("job_id_argument_state") in {
                    "invalid_json", "not_object", "missing", "non_string",
                    "empty_string", "string",
                } else ("unobserved" if tool == "job" else None)
            ),
            "job_state": (
                job.get("status") if isinstance(job.get("status"), str)
                and job.get("status") in _JOB_STATUSES else "unknown"
            ) if tool in {"test", "job"} else None,
            "started_elapsed_ms": elapsed("started_elapsed_ms"),
            "finished_elapsed_ms": elapsed("finished_elapsed_ms"),
        }
        events.append(event)
        indexed_calls.append((safe_index, call, tool, productive))

    events.sort(key=lambda event: (event["index"], event["tool_class"],
                                   event["action"], event["job_alias"] or ""))
    intervals: list[dict[str, Any]] = []
    job_events: dict[str, list[tuple[int, str]]] = {}
    for index, call, tool, _productive in indexed_calls:
        if tool not in {"test", "job"}:
            continue
        job = call.get("job")
        if not isinstance(job, Mapping):
            continue
        raw_id = job.get("job_id")
        if not isinstance(raw_id, str) or _JOB_ID.fullmatch(raw_id) is None:
            continue
        state = job.get("status")
        if state == "canceled":
            state = "cancelled"
        if not isinstance(state, str) or state not in _JOB_STATUSES:
            continue
        job_events.setdefault(raw_id.lower(), []).append((index, state))

    for raw_id, observations in job_events.items():
        job_alias = aliases[raw_id]
        for position, (running_index, state) in enumerate(observations):
            if state != "running":
                continue
            completed_index = next((index for index, next_state in observations[position + 1:]
                                    if next_state == "completed"), None)
            if completed_index is None:
                continue
            productive_count = sum(
                1 for index, _call, _tool, productive in indexed_calls
                if running_index < index < completed_index and productive
            )
            intervals.append({
                "job_alias": job_alias,
                "running_call_index": running_index,
                "completed_call_index": completed_index,
                "productive_call_count_between": productive_count,
                "productive_overlap": productive_count > 0,
            })
    intervals.sort(key=lambda item: (item["running_call_index"], item["job_alias"]))
    truncated = len(events) > MAX_JOB_SEQUENCE_EVENTS
    return {
        "schema": "h007_ordered_tool_activity_v1",
        "state": "observed" if events else "no_tool_calls",
        "events": events[:MAX_JOB_SEQUENCE_EVENTS],
        "productive_overlap_intervals": intervals[:MAX_JOB_SEQUENCE_EVENTS],
        "tool_call_count": len(events),
        "job_tool_call_count": sum(event["tool_class"] in {"job", "test"}
                                    for event in events),
        "unbound_job_tool_call_count": unbound_job_calls,
        "productive_overlap_interval_count": len(intervals),
        "zero_productive_overlap_interval_count": sum(
            not interval["productive_overlap"] for interval in intervals
        ),
        "sequence_truncated": truncated or len(intervals) > MAX_JOB_SEQUENCE_EVENTS,
        "raw_job_ids_retained": False,
        "tool_arguments_retained": False,
        "tool_outputs_retained": False,
        "task_ids_retained": False,
        "task_result_bodies_retained": False,
        "paths_retained": False,
    }


def summarize_open_tool_calls_at_boundary(
    tool_calls: Sequence[Mapping[str, Any]],
    *,
    boundary_elapsed_ms: int,
    terminal: str,
) -> dict[str, Any]:
    """Project ACP calls without a terminal update when the run ends.

    This records the observed stream boundary only. An unpaired ACP start does
    not prove whether an underlying process is still running or whether its
    terminal notification was not published.
    """
    boundary = (boundary_elapsed_ms if isinstance(boundary_elapsed_ms, int)
                and not isinstance(boundary_elapsed_ms, bool)
                and 0 <= boundary_elapsed_ms <= MAX_JOB_ELAPSED_MS else None)
    terminal_state = (terminal if isinstance(terminal, str) and terminal in {
        "completed", "failed", "cancelled", "timeout",
    } else "unknown")
    open_calls: list[dict[str, Any]] = []
    for ordinal, call in enumerate(tool_calls, start=1):
        if call.get("status", "unknown") != "unknown":
            continue
        raw_tool = call.get("tool")
        tool = raw_tool if isinstance(raw_tool, str) and raw_tool in _ACTIVITY_TOOLS else "other"
        if tool in {"test", "job"}:
            job = call.get("job")
            job = job if isinstance(job, Mapping) else {}
            raw_action = job.get("action")
        elif tool == "task":
            raw_action = call.get("task_action")
        else:
            raw_action = "invoke"
        allowed_actions = _JOB_ACTIONS | _TASK_ACTIONS | {"invoke"}
        action = raw_action if isinstance(raw_action, str) and raw_action in allowed_actions else "unknown"
        started = call.get("started_elapsed_ms")
        if not isinstance(started, int) or isinstance(started, bool) or started < 0:
            started = None
        safe_index = call.get("index")
        if not isinstance(safe_index, int) or isinstance(safe_index, bool) or safe_index < 0:
            safe_index = ordinal
        age = (boundary - started if boundary is not None and started is not None
               and started <= boundary else None)
        open_calls.append({
            "index": min(safe_index, 1_000_000),
            "tool_class": tool,
            "action": action,
            "started_elapsed_ms": started if started is None or started <= MAX_JOB_ELAPSED_MS else None,
            "age_at_boundary_ms": age,
            "stream_state": "start_without_terminal",
        })
    open_calls.sort(key=lambda event: (event["index"], event["tool_class"], event["action"]))
    return {
        "schema": "h007_open_tool_calls_at_boundary_v1",
        "state": "observed" if tool_calls else "no_tool_calls",
        "boundary_elapsed_ms": boundary,
        "run_terminal": terminal_state,
        "unpaired_call_count": len(open_calls),
        "unpaired_calls": open_calls[:MAX_JOB_SEQUENCE_EVENTS],
        "sequence_truncated": len(open_calls) > MAX_JOB_SEQUENCE_EVENTS,
        "underlying_process_state_inferred": False,
        "raw_tool_ids_retained": False,
        "tool_arguments_retained": False,
        "tool_outputs_retained": False,
        "paths_retained": False,
    }


def _safe_assertion_operation(line: str) -> str | None:
    """Map a traceback assertion call to a coarse category without retaining code."""
    line = line.strip()
    if re.match(r"^assert(?:\s|$)", line):
        return "direct_assertion"
    match = _ASSERTION_CALL.match(line)
    if match is None:
        return None
    method = match.group("method")
    if method == "fail":
        return "explicit_failure"
    if method in {"assertEqual", "assertNotEqual", "assertAlmostEqual",
                  "assertNotAlmostEqual", "assertCountEqual"}:
        return "equality"
    if method in {"assertTrue", "assertFalse"}:
        return "truthiness"
    if method in {"assertIn", "assertNotIn"}:
        return "membership"
    if method in {"assertIs", "assertIsNot", "assertIsNone", "assertIsNotNone"}:
        return "identity"
    if method in {"assertRaises", "assertRaisesRegex", "assertWarns",
                  "assertWarnsRegex", "assertLogs", "assertNoLogs"}:
        return "exception_or_warning"
    if method in {"assertGreater", "assertGreaterEqual", "assertLess",
                  "assertLessEqual"}:
        return "ordering"
    if method in {"assertRegex", "assertNotRegex"}:
        return "pattern"
    if method in {"assertSequenceEqual", "assertListEqual", "assertTupleEqual",
                  "assertSetEqual", "assertDictEqual"}:
        return "container_equality"
    return "other_assertion"


def _safe_setup_failure_reason(output: str) -> str:
    """Classify common setup failures without retaining verifier output."""
    normalized = output[:MAX_VERIFIER_OUTPUT_CHARS].lower()
    if "waiting for file lock" in normalized or "blocking waiting for file lock" in normalized:
        return "build_lock_wait"
    if "no space left on device" in normalized:
        return "disk_full"
    if "no such file or directory" in normalized:
        return "missing_path"
    if "modulenotfounderror" in normalized or "no module named" in normalized:
        return "python_import_failure"
    if "could not compile" in normalized or "failed to run custom build command" in normalized:
        return "workspace_build_failure"
    if "timed out" in normalized or "timeout expired" in normalized:
        return "setup_command_timeout"
    return "unclassified_setup_failure"


def summarize_provider_attempt_log(path: Path, *, run_id: str) -> dict[str, Any]:
    """Project safe provider-attempt timing/retry fields from the private debug log.

    Only named provider-turn tracing events and a short field allowlist are
    retained. Raw log lines, request/response content, URLs, credentials,
    session identifiers, and provider attempt IDs are never returned.
    """
    result: dict[str, Any] = {
        "schema": PROVIDER_ATTEMPT_SCHEMA,
        "state": "missing",
        "log_present": False,
        "log_bytes": 0,
        "recognized_events": 0,
        "provider_attempts_started": 0,
        "provider_attempts_succeeded": 0,
        "provider_attempts_failed": 0,
        "unresolved_provider_attempt_count": None,
        "attempt_accounting_consistent": None,
        "retry_scheduled_count": 0,
        "setup_timeout_count": 0,
        "stream_idle_timeout_count": 0,
        "first_response_event_timeout_count": 0,
        "later_stream_timeout_count": 0,
        "retry_chain_deadline_count": 0,
        "last_failure": None,
        "input_truncated": False,
        "raw_log_in_receipt": False,
        "raw_provider_text_in_receipt": False,
        "attempt_ids_retained": False,
    }
    try:
        size = path.stat().st_size
    except FileNotFoundError:
        return result
    except OSError:
        result["state"] = "unreadable"
        return result
    result["log_present"] = True
    result["log_bytes"] = size
    if size > MAX_PROVIDER_LOG_BYTES:
        result["state"] = "too_large"
        result["input_truncated"] = True
        return result

    attempts: dict[str, dict[str, Any]] = {}
    event_count = 0
    truncated_line = False
    try:
        with path.open("rb") as stream:
            while True:
                raw_line = stream.readline(MAX_PROVIDER_LOG_LINE_BYTES + 1)
                if not raw_line:
                    break
                if len(raw_line) > MAX_PROVIDER_LOG_LINE_BYTES:
                    truncated_line = True
                    while raw_line and not raw_line.endswith(b"\n"):
                        raw_line = stream.readline(MAX_PROVIDER_LOG_LINE_BYTES + 1)
                    continue
                line = raw_line.decode("utf-8", errors="ignore")
                marker = next((name for token, name in _PROVIDER_LOG_MARKERS if token in line), None)
                if marker is None:
                    continue
                event_count += 1
                values: dict[str, str] = {}
                for match in _PROVIDER_LOG_FIELD.finditer(line):
                    name = match.group("name")
                    if name in _PROVIDER_SAFE_FIELD_NAMES:
                        values[name] = match.group("quoted") or match.group("bare") or ""

                attempt_id = values.get("attempt_id")
                correlation = None
                if attempt_id and re.fullmatch(r"[A-Za-z0-9_-]{1,64}", attempt_id):
                    correlation = hashlib.sha256(
                        f"{run_id}:{attempt_id}".encode("utf-8")
                    ).hexdigest()
                    attempts.setdefault(correlation, {"attempt_correlation": correlation})
                attempt = attempts.get(correlation) if correlation is not None else None

                if marker == "setup_started":
                    result["provider_attempts_started"] += 1
                elif marker == "setup_timeout":
                    result["setup_timeout_count"] += 1
                elif marker == "stream_idle_timeout":
                    result["stream_idle_timeout_count"] += 1
                    decoded = _safe_provider_int(values.get("decoded_event_count"))
                    if decoded == 0:
                        result["first_response_event_timeout_count"] += 1
                    elif decoded is not None and decoded > 0:
                        result["later_stream_timeout_count"] += 1
                elif marker == "retry_scheduled":
                    result["retry_scheduled_count"] += 1
                elif marker == "retry_chain_deadline":
                    result["retry_chain_deadline_count"] += 1
                elif marker == "attempt_succeeded":
                    result["provider_attempts_succeeded"] += 1

                if attempt is not None:
                    index = _safe_provider_int(values.get("attempt_index"))
                    if index is not None and index <= 255:
                        attempt["attempt_index"] = index
                    error_class = values.get("error_class")
                    if error_class and _PROVIDER_SAFE_ERROR_CLASS.fullmatch(error_class):
                        attempt["error_class"] = error_class
                    for field in ("attempts_left", "attempts_consumed", "remaining_ms",
                                  "decoded_event_count",
                                  "elapsed_ms", "elapsed_since_setup_ms",
                                  "since_last_decoded_event_ms", "idle_wait_ms",
                                  "setup_elapsed_ms"):
                        integer = _safe_provider_int(values.get(field))
                        if integer is not None and integer <= 3_600_000:
                            attempt[field] = integer
                    if values.get("stage") in _PROVIDER_PHASES:
                        attempt["setup_stage"] = values["stage"]
                    if values.get("event_kind") in _PROVIDER_EVENT_KINDS:
                        attempt["last_response_event_kind"] = values["event_kind"]

                    if marker == "setup_timeout":
                        attempt["failure_phase"] = "response_headers"
                    elif marker == "stream_idle_timeout":
                        decoded = _safe_provider_int(values.get("decoded_event_count"))
                        attempt["failure_phase"] = (
                            "first_response_event" if decoded == 0 else "later_stream_event"
                        )
                    elif marker == "retry_scheduled":
                        attempt["retry_outcome"] = "retry_scheduled"
                    elif marker == "retry_chain_deadline":
                        attempt["retry_outcome"] = "retry_chain_deadline"
                    elif marker == "attempt_failed_terminally":
                        attempt["retry_outcome"] = "terminal_failure"
                    elif marker == "attempt_superseded":
                        attempt["failure_phase"] = "after_visible_output"
                        attempt["retry_outcome"] = "superseded_after_visible_output"

        failures = [attempt for attempt in attempts.values()
                    if "failure_phase" in attempt or "retry_outcome" in attempt]
        result["provider_attempts_failed"] = len(failures)
        result["last_failure"] = failures[-1] if failures else None
        result["recognized_events"] = event_count
        result["input_truncated"] = truncated_line
        if not truncated_line:
            terminal_attempts = (result["provider_attempts_succeeded"]
                                 + result["provider_attempts_failed"])
            started_attempts = result["provider_attempts_started"]
            result["unresolved_provider_attempt_count"] = max(
                0, started_attempts - terminal_attempts)
            result["attempt_accounting_consistent"] = (
                started_attempts >= terminal_attempts)
        result["state"] = "observed" if event_count else "no_provider_events"
        return result
    except OSError:
        result["state"] = "unreadable"
        return result


def _safe_provider_int(value: str | None) -> int | None:
    if value is None or re.fullmatch(r"\d{1,10}", value) is None:
        return None
    try:
        return int(value)
    except ValueError:
        return None


def extract_task_failure_class(output: Any) -> str | None:
    """Extract only a canonical delegated-run failure class from tool output."""
    if not isinstance(output, str):
        return None
    prefix = "Failure stage/class:"
    for line in output.splitlines():
        if line.startswith(prefix):
            candidate = line[len(prefix):].strip()
            return candidate if candidate in _SAFE_FAILURE_CLASSES else None
    return None


def summarize_unittest_output(
    output: str,
    *,
    exit_code: int | None,
    timed_out: bool = False,
    runner_error_class: str | None = None,
    input_truncated: bool = False,
) -> dict[str, Any]:
    """Project Python unittest output into names and outcome classes only."""
    if len(output) > MAX_VERIFIER_OUTPUT_CHARS:
        half = MAX_VERIFIER_OUTPUT_CHARS // 2
        output = output[:half] + "\n...[diagnostic input truncated]...\n" + output[-half:]
        input_truncated = True
    if timed_out:
        state = "timeout"
    elif runner_error_class:
        state = "runner_error"
    else:
        state = "unknown"

    tests_run: int | None = None
    verbose: dict[str, dict[str, Any]] = {}
    failed_sections: list[dict[str, Any]] = []
    current_failure: dict[str, Any] | None = None
    traceback_active = False
    aggregate: dict[str, int] = {}
    setup_exception_class: str | None = None

    for raw_line in output.splitlines():
        line = raw_line.strip()
        ran = _RAN_LINE.match(line)
        if ran:
            tests_run = int(ran.group("count"))

        result = _VERBOSE_RESULT.match(line)
        if result and len(verbose) < MAX_DIAGNOSTIC_TESTS:
            test_id = _bounded_test_id(result.group("test"))
            outcome = result.group("result")
            row = {"test_id": test_id, "outcome": outcome.lower()}
            if outcome == "FAIL":
                row["failure_kind"] = "assertion_failure"
            elif outcome == "ERROR":
                row["failure_kind"] = "test_error"
            verbose[test_id] = row
            current_failure = row if outcome in {"FAIL", "ERROR"} else None
            traceback_active = False
            continue

        heading = _FAILURE_HEADING.match(line)
        if heading:
            test_id = _bounded_test_id(heading.group("test"))
            row = verbose.get(test_id)
            if row is None:
                row = {
                    "test_id": test_id,
                    "outcome": "fail" if heading.group("kind") == "FAIL" else "error",
                    "failure_kind": "assertion_failure"
                    if heading.group("kind") == "FAIL" else "test_error",
                }
                if len(failed_sections) < MAX_DIAGNOSTIC_TESTS:
                    failed_sections.append(row)
            current_failure = row
            traceback_active = False
            continue

        if line.startswith("FAILED"):
            aggregate = {match.group("name"): int(match.group("count"))
                         for match in _COUNT_FIELD.finditer(line)}
            current_failure = None
            traceback_active = False
            continue
        if line == "OK":
            current_failure = None
            traceback_active = False
            continue

        if current_failure is not None and line == "Traceback (most recent call last):":
            traceback_active = True
            continue

        exception = _EXCEPTION_LINE.match(line)
        if current_failure is not None and exception:
            exception_class = exception.group("class")[:128]
            current_failure["exception_class"] = exception_class
            current_failure["exception_message"] = _safe_failure_message(
                line[exception.end():].lstrip(), exception_class)
            traceback_active = False
        elif setup_exception_class is None and exception:
            setup_exception_class = exception.group("class")[:128]
            traceback_active = False
        elif current_failure is not None and traceback_active:
            operation = _safe_assertion_operation(line)
            if operation is not None:
                current_failure["_safe_assertion_operation"] = operation

    failure_rows = list(verbose.values())
    if not failure_rows:
        failure_rows = failed_sections
    failure_rows = [row for row in failure_rows
                    if row.get("outcome") in {"fail", "error"}]
    assertion_operation_counts: Counter[str] = Counter()
    failure_domain_counts: Counter[str] = Counter()
    failure_domain_details: dict[str, dict[str, Any]] = {}
    for row in failure_rows:
        operation = row.pop("_safe_assertion_operation", None)
        test_id = row.get("test_id")
        domain = _verifier_failure_domain(test_id) if isinstance(test_id, str) else "unmapped"
        failure_domain_counts[domain] += 1
        detail = failure_domain_details.setdefault(domain, {
            "failure_count": 0,
            "failure_kinds": Counter(),
            "exception_classes": Counter(),
            "assertion_operations": Counter(),
            "message_categories": Counter(),
            "safe_term_counts": Counter(),
        })
        detail["failure_count"] += 1
        failure_kind = row.get("failure_kind")
        detail["failure_kinds"][failure_kind if failure_kind in {
            "assertion_failure", "test_error"
        } else "unclassified"] += 1
        exception_class = row.get("exception_class")
        safe_exception = (
            exception_class if exception_class in _SAFE_EXCEPTION_CLASSES else "other"
        )
        detail["exception_classes"][safe_exception] += 1
        safe_operation = operation if operation in {
            "direct_assertion", "equality", "truthiness", "membership", "identity",
            "exception_or_warning", "ordering", "pattern", "container_equality",
            "explicit_failure", "other_assertion",
        } else "unclassified"
        detail["assertion_operations"][safe_operation] += 1
        message = row.get("exception_message")
        if isinstance(message, Mapping):
            message_kind = message.get("kind")
            safe_message_kind = message_kind if message_kind in {
                "invalid_integer_literal", "invalid_float_literal", "unpack_too_few",
                "unpack_too_many", "conversion_failure", "value_error_message",
                "expected_actual_mismatch", "expected_value_missing", "comparison_mismatch",
                "assertion_message", "exception_message",
            } else "unclassified"
            detail["message_categories"][safe_message_kind] += 1
            terms = message.get("terms")
            if isinstance(terms, list):
                for term in terms:
                    if isinstance(term, str) and term in _SAFE_FAILURE_TERMS:
                        detail["safe_term_counts"][term] += 1
        if row.get("exception_class") == "AssertionError":
            assertion_operation_counts[operation or "unclassified"] += 1

    public_failure_domain_details = {
        domain: {
            key: dict(sorted(value.items())) if isinstance(value, Counter) else value
            for key, value in details.items()
        }
        for domain, details in sorted(failure_domain_details.items())
    }

    verbose_failures = sum(row.get("outcome") in {"fail", "FAIL"}
                           for row in verbose.values())
    verbose_errors = sum(row.get("outcome") in {"error", "ERROR"}
                         for row in verbose.values())
    failures = aggregate.get("failures", verbose_failures)
    errors = aggregate.get("errors", verbose_errors)

    if timed_out:
        state = "timeout"
    elif runner_error_class:
        state = "runner_error"
    elif (tests_run in (None, 0) and exit_code not in (None, 0)
          and (setup_exception_class or any(
              row.get("test_id", "").startswith("setUpClass ") for row in failure_rows
          ))):
        state = "setup_error"
    elif exit_code == 0 and not failures and not errors and tests_run:
        state = "passed"
    elif exit_code == 0 and tests_run == 0:
        state = "empty_suite"
    elif failures and errors:
        state = "mixed_test_failures"
    elif failures:
        state = "assertion_failures"
    elif errors:
        state = "test_errors"
    elif isinstance(exit_code, int) and exit_code != 0:
        state = "unclassified_failure"

    verbose_total = len(verbose)
    classified_state = state in {
        "passed", "setup_error", "empty_suite", "assertion_failures", "test_errors",
        "mixed_test_failures", "timeout", "runner_error",
    }
    detail_complete = classified_state and not input_truncated and (
        not (failures + errors) or len(failure_rows) == failures + errors
    )
    return {
        "schema": VERIFIER_SCHEMA,
        "state": state,
        "exit_code": exit_code,
        "timed_out": timed_out,
        "runner_error_class": runner_error_class,
        "tests_run": tests_run,
        "verbose_tests_observed": verbose_total,
        "passed_count": sum(row.get("outcome") == "ok" for row in verbose.values()),
        "skipped_count": sum(row.get("outcome") == "skipped" for row in verbose.values()),
        "failure_counts": {"failures": failures, "errors": errors},
        "failure_domain_counts": dict(failure_domain_counts),
        "failure_domain_details": public_failure_domain_details,
        "assertion_operation_counts": dict(assertion_operation_counts),
        "failed_tests": failure_rows,
        "setup_exception_class": setup_exception_class if state == "setup_error" else None,
        "setup_failure_reason": (
            _safe_setup_failure_reason(output) if state == "setup_error" else None
        ),
        "diagnostic_detail_complete": detail_complete,
        "input_truncated": input_truncated,
        "raw_output_retained": False,
        "assertion_text_retained": False,
        "traceback_retained": False,
    }


def verifier_tests_observed(diagnostic: Any) -> bool:
    """Return true only when a complete diagnostic proves tests actually ran."""
    return (
        isinstance(diagnostic, Mapping)
        and diagnostic.get("state") in _TEST_EXECUTION_STATES
        and isinstance(diagnostic.get("tests_run"), int)
        and not isinstance(diagnostic.get("tests_run"), bool)
        and diagnostic["tests_run"] > 0
        and diagnostic.get("diagnostic_detail_complete") is True
        and diagnostic.get("timed_out") is not True
        and diagnostic.get("runner_error_class") is None
    )


def _digest(value: Any) -> str | None:
    if not isinstance(value, str) or re.fullmatch(r"[0-9a-f]{64}", value) is None:
        return None
    return value


def summarize_reviewer_lifecycle(tool_calls: Sequence[Mapping[str, Any]]) -> dict[str, Any]:
    """Summarize one verifier child from the receipt's body-free call projection."""
    task_calls = [call for call in tool_calls
                  if call.get("tool") == "task" and call.get("task_action") in _TASK_ACTIONS]
    action_counts = Counter(str(call.get("task_action")) for call in task_calls)
    spawns = [call for call in task_calls
              if call.get("task_action") == "spawn"
              and call.get("task_spawn_agent") == "verifier"]
    events: list[dict[str, Any]] = []
    exact_id_matches = 0
    for spawn in spawns:
        task_id = _digest(spawn.get("task_spawn_task_id_sha256"))
        run_id = _digest(spawn.get("task_spawn_run_id_sha256"))
        if task_id is None or run_id is None:
            continue
        for call in task_calls:
            action = call.get("task_action")
            if action == "spawn":
                match = (call is spawn
                         and _digest(call.get("task_spawn_task_id_sha256")) == task_id
                         and _digest(call.get("task_spawn_run_id_sha256")) == run_id)
                run_status = call.get("task_spawn_run_status")
            elif action in {"status", "wait"}:
                match = _digest(call.get("task_run_id_sha256")) == run_id
                run_status = call.get("task_run_status")
            elif action == "continue":
                match = _digest(call.get("task_run_id_sha256")) == run_id
                run_status = call.get("task_run_status")
            elif action == "get":
                request_task = _digest(call.get("task_id_sha256"))
                response_task = _digest(call.get("task_result_task_id_sha256"))
                response_run = _digest(call.get("task_result_run_id_sha256"))
                match = request_task == task_id and response_task == task_id and response_run == run_id
                run_status = call.get("task_result_run_status")
            else:
                match = False
                run_status = None
            if not match:
                continue
            exact_id_matches += 1
            status = run_status if isinstance(run_status, str) and run_status in _RUN_STATUSES else None
            event = {
                "index": call.get("index") if isinstance(call.get("index"), int) else None,
                "action": action,
                "call_status": call.get("status")
                if call.get("status") in {"completed", "failed", "cancelled", "unknown"} else "unknown",
                "run_status": status,
                "started_elapsed_ms": call.get("started_elapsed_ms")
                if isinstance(call.get("started_elapsed_ms"), int) else None,
                "finished_elapsed_ms": call.get("finished_elapsed_ms")
                if isinstance(call.get("finished_elapsed_ms"), int) else None,
                "failure_class": call.get("task_failure_class")
                if isinstance(call.get("task_failure_class"), str)
                and call.get("task_failure_class") in _SAFE_FAILURE_CLASSES else None,
            }
            events.append(event)

    events.sort(key=lambda event: (event["index"] is None, event["index"] or 0))
    total_matched_events = len(events)
    if total_matched_events > MAX_REVIEW_EVENTS:
        events = events[:MAX_REVIEW_EVENTS]
    statuses = [event for event in events if event["action"] in {"status", "wait"}
                and event["run_status"] in _RUN_STATUSES]
    terminal_statuses = [event for event in statuses if event["run_status"] in _TERMINAL_STATUSES]
    final_status = statuses[-1]["run_status"] if statuses else None
    final_terminal = terminal_statuses[-1] if terminal_statuses else None
    continuations = [event for event in events if event["action"] == "continue"]
    terminal_failures = [event for event in terminal_statuses
                         if event["run_status"] == "failed"]
    continuations_after_terminal_failure = [
        event for event in continuations
        if event["index"] is not None
        and any(failure["index"] is not None and failure["index"] < event["index"]
                for failure in terminal_failures)
    ]
    gets = [event for event in events if event["action"] == "get"]
    completed_gets = [event for event in gets if event["run_status"] == "completed"]
    gets_after_terminal = [event for event in gets if final_terminal is not None
                           and event["index"] is not None and final_terminal["index"] is not None
                           and event["index"] > final_terminal["index"]]
    completed_get_after_status = any(
        event["run_status"] == "completed"
        and event["index"] is not None
        and any(status["run_status"] == "completed"
                and status["index"] is not None
                and status["index"] < event["index"] for status in statuses)
        for event in gets
    )
    ordered_completed_get_count = sum(
        event["run_status"] == "completed"
        and event["index"] is not None
        and any(status["run_status"] == "completed"
                and status["index"] is not None
                and status["index"] < event["index"] for status in statuses)
        for event in gets
    )
    nonterminal_gets = [event for event in gets if event["run_status"] in _RUN_STATUSES
                        and event["run_status"] not in _TERMINAL_STATUSES]
    return {
        "schema": REVIEWER_SCHEMA,
        "reviewer_spawn_count": len(spawns),
        "task_action_counts": dict(sorted(action_counts.items())),
        "reviewer_task_id_sha256": _digest(spawns[0].get("task_spawn_task_id_sha256"))
        if len(spawns) == 1 else None,
        "reviewer_run_id_sha256": _digest(spawns[0].get("task_spawn_run_id_sha256"))
        if len(spawns) == 1 else None,
        "reviewer_event_count": len(events),
        "reviewer_events_truncated": total_matched_events > MAX_REVIEW_EVENTS,
        "exact_identity_matched_call_count": exact_id_matches,
        "ordered_events": events,
        "get_count": len(gets),
        "duplicate_get_count": max(0, len(gets) - 1),
        "get_nonterminal_count": len(nonterminal_gets),
        "completed_get_result_observed_count": len(completed_gets),
        "ordered_completed_get_result_count": ordered_completed_get_count,
        "explicit_terminal_observation_count": len(terminal_statuses),
        "get_after_terminal_status_count": len(gets_after_terminal),
        "reviewer_continue_count": len(continuations),
        "continue_after_terminal_failure_count": len(continuations_after_terminal_failure),
        "continue_after_terminal_failure_observed": bool(
            continuations_after_terminal_failure
        ),
        "get_run_status_counts": dict(sorted(Counter(
            event["run_status"] or "unknown" for event in gets).items())),
        "status_sequence": [{"index": event["index"], "run_status": event["run_status"],
                             "failure_class": event["failure_class"]}
                            for event in statuses],
        "final_status": final_status,
        "final_failure_class": final_terminal["failure_class"] if final_terminal else None,
        "completed_review_result_observed": (
            len(spawns) == 1 and final_status == "completed" and completed_get_after_status
        ),
        "compliant_status_then_get_lifecycle": (
            len(spawns) == 1 and final_status == "completed" and len(gets) == 1
            and ordered_completed_get_count == 1
        ),
        "raw_task_arguments_retained": False,
        "task_transcript_retained": False,
        "task_result_body_retained": False,
    }


def summarize_reviewer_durable_state(
    database_path: Path,
    *,
    reviewer_task_id_sha256: str | None,
    reviewer_run_id_sha256: str | None,
) -> dict[str, Any]:
    """Read the final reviewer state from the lane DB without retaining IDs or bodies.

    Tool-call receipts can end while a child Run is still live, so the ordered
    call projection alone cannot distinguish an unfinished child from one that
    later failed its configured deadline. Correlate the private lane's durable
    rows by the already recorded identity digests, and return only bounded
    state fields and elapsed time.
    """
    result: dict[str, Any] = {
        "schema": "h007_reviewer_durable_state_diagnostic_v1",
        "state": "unavailable",
        "run_status": None,
        "run_terminal": None,
        "task_status": None,
        "failure_class": None,
        "elapsed_ms": None,
        "matched_run_count": 0,
        "matched_task_count": 0,
        "raw_identifiers_retained": False,
        "failure_message_retained": False,
        "task_result_body_retained": False,
        "transcript_retained": False,
    }
    digest_pattern = re.compile(r"^[0-9a-f]{64}$")
    if not (isinstance(reviewer_task_id_sha256, str)
            and digest_pattern.fullmatch(reviewer_task_id_sha256)
            and isinstance(reviewer_run_id_sha256, str)
            and digest_pattern.fullmatch(reviewer_run_id_sha256)):
        result["state"] = "reviewer_identity_unavailable"
        return result

    try:
        path = Path(database_path)
        if not path.is_file():
            result["state"] = "database_missing"
            return result
        if path.is_symlink():
            result["state"] = "database_unavailable"
            return result
        if path.stat().st_size > MAX_REVIEWER_DATABASE_BYTES:
            result["state"] = "database_too_large"
            return result
        uri = "file:" + quote(str(path.resolve()), safe="/") + "?mode=ro"
        connection = sqlite3.connect(uri, uri=True, timeout=0.25)
        connection.execute("PRAGMA query_only = ON")
    except (OSError, sqlite3.Error, ValueError):
        result["state"] = "database_unavailable"
        return result

    try:
        tables = {row[0] for row in connection.execute(
            "SELECT name FROM sqlite_master WHERE type = 'table'"
        )}
        if "agent_run" not in tables or "agent_task" not in tables:
            result["state"] = "schema_unavailable"
            return result

        run_columns = {row[1] for row in connection.execute(
            'PRAGMA table_info("agent_run")'
        )}
        task_columns = {row[1] for row in connection.execute(
            'PRAGMA table_info("agent_task")'
        )}
        required_run_columns = {
            "run_id", "task_id", "status", "terminal", "failure_class",
            "started_at", "finished_at",
        }
        if not required_run_columns <= run_columns or not {
            "task_id", "status",
        } <= task_columns:
            result["state"] = "schema_unavailable"
            return result

        runs = connection.execute(
            "SELECT run_id, task_id, status, terminal, failure_class, started_at, finished_at "
            "FROM agent_run LIMIT ?", (MAX_REVIEWER_DATABASE_ROWS + 1,),
        ).fetchall()
        if len(runs) > MAX_REVIEWER_DATABASE_ROWS:
            result["state"] = "database_row_limit"
            return result
        tasks = connection.execute(
            "SELECT task_id, status FROM agent_task LIMIT ?",
            (MAX_REVIEWER_DATABASE_ROWS + 1,),
        ).fetchall()
        if len(tasks) > MAX_REVIEWER_DATABASE_ROWS:
            result["state"] = "database_row_limit"
            return result

        def digest(value: Any) -> str | None:
            return (hashlib.sha256(value.encode("utf-8")).hexdigest()
                    if isinstance(value, str) else None)

        run_matches = [row for row in runs
                       if digest(row[0]) == reviewer_run_id_sha256]
        task_matches = [row for row in tasks
                        if digest(row[0]) == reviewer_task_id_sha256]
        result["matched_run_count"] = len(run_matches)
        result["matched_task_count"] = len(task_matches)
        if len(run_matches) != 1:
            result["state"] = "run_not_found" if not run_matches else "ambiguous_run"
            return result

        run = run_matches[0]
        if digest(run[1]) != reviewer_task_id_sha256:
            result["state"] = "identity_mismatch"
            return result
        if len(task_matches) > 1:
            result["state"] = "ambiguous_task"
            return result

        def bounded_status(value: Any) -> str | None:
            return value if isinstance(value, str) and value in _RUN_STATUSES else "unknown"

        result["state"] = "observed"
        result["run_status"] = bounded_status(run[2])
        result["run_terminal"] = bounded_status(run[3])
        result["task_status"] = bounded_status(task_matches[0][1]) if task_matches else None
        if isinstance(run[4], str):
            result["failure_class"] = (
                run[4] if run[4] in _SAFE_FAILURE_CLASSES else "unclassified"
            )
        started_at, finished_at = run[5], run[6]
        if (isinstance(started_at, int) and not isinstance(started_at, bool)
                and isinstance(finished_at, int) and not isinstance(finished_at, bool)
                and finished_at >= started_at):
            result["elapsed_ms"] = min(finished_at - started_at, 7 * 24 * 60 * 60 * 1000)
        return result
    except (sqlite3.Error, OSError, ValueError):
        result["state"] = "database_unavailable"
        return result
    finally:
        connection.close()

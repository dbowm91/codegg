"""Bounded, redacted causal traces and evidence-limited attribution reports."""
from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path
from typing import Any, Mapping

SCHEMA = "harness_causal_trace_v1"
MAX_EVENTS = 10_000
MAX_BYTES = 1_048_576
_JOB_ID = re.compile(r"\b([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\b", re.I)
_JOB_STATUS = re.compile(r"\bjob\s+(queued|running|completed|failed|cancelled|canceled|timed_out|still-running)\b", re.I)
_TOOLS = frozenset({"read", "write", "edit", "bash", "test", "job", "task", "lint", "format",
                    "python", "git", "context_read", "unknown"})


def _id_digest(run_id: str, value: str) -> str:
    return hashlib.sha256((run_id + "\0" + value).encode()).hexdigest()[:24]


class Recorder:
    """Append-only projection of observed ACP events; raw payloads never persist."""

    def __init__(self, path: Path, *, run_id: str, case_id: str,
                 source_sha256: str | None, binary_sha256: str | None) -> None:
        self.path = path
        self.run_id = run_id
        self.identity = {"run_id": run_id, "case_id": case_id,
                         "source_sha256": source_sha256, "binary_sha256": binary_sha256,
                         "daemon_generation": None}
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._stream = self.path.open("xb")
        self._seq = 0
        self._bytes = 0
        self._previous = "0" * 64
        self._calls: dict[str, tuple[str, str | None]] = {}
        self._jobs: dict[str, str] = {}
        self._started: set[str] = set()
        self._terminal: set[str] = set()
        self._truncated = False
        self._dropped = 0
        self._closed = False

    @property
    def state(self) -> str:
        return "truncated" if self._truncated else "complete"

    def bind_daemon_generation(self, generation: str | None) -> None:
        self.identity["daemon_generation"] = generation if isinstance(generation, str) else None

    def observe(self, notification: Mapping[str, Any]) -> None:
        if notification.get("method") != "session/update":
            return
        params = notification.get("params")
        update = params.get("update") if isinstance(params, Mapping) else None
        if not isinstance(update, Mapping):
            return
        kind = update.get("sessionUpdate")
        call_id = update.get("toolCallId")
        if kind == "tool_call" and isinstance(call_id, str) and call_id:
            title = update.get("title")
            action = update.get("codeggAction")
            tool = title if isinstance(title, str) and title in _TOOLS else "unknown"
            safe_action = action if action in {"submit", "status", "wait", "output", "cancel"} else None
            self._calls[call_id] = (tool, safe_action)
            self._started.add(call_id)
            self._append("tool_started", notification, tool=tool,
                         call_correlation=_id_digest(self.run_id, call_id))
        elif kind == "tool_call_update" and isinstance(call_id, str) and call_id:
            tool, action = self._calls.get(call_id, ("unknown", None))
            status = update.get("status")
            safe_status = status if status in {"completed", "failed", "cancelled", "unknown"} else "unknown"
            self._terminal.add(call_id)
            self._append("tool_terminal", notification, tool=tool,
                         call_correlation=_id_digest(self.run_id, call_id), status=safe_status)
            raw = update.get("rawOutput")
            if tool in {"test", "job"} and isinstance(raw, str):
                job_match, status_match = _JOB_ID.search(raw), _JOB_STATUS.search(raw)
                if job_match:
                    job_id = job_match.group(1).lower()
                    self._jobs[job_id] = action or ("submit" if tool == "test" else "unknown")
                    job_status = status_match.group(1).lower() if status_match else "unknown"
                    if job_status == "canceled":
                        job_status = "cancelled"
                    self._append("job_observation", notification,
                                 call_correlation=_id_digest(self.run_id, call_id),
                                 job_correlation=_id_digest(self.run_id, job_id),
                                 job_action=self._jobs[job_id], job_status=job_status)
        elif kind == "codegg_event_gap":
            dropped = update.get("dropped")
            self._append("event_gap", notification,
                         dropped=dropped if isinstance(dropped, int) and not isinstance(dropped, bool)
                         and dropped >= 0 else None)

    def record(self, event_type: str, *, elapsed_ms: int | None = None,
               detail: str | None = None) -> None:
        """Record a bounded controller observation, never raw exception text."""
        if event_type not in {"cancel_requested", "provider_timeout", "verifier_started",
                              "verifier_terminal", "continuation", "checkpoint", "repair_cycle"}:
            raise ValueError("unsupported causal event type")
        row: dict[str, Any] = {"elapsed_ms": elapsed_ms if isinstance(elapsed_ms, int) and elapsed_ms >= 0 else None}
        if detail in {"user", "disk_floor", "activity_stall", "budget", "controller", "passed", "failed", "unknown"}:
            row["detail"] = detail
        self._append(event_type, None, **row)

    def _append(self, event_type: str, notification: Mapping[str, Any] | None = None, **fields: Any) -> None:
        if self._closed:
            return
        elapsed = notification.get("_received_elapsed_ms") if isinstance(notification, Mapping) else None
        row = {"schema": SCHEMA, **self.identity, "seq": self._seq + 1,
               "elapsed_ms": elapsed if isinstance(elapsed, int) and not isinstance(elapsed, bool)
               and elapsed >= 0 else None,
               "event_type": event_type, **fields, "prev_sha256": self._previous}
        encoded_for_hash = json.dumps(row, sort_keys=True, separators=(",", ":")).encode()
        digest = hashlib.sha256(encoded_for_hash).hexdigest()
        row["event_sha256"] = digest
        encoded = json.dumps(row, sort_keys=True, separators=(",", ":")).encode() + b"\n"
        if self._seq >= MAX_EVENTS or self._bytes + len(encoded) > MAX_BYTES:
            self._truncated = True
            self._dropped += 1
            return
        self._stream.write(encoded)
        self._stream.flush()
        self._seq += 1
        self._bytes += len(encoded)
        self._previous = digest

    def finish(self) -> dict[str, Any]:
        if not self._closed:
            unpaired = len(self._started - self._terminal)
            self._append("trace_terminal", None, state=self.state, event_count=self._seq,
                         unpaired_tool_starts=unpaired, dropped_events=self._dropped)
            self._stream.flush()
            self._stream.close()
            self._closed = True
        return {"schema": SCHEMA, "state": self.state, "path": str(self.path),
                "daemon_generation": self.identity.get("daemon_generation"),
                "event_count": self._seq, "bytes": self._bytes,
                "dropped_events": self._dropped,
                "sha256": hashlib.sha256(self.path.read_bytes()).hexdigest()}


def attribution_report(trace_rows: list[dict[str, Any]], receipt: Mapping[str, Any]) -> dict[str, Any]:
    """Classify only what the trace and typed receipt establish."""
    trace_state = "complete" if any(row.get("event_type") == "trace_terminal"
                                     for row in trace_rows) else "incomplete"
    if any(row.get("event_type") == "trace_terminal" and row.get("state") == "truncated"
           for row in trace_rows):
        trace_state = "truncated"
    if any(row.get("event_type") == "event_gap" for row in trace_rows):
        trace_state = "gap"
    trace_receipt = receipt.get("causal_trace")
    if isinstance(trace_receipt, Mapping) and trace_receipt.get("state") == "truncated":
        trace_state = "truncated"
    verifier = receipt.get("independent_verifier")
    verifier = verifier if isinstance(verifier, Mapping) else {}
    failure = receipt.get("terminal_failure")
    category = failure.get("category") if isinstance(failure, Mapping) else None
    if category == "provider_timeout":
        failure_class, owner, sufficient = "provider_timeout", "provider_route", True
        reproduction = "Reproduce at the provider boundary before assigning a CodeGG owner."
    elif verifier.get("tests_run") == 0 and verifier.get("exit_code") not in (None, 0):
        failure_class, owner, sufficient = "verifier_setup_error", "harness_fixture", True
        reproduction = "Repair verifier setup, then rerun the independently frozen test."
    elif receipt.get("task_outcome") == "failed":
        failure_class, owner, sufficient = "task_failure", "agent_task_quality", True
        reproduction = "Do not infer a CodeGG defect; require a separate native failing-first contract test."
    elif trace_state != "complete":
        failure_class, owner, sufficient = "observation_incomplete", "harness_observation", False
        reproduction = "Repeat with a complete bounded trace and reconciled tool/job terminals."
    elif (receipt.get("task_outcome") == "passed" and verifier.get("tests_run", 0) > 0
          and verifier.get("exit_code") == 0):
        failure_class, owner, sufficient = "task_success", "none", True
        reproduction = "No corrective action; the independent verifier passed."
    elif any(row.get("event_type") == "tool_terminal" and row.get("status") == "failed"
             for row in trace_rows):
        failure_class, owner, sufficient = "tool_execution_failure_observed", "tool_owner_unresolved", False
        reproduction = "Reproduce the same native tool contract failure in two independent lanes."
    else:
        failure_class, owner, sufficient = "unknown", "unknown", False
        reproduction = "Collect a complete trace and an independent failing-first reproduction."
    return {"schema": "causal_attribution_report_v1", "observed_failure_class": failure_class,
            "evidence_sufficiency": "sufficient_for_classification" if sufficient else "incomplete",
            "plausible_owners": [owner], "unknowns": ["model intent", "unobserved events"],
            "required_reproduction": reproduction, "trace_state": trace_state,
            "codegg_defect_reproduced": False,
            "observed_tool_failure_count": sum(
                row.get("event_type") == "tool_terminal" and row.get("status") == "failed"
                for row in trace_rows),
            "repair_cycles_observed": sum(
                row.get("event_type") == "repair_cycle" and row.get("detail") == "passed"
                for row in trace_rows),
            "continuation_observed": any(row.get("event_type") == "continuation"
                                         for row in trace_rows)}


def read_trace(path: Path) -> list[dict[str, Any]]:
    if path.stat().st_size > MAX_BYTES:
        raise ValueError("causal trace exceeds its configured byte bound")
    rows: list[dict[str, Any]] = []
    previous = "0" * 64
    for index, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        row = json.loads(line)
        if not isinstance(row, dict) or row.get("schema") != SCHEMA or row.get("seq") != index:
            raise ValueError("causal trace schema or sequence is invalid")
        event_hash = row.pop("event_sha256", None)
        if row.get("prev_sha256") != previous:
            raise ValueError("causal trace hash chain is invalid")
        actual = hashlib.sha256(json.dumps(row, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        row["event_sha256"] = event_hash
        if event_hash != actual:
            raise ValueError("causal trace event digest is invalid")
        previous = actual
        rows.append(row)
    if len(rows) > MAX_EVENTS or path.stat().st_size > MAX_BYTES:
        raise ValueError("causal trace exceeds its configured bounds")
    return rows

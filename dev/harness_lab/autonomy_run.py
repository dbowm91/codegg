"""Run autonomy taxonomy cases as live agent tasks and compare two binaries.

This module is the C004 work package D instrument. The taxonomy in
``dev/harness_lab/fixtures/agent_autonomy/cases.jsonl`` freezes *what* each
case asks and *where* its verifier runs, but nothing yet executed a case as a
live agent task. This module does:

* it copies one case's ``fixture/`` into a private lane workspace and runs the
  case's prompt against a live provider route;
* it keeps the verifier **outside** the agent's workspace root, so the agent
  can neither read the hidden contract nor edit the thing that judges it;
* it records the transcript the agent actually produced — the tool sequence,
  each call's argument digest and each call's terminal status — and hands that
  transcript to the case's own verifier;
* it runs the case's independent verifier and records its exit code, so a
  "solved" case means the verifier confirmed it, never that the agent said so.

Fail-closed contract
--------------------
* The budget is admitted **before** the prompt is submitted, so a run that
  cannot prove an enforceable cap never reaches a provider.
* The agent's editable tree is the case's ``fixture/`` only. The verifier and
  the transcript live in a private sibling directory, outside the workspace
  root the agent's tools are bound to.
* A case that the verifier rejects is never reported as solved, and a case the
  verifier never ran is never reported as solved either.
* The comparison scores two binaries on the same cases with the shared score
  contract. A protected negative control is a guard: its regression rejects
  the candidate whatever else moved.
"""
from __future__ import annotations

import hashlib
import json
import random
import os
import re
import shutil
import stat
import subprocess
import threading
import time
import uuid
from collections.abc import Mapping
from pathlib import Path
from typing import Any, Optional

from . import (autonomy, budget, causal_trace, diagnostics, event_observation, faults,
               h007_results, scoring, supervisor)

from .autonomy import AutonomyCase, AutonomySuite, verify_isolation
from .pilot import _prepare_lane_home, opencode_go_credential
from .supervisor import (AcpClient, HarnessError, HarnessTimeout, HarnessTurnFailure,
                         SubjectSpec, sha256_file)

#: Where the taxonomy's case bodies live, relative to the repository root.
AUTONOMY_ROOT_RELPATH = "dev/autonomy_fixtures"

#: The environment variable that tells a verifier where the case root is.
#: The verifier's own directory is not the case root in a live lane: the
#: verifier is copied to a private directory so the agent cannot read it, and
#: the case root (which holds the agent's editable ``fixture/``) is elsewhere.
CASE_ROOT_ENV = "CODEGG_AUTONOMY_CASE_ROOT"

#: The environment variable that tells a verifier where the transcript is.
TRANSCRIPT_ENV = "CODEGG_AUTONOMY_TRANSCRIPT"

#: The ACP protocol version the harness speaks.
ACP_PROTOCOL_VERSION = 1

#: A case whose verifier never ran is not a solved case. The receipt carries
#: this explicitly instead of treating a missing verifier as a pass.
VERIFIER_NOT_RUN = "not_run"
H006_CASE_ID = "sustained-event-ledger"
H007_CASE_ID = "build-provenance-catalog"
H007_ALLOWED_MODEL_ROUTES = frozenset({
    "opencode_go/muse-spark-1.3-contributor",
    "opencode_go/deepseek-v4.1-flash",
})
H006_INSTALLATION_PREFLIGHT_TIMEOUT_SECONDS = 15.0
H006_H007_VERIFIER_RESERVE_SECONDS = 300.0
H006_MIN_ACTIVE_DURATION_MS = 30 * 60 * 1000
H006_MAX_ACTIVITY_GAP_MS = 5 * 60 * 1000
H007_MIN_ACTIVE_DURATION_MS = H006_MIN_ACTIVE_DURATION_MS
H007_MAX_ACTIVITY_GAP_MS = H006_MAX_ACTIVITY_GAP_MS
H007_CONTINUATION_MAX_BYTES = 64 * 1024
# Keep continuation bounded, but do not let a turn-count cap undercut the
# larger run-wide provider-request, native-call, wall-clock, and disk limits.
H007_MAX_AUTONOMOUS_FOLLOWUP_TURNS = 100
H007_S018_REQUIRED_STAGE_ORDER = (
    "versioned_records", "durable_import", "dependency_index", "provenance_queries",
    "cli_contract", "acceptance_audit", "workspace_qualification",
)
H007_S018_CONTRACT_PHASE_ORDER = (
    "versioned_records", "durable_import", "dependency_index", "provenance_queries",
    "cli_export",
)
H007_S018_REQUIRED_GATE_ORDER = (
    "starter_repair_format", "starter_repair_store", "versioned_records",
    "durable_import", "dependency_index", "release_scale_overlap", "provenance_queries",
    "cli_contract", "acceptance_audit", "bounded_probes", "workspace_qualification",
    "independent_review",
)
H007_S018_REQUIRED_SUCCESS_JOB_LABELS = ("workspace_tests", "clippy")
H007_S018_REQUIRED_REPAIR_JOB_PAIRS = (
    ("catalog_format", "catalog_format_seed", "catalog_format_repair"),
    ("catalog_store", "catalog_store_seed", "catalog_store_repair"),
)
H007_S018_RELEASE_OVERLAP_COMMAND = (
    "cargo test --offline --locked --release -p catalog-index --test corpus"
)
H007_S018_REVIEW_START_ACTIVE_MS = 15 * 60 * 1000
H007_S018_REVIEW_START_MARKER = (
    "The 15-minute independent-review start threshold has been reached."
)
H007_S018_FINAL_REVIEW_START_ACTIVE_MS = 30 * 60 * 1000
H007_S018_FINAL_REVIEW_START_MARKER = (
    "The 30-minute independent-review start threshold has been reached."
)
H007_SUBAGENT_WALL_CLOCK_TIMEOUT_SECS = 1200
H007_S018_PUBLIC_ARTIFACTS = (
    "PUBLIC_ACCEPTANCE_AUDIT.md", "HARNESS_PROBE_REPORT.json", "tools/catalog_probe.py",
)
HARNESS_PRIVATE_ROOT_ENV = "CODEGG_HARNESS_PRIVATE_ROOT"
HARNESS_PRIVATE_CANARY_NAME = "sandbox-canary.txt"


def _h007_review_uses_30_minute_threshold(case_prompt: Optional[str]) -> bool:
    """Identify cases that defer independent review until active minute 30."""
    return bool(
        isinstance(case_prompt, str)
        and "FINAL REVIEW TIMING: Do not spawn the single read-only verifier subtask "
        "at the 15-minute threshold." in case_prompt
    )


def _h007_subagent_config(case_id: str) -> Optional[dict[str, int]]:
    """Give the H007 independent reviewer enough bounded time to finish."""
    if case_id != H007_CASE_ID:
        return None
    return {"wall_clock_timeout_secs": H007_SUBAGENT_WALL_CLOCK_TIMEOUT_SECS}


__all__ = [
    "AUTONOMY_ROOT_RELPATH",
    "H007_ALLOWED_MODEL_ROUTES",
    "build_transcript",
    "case_root",
    "compare_runs",
    "run_case",
    "run_suite",
]


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def case_root(repository: Path, case_id: str) -> Path:
    """Return the in-repo case root that holds ``fixture/`` and ``verifier/``."""
    return repository / AUTONOMY_ROOT_RELPATH / case_id


def _validate_h007_model_route(model: str) -> None:
    if model not in H007_ALLOWED_MODEL_ROUTES:
        raise HarnessError(
            "H007 model route is not in the explicit qualification allowlist")


def _copy_tree(source: Path, destination: Path) -> None:
    """Copy a directory tree, refusing to follow a symlink out of the source."""
    if destination.exists():
        shutil.rmtree(destination)
    shutil.copytree(source, destination, symlinks=False)


def build_transcript(case: AutonomyCase, tool_calls: list[dict[str, Any]], *,
                     terminal: str, stop_reason: Optional[str],
                     failure: Optional[dict[str, Any]] = None,
                     fault: Optional[dict[str, Any]] = None) -> dict[str, Any]:
    """Return the transcript a case's verifier reads.

    The transcript records what the agent **did**, not what it said. The
    harness projects the tool name and a digest of the tool's arguments, never
    the arguments themselves, so a verifier can tell "the same call again"
    from "a different call" without reading the call's payload.

    ``fault`` records the fault the case declared and what the transcript
    shows about it. It is ``None`` for a case that declared no fault, and a
    fault that was declared but never observed is recorded as unexercised
    rather than left out.
    """
    return {
        "schema_version": (3 if case.case_id == H006_CASE_ID else
                           4 if case.case_id == H007_CASE_ID else 1),
        "case_id": case.case_id,
        "family": case.family,
        "group_id": case.group_id,
        "protected": case.protected,
        "terminal": terminal,
        "stop_reason": stop_reason,
        "failure": failure,
        "fault": fault,
        "tool_calls": tool_calls,
    }


class _ToolCallProjector:
    """Project the ACP notification stream into ordered tool-call records.

    A tool call is reported by two ACP notifications: a ``tool_call`` start
    that names the tool and carries the argument digest, and a
    ``tool_call_update`` finish that carries the terminal status. The start
    without a finish means the call never completed, and its status stays
    unknown rather than being reported as a success.

    The product publishes a **failed** tool as a successful call whose output
    carries the error, so the ACP status alone cannot tell a failed tool from
    a successful one. The product's error envelope is read out of the reported
    output and carried alongside the status as a boolean, so the observation
    of a fault is possible at all. The output itself is never carried into the
    receipt: only the boolean is.

    The projection is incremental so a live turn can be watched while it runs:
    the records are the same whether the stream is read at the end of the turn
    or while the turn is still going.
    """

    def __init__(self) -> None:
        self._started: dict[str, dict[str, Any]] = {}
        self._order: list[str] = []
        self._finished: dict[str, str] = {}
        self._elapsed: dict[str, int] = {}
        self._job: dict[str, dict[str, Any]] = {}
        self._tool_error_classes: dict[str, str] = {}
        self._placement_accepted: set[str] = set()
        self._task_run_statuses: dict[str, str] = {}
        self._task_spawn_ids_present: set[str] = set()
        self._task_spawn_metadata: dict[str, dict[str, str]] = {}
        self._task_result_metadata: dict[str, dict[str, str]] = {}
        self._task_failure_classes: dict[str, str] = {}

    def absorb(self, notification: dict[str, Any]) -> None:
        """Absorb one ACP notification."""
        if notification.get("method") != "session/update":
            return
        params = notification.get("params", {})
        update = params.get("update", {}) if isinstance(params, dict) else {}
        if not isinstance(update, dict):
            return
        kind = update.get("sessionUpdate")
        if kind == "tool_call":
            call_id = update.get("toolCallId")
            if not isinstance(call_id, str):
                return
            if call_id not in self._started:
                self._order.append(call_id)
            title = update.get("title")
            digest = update.get("codeggArgumentsSha256")
            self._started[call_id] = {
                "tool": title if isinstance(title, str) else "unknown",
                "arguments_sha256": digest if isinstance(digest, str) else None,
                "started_elapsed_ms": notification.get("_received_elapsed_ms"),
                "action": update.get("codeggAction") if update.get("codeggAction") in
                          {"submit", "status", "wait", "output", "cancel", "spawn"} else None,
                "job_id_argument_state": update.get("codeggJobIdArgumentState")
                if title == "job" and update.get("codeggJobIdArgumentState") in {
                    "invalid_json", "not_object", "missing", "non_string",
                    "empty_string", "string",
                } else None,
                "job_id_argument_sha256": update.get("codeggJobIdArgumentSha256")
                if title == "job" and _is_sha256_digest(
                    update.get("codeggJobIdArgumentSha256")) else None,
                "task_action": update.get("codeggAction")
                if title == "task" and update.get("codeggAction")
                in {"spawn", "get", "status", "wait", "continue"} else None,
                "task_id_sha256": update.get("codeggTaskIdSha256")
                if title == "task" and _is_sha256_digest(update.get("codeggTaskIdSha256"))
                else None,
                "task_run_id_sha256": update.get("codeggTaskRunIdSha256")
                if title == "task"
                and _is_sha256_digest(update.get("codeggTaskRunIdSha256")) else None,
                "task_placement": update.get("codeggTaskPlacement"),
            }
        elif kind == "tool_call_update":
            call_id = update.get("toolCallId")
            status = update.get("status")
            if isinstance(call_id, str) and isinstance(status, str):
                self._finished[call_id] = status
                elapsed = notification.get("_received_elapsed_ms")
                if isinstance(elapsed, int):
                    self._elapsed[call_id] = elapsed
                title = self._started.get(call_id, {}).get("tool")
                action = self._started.get(call_id, {}).get("action")
                error_class = update.get("codeggToolErrorClass")
                if (title == "test" and action == "submit" and status == faults.STATUS_FAILED
                        and isinstance(error_class, str)
                        and error_class in diagnostics.SAFE_TEST_SUBMIT_ERROR_CLASSES):
                    self._tool_error_classes[call_id] = error_class
                raw_output = update.get("rawOutput")
                if title in {"job", "test"} and isinstance(raw_output, str):
                    job = _project_job_result(raw_output, title,
                                              self._started.get(call_id, {}).get("action"))
                    if job is not None:
                        self._job[call_id] = job
                spawn_metadata = _safe_task_identity_metadata(update.get("codeggTaskSpawn"))
                if title == "task" and spawn_metadata is not None:
                    self._task_spawn_metadata[call_id] = spawn_metadata
                result_metadata = _safe_task_identity_metadata(update.get("codeggTaskResult"))
                if title == "task" and result_metadata is not None:
                    self._task_result_metadata[call_id] = result_metadata
            if isinstance(call_id, str) and update.get("codeggPlacementAccepted") is True:
                self._placement_accepted.add(call_id)
            if (isinstance(call_id, str)
                    and self._started.get(call_id, {}).get("tool") == "task"):
                raw_output = update.get("rawOutput")
                status = _safe_task_run_status(raw_output)
                if status is not None:
                    self._task_run_statuses[call_id] = status
                failure_class = diagnostics.extract_task_failure_class(raw_output)
                if failure_class is not None:
                    self._task_failure_classes[call_id] = failure_class
                if (self._started.get(call_id, {}).get("action") == "spawn"
                        and _safe_task_spawn_ids_present(raw_output)):
                    self._task_spawn_ids_present.add(call_id)

    def calls(self) -> list[dict[str, Any]]:
        """Return the projected tool calls, in the order they started."""
        calls = []
        for index, call_id in enumerate(self._order):
            started = self._started[call_id]
            call = {
                "index": index,
                "tool": started["tool"],
                "arguments_sha256": started["arguments_sha256"],
                "status": self._finished.get(call_id, "unknown"),
                "started_elapsed_ms": started.get("started_elapsed_ms"),
                "finished_elapsed_ms": self._elapsed.get(call_id),
                # ACP terminal status is typed evidence. Error-like output text
                # may be ordinary file content and never changes the outcome.
                "failed": self._finished.get(call_id) == faults.STATUS_FAILED,
            }
            error_class = self._tool_error_classes.get(call_id)
            if error_class is not None:
                call["tool_error_class"] = error_class
            job = self._job.get(call_id)
            if job is not None:
                # The verifier treats an omitted job field as an empty mapping.
                # Preserve that shape for non-job tools; job/test calls with
                # unrecognized output still carry explicit null metadata.
                call["job"] = job
            elif started["tool"] in {"job", "test"}:
                call["job"] = {
                    "job_id": None,
                    "action": started.get("action")
                    or ("submit" if started["tool"] == "test" else "unknown"),
                    "status": None,
                }
            if started["tool"] == "job":
                call["job_id_argument_state"] = (
                    started.get("job_id_argument_state") or "unobserved"
                )
                action = started.get("action")
                if action in {"status", "wait", "output", "cancel"}:
                    argument_state = started.get("job_id_argument_state")
                    argument_digest = started.get("job_id_argument_sha256")
                    result_id = job.get("job_id") if isinstance(job, Mapping) else None
                    if argument_state != "string":
                        match_state = "mismatched"
                    elif argument_digest is None:
                        match_state = "unobserved"
                    elif not isinstance(result_id, str) or _JOB_ID_PATTERN.fullmatch(
                            result_id) is None:
                        match_state = "unobserved"
                    else:
                        canonical_result_id = str(uuid.UUID(result_id))
                        result_digest = hashlib.sha256(
                            canonical_result_id.encode("utf-8")
                        ).hexdigest()
                        match_state = ("matched" if result_digest == argument_digest
                                       else "mismatched")
                    call["job_id_argument_matches_result"] = match_state
                else:
                    call["job_id_argument_matches_result"] = "not_applicable"
            if started["task_placement"] is not None:
                call["task_placement"] = started["task_placement"]
            if started["tool"] == "task" and started.get("task_action") is not None:
                call["task_action"] = started["task_action"]
                if started["task_id_sha256"] is not None:
                    call["task_id_sha256"] = started["task_id_sha256"]
                if started["task_run_id_sha256"] is not None:
                    call["task_run_id_sha256"] = started["task_run_id_sha256"]
                if started["task_action"] == "spawn":
                    call["task_spawn_ids_present"] = call_id in self._task_spawn_ids_present
            spawn_metadata = self._task_spawn_metadata.get(call_id)
            if spawn_metadata is not None:
                call["task_spawn_task_id_sha256"] = spawn_metadata["task_id_sha256"]
                call["task_spawn_run_id_sha256"] = spawn_metadata["run_id_sha256"]
                call["task_spawn_run_status"] = spawn_metadata["run_status"]
                if spawn_metadata.get("agent") is not None:
                    call["task_spawn_agent"] = spawn_metadata["agent"]
            result_metadata = self._task_result_metadata.get(call_id)
            if result_metadata is not None:
                call["task_result_task_id_sha256"] = result_metadata["task_id_sha256"]
                call["task_result_run_id_sha256"] = result_metadata["run_id_sha256"]
                call["task_result_run_status"] = result_metadata["run_status"]
            if call_id in self._placement_accepted:
                call["placement_accepted"] = True
            if call_id in self._task_run_statuses:
                call["task_run_status"] = self._task_run_statuses[call_id]
            if call_id in self._task_failure_classes:
                call["task_failure_class"] = self._task_failure_classes[call_id]
            calls.append(call)
        return calls


def _safe_task_run_status(output: Any) -> Optional[str]:
    """Extract only an allowlisted durable run status from task output."""
    if not isinstance(output, str):
        return None
    allowed = {"created", "queued", "preparing", "running", "waiting",
               "cancelling", "completed", "failed", "interrupted", "cancelled"}
    for line in output.splitlines():
        candidate = None
        if line.startswith("Status: "):
            candidate = line.removeprefix("Status: ").strip()
        elif line.startswith("Run is terminal: "):
            candidate = line.removeprefix("Run is terminal: ").strip()
        if candidate in allowed:
            return candidate
    return None


def _is_sha256_digest(value: Any) -> bool:
    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None


def _safe_task_identity_metadata(value: Any) -> Optional[dict[str, str]]:
    """Accept typed ACP task identity, status, and the verifier role only."""
    if not isinstance(value, Mapping):
        return None
    task_digest = value.get("task_id_sha256")
    run_digest = value.get("run_id_sha256")
    status = value.get("run_status")
    agent = value.get("agent")
    allowed = {"created", "queued", "preparing", "running", "waiting",
               "cancelling", "completed", "failed", "interrupted", "cancelled"}
    if (not _is_sha256_digest(task_digest) or not _is_sha256_digest(run_digest)
            or not isinstance(status, str) or status not in allowed):
        return None
    if agent is not None and (not isinstance(agent, str)
                              or agent not in {"verifier", "other"}):
        return None
    metadata = {"task_id_sha256": task_digest, "run_id_sha256": run_digest,
                "run_status": status}
    if isinstance(agent, str):
        metadata["agent"] = agent
    return metadata


def _safe_task_spawn_ids_present(output: Any) -> bool:
    """Record only whether a native task spawn returned both opaque IDs."""
    if not isinstance(output, str):
        return False
    if not re.search(r"(?m)^Task queued\s*$", output):
        return False
    identity = r"([A-Za-z0-9_-]{1,128})"
    task_ids = re.findall(rf"(?m)^Task:\s*{identity}\s*$", output)
    run_ids = re.findall(rf"(?m)^Run:\s*{identity}\s*$", output)
    if len(task_ids) != 1 or len(run_ids) != 1:
        return False
    return task_ids[0] != run_ids[0]


class _ProcessTreeSampler:
    """Sample bounded host CPU and resident memory for one ACP process tree."""

    def __init__(self, root_pid: int, interval: float = 0.5) -> None:
        self.root_pid = root_pid
        self.interval = interval
        self.stop_event = threading.Event()
        self.peak_rss_bytes = 0
        self.cpu_seconds = 0.0
        self.samples = 0
        self._thread = threading.Thread(target=self._sample_loop, daemon=True)
        self._thread.start()

    def _sample_loop(self) -> None:
        previous = time.monotonic()
        while not self.stop_event.wait(self.interval):
            try:
                result = subprocess.run(
                    ["ps", "-axo", "pid=,ppid=,%cpu=,rss="],
                    capture_output=True, text=True, timeout=2.0, check=False)
                if result.returncode != 0:
                    continue
                rows: dict[int, tuple[int, float, int]] = {}
                for line in result.stdout.splitlines():
                    fields = line.split()
                    if len(fields) != 4:
                        continue
                    try:
                        pid, ppid = int(fields[0]), int(fields[1])
                        cpu, rss_kib = float(fields[2]), int(fields[3])
                    except ValueError:
                        continue
                    rows[pid] = (ppid, cpu, rss_kib)
                tree = {self.root_pid}
                changed = True
                while changed:
                    changed = False
                    for pid, (ppid, _cpu, _rss) in rows.items():
                        if ppid in tree and pid not in tree:
                            tree.add(pid)
                            changed = True
                members = [rows[pid] for pid in tree if pid in rows]
                if not members:
                    continue
                now = time.monotonic()
                elapsed = max(0.0, now - previous)
                previous = now
                cpu_percent = sum(max(0.0, row[1]) for row in members)
                rss_bytes = sum(max(0, row[2]) for row in members) * 1024
                self.cpu_seconds += cpu_percent / 100.0 * elapsed
                self.peak_rss_bytes = max(self.peak_rss_bytes, rss_bytes)
                self.samples += 1
            except (OSError, subprocess.SubprocessError):
                continue

    def finish(self) -> dict[str, Any]:
        self.stop_event.set()
        self._thread.join(timeout=2.5)
        return {
            "cpu_seconds": round(self.cpu_seconds, 3) if self.samples else None,
            "peak_rss_bytes": self.peak_rss_bytes if self.samples else None,
            "sample_count": self.samples,
        }


def _directory_size_bytes(root: Path) -> int:
    total = 0
    if not root.exists():
        return total
    for current, _directories, filenames in os.walk(root, followlinks=False):
        for filename in filenames:
            try:
                path = Path(current) / filename
                if not path.is_symlink():
                    total += path.stat().st_size
            except OSError:
                continue
    return total


def _nearest_existing_directory(path: Path) -> Path:
    """Find an existing directory for a disk-usage probe of a not-yet-created lane."""
    candidate = path
    while not candidate.is_dir():
        parent = candidate.parent
        if parent == candidate:
            return candidate
        candidate = parent
    return candidate


_JOB_ID_PATTERN = re.compile(r"\b([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\b", re.I)


def _project_job_result(raw_output: str, tool: str,
                        action: Optional[str]) -> Optional[dict[str, Any]]:
    """Extract bounded job metadata without retaining tool output."""
    job_id = _JOB_ID_PATTERN.search(raw_output)
    status = re.search(r"\bjob\s+(queued|running|completed|failed|cancelled|canceled|timed_out|still-running)\b", raw_output, re.I)
    if tool not in {"job", "test"}:
        return None
    return {
        "job_id": job_id.group(1).lower() if job_id else None,
        "action": action or ("submit" if tool == "test" else "unknown"),
        "status": status.group(1).lower() if status else None,
    }


class _DiskFloorWatcher:
    def __init__(self, path: Path, floor_bytes: int, client: AcpClient,
                 session_id: str, *, poll_seconds: float = 1.0) -> None:
        self.path = path
        self.floor_bytes = floor_bytes
        self.client = client
        self.session_id = session_id
        self.poll_seconds = poll_seconds
        self.stop_event = threading.Event()
        self.thread: Optional[threading.Thread] = None
        self.free_bytes: Optional[int] = None
        self.minimum_free_bytes: Optional[int] = None
        self.triggered = False
        self.error: Optional[str] = None

    def start(self) -> None:
        self._sample()
        if self.error is not None or self.triggered:
            self.client.notify("session/cancel", {"sessionId": self.session_id})
            return
        self.thread = threading.Thread(target=self._run, name="harness-disk-floor", daemon=True)
        self.thread.start()

    def stop(self) -> None:
        self.stop_event.set()
        if self.thread is not None:
            self.thread.join(timeout=2.0)

    def _sample(self) -> None:
        try:
            self.free_bytes = shutil.disk_usage(self.path).free
            if self.minimum_free_bytes is None or self.free_bytes < self.minimum_free_bytes:
                self.minimum_free_bytes = self.free_bytes
            if self.free_bytes < self.floor_bytes:
                self.triggered = True
        except OSError as error:
            self.error = type(error).__name__

    def _run(self) -> None:
        while not self.stop_event.wait(self.poll_seconds):
            self._sample()
            if self.error is not None or self.triggered:
                try:
                    self.client.notify("session/cancel", {"sessionId": self.session_id})
                except (HarnessError, OSError, BrokenPipeError):
                    pass
                return


class _ActivityStallWatcher:
    """Cancel H007 if no ACP tool activity is observed within its stall bound."""

    def __init__(self, client: AcpClient, session_id: str, stall_seconds: int, *,
                 poll_seconds: float = 0.25) -> None:
        self.client = client
        self.session_id = session_id
        self.stall_seconds = stall_seconds
        self.poll_seconds = poll_seconds
        self.stop_event = threading.Event()
        self.thread: Optional[threading.Thread] = None
        self.triggered = False
        self.cancel_error: Optional[str] = None
        self._last_activity = time.monotonic()
        self._absorbed = 0

    def start(self) -> None:
        self.thread = threading.Thread(target=self._run, name="h007-activity-stall",
                                       daemon=True)
        self.thread.start()

    def stop(self) -> None:
        self.stop_event.set()
        if self.thread is not None:
            self.thread.join(timeout=2.0)
            self.thread = None

    def _run(self) -> None:
        while not self.stop_event.wait(self.poll_seconds):
            notifications = self.client.notifications
            for notification in notifications[self._absorbed:]:
                if self._is_tool_activity(notification):
                    self._last_activity = time.monotonic()
                self._absorbed += 1
            if time.monotonic() - self._last_activity < self.stall_seconds:
                continue
            self.triggered = True
            try:
                self.client.notify("session/cancel", {"sessionId": self.session_id})
            except (HarnessError, OSError, BrokenPipeError) as error:
                self.cancel_error = type(error).__name__
            return

    @staticmethod
    def _is_tool_activity(notification: dict[str, Any]) -> bool:
        """Count only tool-call boundaries as activity for sustained work.

        Message chunks and other session updates can continue while the agent
        makes no tool progress. Resetting the stall clock for those updates
        allowed a silent tool gap to exceed the same limit used by qualification.
        """
        if notification.get("method") != "session/update":
            return False
        params = notification.get("params", {})
        update = params.get("update", {}) if isinstance(params, dict) else {}
        return isinstance(update, dict) and update.get("sessionUpdate") in {
            "tool_call", "tool_call_update"
        }


def _observed_tool_calls(client: AcpClient) -> list[dict[str, Any]]:
    """Return the observed tool calls in order.

    A tool call is reported by two ACP notifications: a ``tool_call`` start
    that names the tool and carries the argument digest, and a
    ``tool_call_update`` finish that carries the terminal status. The start
    without a finish means the call never completed, and its status stays
    unknown rather than being reported as a success.
    """
    projector = _ToolCallProjector()
    for notification in client.notifications:
        projector.absorb(notification)
    return projector.calls()


class _FaultWatcher:
    """Stop a faulted trial once the declared bound of identical failures is met.

    The watcher reads the same notification stream the transcript is built
    from, so the bound it enforces and the transcript the verifier reads are
    two views of one record. It never decides whether the control held: it
    only stops a trial that has already failed the same way enough times, so a
    faulted case is bounded even when the agent never gives up.
    """

    def __init__(self, client: AcpClient, spec: faults.FaultSpec, session_id: str, *,
                 applied: Optional[faults.AppliedFault] = None,
                 poll_seconds: float = 0.05) -> None:
        self._client = client
        self._spec = spec
        self._session_id = session_id
        self._applied = applied
        self._poll_seconds = poll_seconds
        self._projector = _ToolCallProjector()
        self._absorbed = 0
        self._stop = threading.Event()
        self._thread: Optional[threading.Thread] = None
        self.bound_reached = False
        self.reapplied = 0
        self.cancel_error: Optional[str] = None

    def start(self) -> None:
        """Begin watching the turn."""
        self._thread = threading.Thread(target=self._run, name="harness-fault-watcher",
                                        daemon=True)
        self._thread.start()

    def stop(self) -> None:
        """Stop watching and absorb whatever the stream carried meanwhile."""
        self._stop.set()
        if self._thread is not None:
            self._thread.join(timeout=5.0)
            self._thread = None
        self._absorb()

    def observed_failures(self) -> int:
        """Return how many times the faulted call has failed so far."""
        return int(faults.observe_fault(self._projector.calls(), self._spec)["observed_failures"])

    def _run(self) -> None:
        while not self._stop.is_set():
            self._absorb()
            self._hold()
            if self.observed_failures() >= self._spec.max_consecutive_failures:
                self._cancel()
                return
            self._stop.wait(self._poll_seconds)
        self._absorb()

    def _hold(self) -> None:
        """Put the fault back in force if the agent undid it."""
        if self._applied is None:
            return
        try:
            if self._applied.reapply():
                self.reapplied += 1
        except OSError:
            # A fault that cannot be held is reported by the receipt as a
            # failed observation, not by crashing the trial.
            return

    def _absorb(self) -> None:
        notifications = self._client.notifications
        while self._absorbed < len(notifications):
            self._projector.absorb(notifications[self._absorbed])
            self._absorbed += 1

    def _cancel(self) -> None:
        try:
            self._client.notify("session/cancel", {"sessionId": self._session_id})
        except Exception as error:  # noqa: BLE001 - the failure is recorded, not raised
            self.cancel_error = f"{type(error).__name__}: {error}"
            return
        self.bound_reached = True


def _run_verifier(command: list[str], cwd: Path, env: dict[str, str],
                   timeout: float, *, diagnostics_only: bool = False) -> dict[str, Any]:
    """Run a case's independent verifier and summarize the outcome."""
    started = time.monotonic()
    try:
        completed = subprocess.run(command, cwd=str(cwd), capture_output=True, text=True,
                                   timeout=timeout, check=False, env=env)
    except subprocess.TimeoutExpired as error:
        partial_output = "\n".join(
            part.decode("utf-8", errors="replace") if isinstance(part, bytes)
            else part if isinstance(part, str) else ""
            for part in (error.stdout, error.stderr)
        )
        diagnostic = diagnostics.summarize_unittest_output(
            partial_output, exit_code=None, timed_out=True)
        if not diagnostics_only:
            return {"command": list(command), "cwd": str(cwd), "exit_code": None,
                    "timed_out": True,
                    "duration_ms": int((time.monotonic() - started) * 1000),
                    "output_tail": "", "tests_run": None, "failure_summary": None}
        return {"command": list(command), "cwd": str(cwd), "exit_code": None,
                "timed_out": True, "duration_ms": int((time.monotonic() - started) * 1000),
                "tests_run": diagnostic["tests_run"], "diagnostic": diagnostic}
    except OSError as error:
        diagnostic = diagnostics.summarize_unittest_output(
            "", exit_code=None, runner_error_class=type(error).__name__)
        if not diagnostics_only:
            return {"command": list(command), "cwd": str(cwd), "exit_code": None,
                    "timed_out": False,
                    "duration_ms": int((time.monotonic() - started) * 1000),
                    "output_tail": f"{type(error).__name__}: {error}",
                    "tests_run": None, "failure_summary": None}
        return {"command": list(command), "cwd": str(cwd), "exit_code": None,
                "timed_out": False, "duration_ms": int((time.monotonic() - started) * 1000),
                "tests_run": None, "diagnostic": diagnostic}
    output = (completed.stdout or "") + (completed.stderr or "")
    diagnostic = diagnostics.summarize_unittest_output(output, exit_code=completed.returncode)
    if not diagnostics_only:
        tests_run = diagnostic["tests_run"]
        failure_summary = None
        if completed.returncode != 0:
            for label in ("failures", "errors"):
                if f"{label}=" in output:
                    failure_summary = next(part for part in output.split()
                                           if f"{label}=" in part).strip()
                    break
        tail = "\n".join(output.splitlines()[-12:])
        return {"command": list(command), "cwd": str(cwd),
                "exit_code": completed.returncode, "timed_out": False,
                "duration_ms": int((time.monotonic() - started) * 1000),
                "output_tail": tail, "tests_run": tests_run,
                "failure_summary": failure_summary}
    return {"command": list(command), "cwd": str(cwd), "exit_code": completed.returncode,
            "timed_out": False, "duration_ms": int((time.monotonic() - started) * 1000),
            "tests_run": diagnostic["tests_run"], "diagnostic": diagnostic}


def _turn_timeout(case_id: str, requested_timeout: float, case_timeout: float,
                  remaining_seconds: Optional[float]) -> float:
    """Bound one agent turn while reserving enough time for its verifier."""
    if remaining_seconds is None:
        return min(requested_timeout, case_timeout)
    if case_id in {H006_CASE_ID, H007_CASE_ID}:
        # H006/H007 require sustained active work, but the verifier must still
        # run inside the same hard deadline. Recompute this on every prompt:
        # normal end_turn follow-ups share one case budget and must not consume
        # the reserve that an earlier prompt left behind.
        available = max(0.0, remaining_seconds - H006_H007_VERIFIER_RESERVE_SECONDS)
    else:
        # Divide suite time across the turn, verifier, lane setup, and cleanup.
        available = max(1.0, remaining_seconds / 3.0)
    return min(requested_timeout, case_timeout, available)


def _verifier_timeout(case_id: str, turn_timeout: float,
                      remaining_seconds: Optional[float]) -> float:
    """Keep H006's independent verifier inside the remaining hard deadline."""
    if case_id not in {H006_CASE_ID, H007_CASE_ID} or remaining_seconds is None:
        return turn_timeout
    return min(turn_timeout, max(1.0, remaining_seconds))


def _duration_qualified(case_id: str, duration_ms: int,
                        tool_calls: list[dict[str, Any]]) -> bool:
    """Require sustained H006 activity through its declared 30-minute minimum."""
    if case_id not in {H006_CASE_ID, H007_CASE_ID}:
        return True
    activity_times = _activity_elapsed_ms(tool_calls)
    finished_times = [
        call["finished_elapsed_ms"] for call in tool_calls
        if isinstance(call.get("finished_elapsed_ms"), int)
    ]
    if not activity_times or not finished_times:
        return False
    first_activity_ms = min(activity_times)
    last_activity_ms = max(activity_times)
    active_work_span_ms = last_activity_ms - first_activity_ms
    maximum_activity_gap_ms = _maximum_activity_gap_ms(duration_ms, tool_calls)
    minimum = (H007_MIN_ACTIVE_DURATION_MS if case_id == H007_CASE_ID
               else H006_MIN_ACTIVE_DURATION_MS)
    maximum_gap = (H007_MAX_ACTIVITY_GAP_MS if case_id == H007_CASE_ID
                   else H006_MAX_ACTIVITY_GAP_MS)
    return duration_ms >= minimum \
        and max(finished_times) >= minimum \
        and active_work_span_ms >= minimum \
        and maximum_activity_gap_ms <= maximum_gap


def _activity_elapsed_ms(tool_calls: list[dict[str, Any]]) -> list[int]:
    """Return sorted start/finish observations for the session's projected tool calls."""
    return sorted(
        elapsed_ms
        for call in tool_calls
        for elapsed_ms in (call.get("started_elapsed_ms"), call.get("finished_elapsed_ms"))
        if isinstance(elapsed_ms, int) and not isinstance(elapsed_ms, bool)
    )


def _maximum_activity_gap_ms(duration_ms: int,
                             tool_calls: list[dict[str, Any]]) -> int:
    """Measure gaps between tool events, including inactivity at turn end."""
    activity_times = _activity_elapsed_ms(tool_calls)
    if not activity_times:
        return 0
    gaps = [later - earlier for earlier, later in zip(activity_times, activity_times[1:])]
    if duration_ms > activity_times[-1]:
        gaps.append(duration_ms - activity_times[-1])
    return max(gaps, default=0)


def _active_work_span_ms(tool_calls: list[dict[str, Any]]) -> int:
    """Return the elapsed span between the first and last observed tool calls."""
    activity_times = [
        call.get("finished_elapsed_ms") for call in tool_calls
        if isinstance(call.get("finished_elapsed_ms"), int)
    ]
    return max(activity_times) - min(activity_times) if activity_times else 0


def _h007_repair_cycles(tool_calls: list[dict[str, Any]], *,
                        preferred_job_pairs: tuple[tuple[str, str], ...] = ()) -> int:
    """Count one-to-one failed-test → productive-repair → completed-test pairs."""
    return len(h007_results._observed_repair_cycle_pairs(
        tool_calls, preferred_job_pairs=preferred_job_pairs,
    ))


def _h007_read_continuation(fixture_root: Path) -> Optional[str]:
    """Read the bounded public continuation file without following a symlink."""
    path = fixture_root / "CONTINUATION.md"
    try:
        root = fixture_root.resolve(strict=True)
        if path.is_symlink():
            return None
        resolved = path.resolve(strict=True)
        if resolved.parent != root:
            return None
        descriptor = os.open(resolved, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
        try:
            metadata = os.fstat(descriptor)
            if (not stat.S_ISREG(metadata.st_mode)
                    or metadata.st_size > H007_CONTINUATION_MAX_BYTES):
                return None
            chunks = bytearray()
            while len(chunks) <= H007_CONTINUATION_MAX_BYTES:
                chunk = os.read(descriptor, min(8192, H007_CONTINUATION_MAX_BYTES + 1
                                                - len(chunks)))
                if not chunk:
                    break
                chunks.extend(chunk)
            if len(chunks) > H007_CONTINUATION_MAX_BYTES:
                return None
            return chunks.decode("utf-8")
        finally:
            os.close(descriptor)
    except (OSError, UnicodeError, ValueError):
        return None


def _set_harness_private_root_env(env: dict[str, str], private_root: Path) -> None:
    """Tell CodeGG's deny-only sandbox input where this run's private files live."""
    env[HARNESS_PRIVATE_ROOT_ENV] = str(private_root.resolve())


def _owned_lane_daemon_process_count(spec: supervisor.SubjectSpec) -> Optional[int]:
    """Return only the number of lane-owned daemons remaining after cleanup."""
    try:
        return len(supervisor._owned_daemon_pids(spec))
    except (OSError, subprocess.SubprocessError, ValueError):
        return None


def _seed_private_root_canary(private_root: Path) -> Path:
    """Create a harmless public canary, separate from all run-private evidence."""
    canary = private_root / HARNESS_PRIVATE_CANARY_NAME
    canary.write_text("CodeGG private-root denial canary only.\n", encoding="utf-8")
    return canary


def _seed_private_root_canary_link(fixture_root: Path, private_root: Path) -> Path:
    """Seed a known workspace symlink for a native sandbox read-denial probe.

    The agent receives only the workspace-relative link path. The private
    target is never put in its shell environment, which is intentionally
    sanitized by the managed-process layer.
    """
    link = fixture_root / ".harness-private-canary-link"
    if link.exists() or link.is_symlink():
        raise HarnessError("the private-root canary link path already exists")
    target = (private_root / HARNESS_PRIVATE_CANARY_NAME).resolve(strict=True)
    link.symlink_to(target)
    return link


def _h007_documented_phases(continuation: Optional[str]) -> list[str]:
    if continuation is None:
        return []
    return h007_results.documented_phases(continuation) or []


def _h007_continuation_confirmed(tool_calls: list[dict[str, Any]],
                                 fixture_root: Path, *,
                                 require_gate_block: bool = False,
                                 case_prompt: Optional[str] = None) -> bool:
    """Validate bounded, public continuation evidence against the native trace."""
    continuation = _h007_read_continuation(fixture_root)
    if continuation is None:
        return False
    prompt_contract = _h007_case_requires_gate_block(case_prompt)
    valid = h007_results.continuation_document_confirmed(
        continuation,
        tool_calls,
        require_gate_block=(require_gate_block or prompt_contract),
        required_phases=(_h007_required_phases(case_prompt) if prompt_contract else None),
        required_contract_phases=(
            _h007_required_contract_phases(case_prompt) if prompt_contract else None),
        required_stages=_h007_required_stages(case_prompt),
        required_gates=(_h007_required_gates(case_prompt) if prompt_contract else None),
        required_job_label="overlap" if prompt_contract else None,
        required_job_command=(H007_S018_RELEASE_OVERLAP_COMMAND if prompt_contract else None),
        required_success_job_labels=(
            H007_S018_REQUIRED_SUCCESS_JOB_LABELS if prompt_contract else None
        ),
        required_repair_job_pairs=(
            H007_S018_REQUIRED_REPAIR_JOB_PAIRS
            if _h007_case_requires_repair_job_pairs(case_prompt) else None
        ),
        require_all_test_jobs_terminal=prompt_contract,
    )
    if not valid or not prompt_contract:
        return valid
    if not _h007_required_public_artifacts_present(fixture_root):
        return False
    return _h007_required_review_observed(continuation, tool_calls)


def _h007_case_requires_gate_block(case_prompt: Optional[str]) -> bool:
    """Keep the explicit S018 final-checkpoint ledger part of completion proof."""
    return (isinstance(case_prompt, str)
            and "FINAL CHECKPOINT" in case_prompt
            and "Keep the seven required stages named exactly" in case_prompt)


def _h007_case_requires_repair_job_pairs(case_prompt: Optional[str]) -> bool:
    """Enable the named repair-pair contract only for manifests that specify it."""
    return (isinstance(case_prompt, str)
            and all(marker in case_prompt for marker in (
                "job_id_catalog_format_seed", "job_id_catalog_format_repair",
                "job_id_catalog_store_seed", "job_id_catalog_store_repair",
            )))


def _h007_case_uses_separate_stage_ledger(case_prompt: Optional[str]) -> bool:
    """Detect prompts that preserve the public H007 phase list and split contract phases."""
    return (_h007_case_requires_gate_block(case_prompt)
            and "contract_phases" in case_prompt
            and "required_stages" in case_prompt)


def _h007_required_phases(case_prompt: Optional[str]) -> tuple[str, ...]:
    """Return the H007 phase order used by the frozen public continuation format."""
    if _h007_case_uses_separate_stage_ledger(case_prompt):
        return H007_S018_REQUIRED_STAGE_ORDER
    if _h007_case_requires_gate_block(case_prompt):
        return H007_S018_REQUIRED_STAGE_ORDER
    return h007_results.H007_REQUIRED_CONTINUATION_PHASE_ORDER


def _h007_required_contract_phases(case_prompt: Optional[str]) -> Optional[tuple[str, ...]]:
    """Return contract.json's implementation phase order when it is tracked separately."""
    if _h007_case_uses_separate_stage_ledger(case_prompt):
        return H007_S018_CONTRACT_PHASE_ORDER
    return None


def _h007_required_stages(case_prompt: Optional[str]) -> Optional[tuple[str, ...]]:
    """Return the separate H007 task-stage order for contract-ledger prompts."""
    if _h007_case_uses_separate_stage_ledger(case_prompt):
        return H007_S018_REQUIRED_STAGE_ORDER
    return None


def _h007_required_gates(case_prompt: Optional[str]) -> tuple[str, ...]:
    """Use the separate gate names defined by the live S018 prompt."""
    if _h007_case_requires_gate_block(case_prompt):
        return H007_S018_REQUIRED_GATE_ORDER
    return h007_results.H007_REQUIRED_CONTINUATION_GATE_ORDER


def _h007_public_artifact_status(fixture_root: Path) -> dict[str, bool]:
    """Report bounded presence/shape checks without retaining public artifact contents."""
    status = {name: False for name in H007_S018_PUBLIC_ARTIFACTS}
    try:
        root = fixture_root.resolve(strict=True)
    except (OSError, RuntimeError):
        return status
    for relative in H007_S018_PUBLIC_ARTIFACTS:
        path = root / relative
        try:
            candidate = root
            for part in Path(relative).parts:
                candidate = candidate / part
                if candidate.is_symlink():
                    break
            if candidate.is_symlink():
                continue
            resolved = path.resolve(strict=True)
            if not resolved.is_relative_to(root) or not resolved.is_file():
                continue
            if resolved.stat().st_size <= 0:
                continue
            if resolved.stat().st_size > 128 * 1024:
                continue
            if relative == "HARNESS_PROBE_REPORT.json":
                try:
                    parsed = json.loads(resolved.read_text(encoding="utf-8"))
                except (OSError, UnicodeError, json.JSONDecodeError):
                    continue
                if not isinstance(parsed, (dict, list)):
                    continue
            status[relative] = True
        except (OSError, RuntimeError, ValueError):
            continue
    return status


def _h007_required_public_artifacts_present(fixture_root: Path) -> bool:
    """Require every public acceptance/probe artifact named by the S018 prompt."""
    return all(_h007_public_artifact_status(fixture_root).values())


def _h007_required_review_observed(continuation: Optional[str],
                                    tool_calls: list[dict[str, Any]]) -> bool:
    """Require recorded Task/Run IDs and one native child with a terminal result."""
    if not isinstance(continuation, str):
        return False
    fields: dict[str, list[str]] = {}
    for line in continuation.splitlines():
        key, separator, value = line.partition(":")
        if separator:
            fields.setdefault(key.strip(), []).append(value.strip())
    task_ids = fields.get("review_task", [])
    run_ids = fields.get("review_run", [])
    agents = fields.get("review_agent", [])
    statuses = fields.get("review_status", [])
    findings = fields.get("review_findings", [])
    task_id_pattern = re.compile(r"^[A-Za-z0-9_-]{1,128}$")
    status_is_completed = (len(statuses) == 1
                           and re.fullmatch(
                               r"completed(?:\s+\([^()\r\n]{1,300}\))?",
                               statuses[0]) is not None)
    if (len(task_ids) != 1 or len(run_ids) != 1
            or agents != ["verifier"] or task_ids[0] == run_ids[0]
            or not task_id_pattern.fullmatch(task_ids[0])
            or not task_id_pattern.fullmatch(run_ids[0])
            or task_ids[0].lower() in {"pending", "none", "null"}
            or run_ids[0].lower() in {"pending", "none", "null"}
            or not status_is_completed
            or len(findings) != 1
            or findings[0].strip().lower() in {"", "none", "pending", "n/a", "not run"}):
        return False
    spawns = [index for index, call in enumerate(tool_calls)
              if call.get("tool") == "task" and call.get("task_action") == "spawn"
              and call.get("task_spawn_ids_present") is True
              and call.get("task_spawn_agent") == "verifier"
              and call.get("status") == "completed" and call.get("failed") is not True]
    if len(spawns) != 1:
        return False
    spawn_index = spawns[0]
    spawn = tool_calls[spawn_index]
    task_digest = hashlib.sha256(task_ids[0].encode("utf-8")).hexdigest()
    run_digest = hashlib.sha256(run_ids[0].encode("utf-8")).hexdigest()
    if (spawn.get("task_spawn_task_id_sha256") != task_digest
            or spawn.get("task_spawn_run_id_sha256") != run_digest):
        return False
    exact_get_attempts = [
        call for call in tool_calls
        if call.get("tool") == "task" and call.get("task_action") == "get"
        and call.get("task_id_sha256") == task_digest
    ]
    if len(exact_get_attempts) != 1:
        return False
    for get_index, call in enumerate(tool_calls):
        if not (
            get_index > spawn_index and call.get("tool") == "task"
            and call.get("task_action") == "get"
            and call.get("task_id_sha256") == task_digest
            and call.get("task_result_task_id_sha256") == task_digest
            and call.get("task_result_run_id_sha256") == run_digest
            and call.get("task_result_run_status") == "completed"
            and call.get("status") == "completed" and call.get("failed") is not True
        ):
            continue
        exact_run_terminal = any(
            spawn_index < status_index < get_index
            and status_call.get("tool") == "task"
            and status_call.get("task_action") in {"status", "wait"}
            and status_call.get("task_run_id_sha256") == run_digest
            and status_call.get("task_run_status") == "completed"
            and status_call.get("status") == "completed"
            and status_call.get("failed") is not True
            for status_index, status_call in enumerate(tool_calls)
        )
        if exact_run_terminal:
            return True
    return False


def _h007_phase_requirement(case_prompt: Optional[str], phase: str) -> Optional[str]:
    """Extract the bounded public requirement for one named H007 phase."""
    if not isinstance(case_prompt, str) or not case_prompt:
        return None
    lines = case_prompt.splitlines()
    in_phase_section = False
    found = False
    requirement: list[str] = []
    phase_header = re.compile(r"^\s*\d+\.\s+(?:`([^`]+)`|([a-z][a-z0-9_]*)):?(.*)$")
    for line in lines:
        if line.strip() in {"## Dependent implementation phases", "REQUIRED DEPENDENT STAGES"}:
            in_phase_section = True
            continue
        if in_phase_section and (line.startswith("## ")
                                 or line.strip() in {
                                     "NATIVE JOBS AND REQUIRED REPAIRS",
                                     "BOUNDED HARNESS PROBES", "FINAL CHECKPOINT",
                                     "HORIZON AND STOP RULES",
                                 }):
            break
        if not in_phase_section:
            continue
        match = phase_header.match(line)
        if match:
            if found:
                break
            name = match.group(1) or match.group(2)
            if name == phase:
                found = True
                requirement.append(match.group(3).strip())
            continue
        if found:
            if not line.strip():
                break
            requirement.append(line.strip())
    if not requirement:
        return None
    description = " ".join(requirement)
    return description[:2000].rstrip()


def _h007_s018_overlap_confirmed(continuation: Optional[str],
                                  tool_calls: list[dict[str, Any]]) -> bool:
    """Bind the named S018 release job to its native running/work/completed trace."""
    if not isinstance(continuation, str):
        return False
    fields: dict[str, list[str]] = {}
    for line in continuation.splitlines():
        key, separator, value = line.partition(":")
        if separator:
            fields.setdefault(key.strip(), []).append(value.strip())
    job_ids = fields.get("job_id_overlap", [])
    if job_ids == ["pending"] or len(job_ids) != 1:
        return False
    job_id = job_ids[0]
    if fields.get("job_command_overlap") != [H007_S018_RELEASE_OVERLAP_COMMAND]:
        return False
    if fields.get("job_id_overlap_running") != ["running"] \
            or fields.get("job_id_overlap_terminal") != ["completed"]:
        return False
    return _h007_s018_overlap_trace_confirmed(job_id, tool_calls)


def _h007_s018_overlap_trace_confirmed(job_id: str,
                                       tool_calls: list[dict[str, Any]]) -> bool:
    """Confirm exact submitted-ID running/work/completed order from native calls."""
    if not job_id or job_id == "pending":
        return False
    submitted = [index for index, call in enumerate(tool_calls)
                 if call.get("tool") == "test" and isinstance(call.get("job"), dict)
                 and call["job"].get("job_id") == job_id]
    running = [index for index, call in enumerate(tool_calls)
               if isinstance(call.get("job"), dict) and call["job"].get("job_id") == job_id
               and call["job"].get("status") == "running"]
    completed = [index for index, call in enumerate(tool_calls)
                 if call.get("tool") == "job" and isinstance(call.get("job"), dict)
                 and call["job"].get("job_id") == job_id
                 and call["job"].get("status") == "completed"]
    productive = {"write", "edit", "apply_patch", "bash", "test"}
    return any(start < running_at < terminal
               and any(tool_calls[index].get("tool") in productive
                       for index in range(running_at + 1, terminal))
               and any(tool_calls[index].get("tool") in productive
                       for index in range(terminal + 1, len(tool_calls)))
               for start in submitted for running_at in running for terminal in completed)


def _h007_horizon_extension_prompt(active_work_span_ms: int,
                                    tool_calls: list[dict[str, Any]],
                                    tool_call_limit: Optional[int],
                                    provider_request_limit: Optional[int]) -> str:
    """Continue useful work after task completion when the horizon is still short."""
    calls = len(tool_calls)
    remaining_calls = (max(0, tool_call_limit - calls)
                       if tool_call_limit is not None else None)
    call_budget = (
        f" The case allows at most {tool_call_limit} native calls; {calls} are recorded, "
        f"leaving {remaining_calls}. Do not exceed the cap."
        if tool_call_limit is not None else ""
    )
    request_limit = provider_request_limit or 1200
    minutes = active_work_span_ms / 60_000
    return (
        "The required stages, gates, exact-ID overlap, and public artifacts are complete, "
        "but the measured active-work span is " + f"{minutes:.1f} minutes of the required 30. "
        "Continue in this same session with bounded, correctness-bearing restart or "
        "failure-preservation cases inside the five existing packages. Add an assertion for "
        "each new behavior; use background jobs for long work and do productive work while "
        "they run. Do not wait, pad, repeat an unchanged passing suite, expand the product "
        "surface, or rewrite completed evidence. Keep the completed checklist and "
        "`next_phase: none` intact. Stop after the measured active-work span reaches at least "
        "32 minutes, then let the runner execute the unchanged independent verifier. Preserve "
        "the 1,200-request, 3,600-second, 10-GiB disk, and 300-second activity limits. "
        f"The request cap is {request_limit}." + call_budget
    )


def _h007_incomplete_repair_pair_feedback(
        repair_job_diagnostic: Mapping[str, Any]) -> str:
    """Name incomplete public repair-pair predicates without exposing IDs."""
    groups = repair_job_diagnostic.get("groups")
    if not isinstance(groups, Mapping):
        return ""
    labels = {
        "catalog_format": "catalog-format",
        "catalog_store": "catalog-store",
    }
    messages = {
        "failed_job_submitted_once": "submit its seed test exactly once",
        "failed_job_status_called_immediately": (
            "make the next native call status for the exact seed ID"),
        "failed_terminal_observed": (
            "observe the exact seed job's terminal failed status before editing or rerunning"),
        "repair_job_submitted_once": "submit its repair test exactly once",
        "repair_job_status_called_immediately": (
            "make the next native call status for the exact repair ID"),
        "repair_terminal_observed": (
            "observe the exact repair job's terminal completed status"),
        "repair_followed_failure": (
            "observe the seed's terminal failure before submitting its repair rerun"),
        "productive_change_between_failure_and_rerun": (
            "make a correctness-bearing source change after the observed failure and before the rerun"),
        "documented_states_match_observations": (
            "record each exact ID and its matching observed terminal state once"),
        "job_ids_are_distinct": "use distinct seed and repair job IDs",
        "repair_cycle_correlated": (
            "preserve the observed failure-to-edit-to-completed-rerun sequence"),
    }
    incomplete: list[str] = []
    for key, label in labels.items():
        group = groups.get(key)
        if not isinstance(group, Mapping) or group.get("qualified") is True:
            continue
        blockers = [message for field, message in messages.items()
                    if group.get(field) is not True]
        if blockers:
            incomplete.append(f"{label}: " + "; ".join(blockers))
        else:
            incomplete.append(f"{label}: complete the exact-ID evidence sequence")
    if not incomplete:
        return ""
    return (
        "Pair-specific repair feedback from public native observations: "
        + " | ".join(incomplete)
        + ". Keep each gate pending until its listed sequence is complete; do not queue "
        "the seed and repair jobs together or reuse one pair's evidence for the other."
    )


def _h007_timeout_stop_reason(stop_reason: Optional[str], *, timed_out: bool) -> Optional[str]:
    """Prevent a completed earlier prompt from masking a later request timeout."""
    return "turn_timeout" if timed_out else stop_reason


def _h007_prompt_stop_class(stop_reason: Optional[str], *,
                            failure_category: Optional[str] = None) -> str:
    """Project one prompt's terminal outcome without carrying state across prompts."""
    if failure_category == "cancelled":
        return "cancelled"
    if failure_category == "provider_timeout":
        return "provider_timeout"
    if failure_category == "turn_timeout":
        return "turn_timeout"
    if failure_category is not None:
        return "request_failure"
    if stop_reason == "end_turn":
        return "end_turn"
    if stop_reason in {"cancelled", "canceled"}:
        return "cancelled"
    return "other"


_H007_CANCELLATION_SOURCES = frozenset({
    "activity_stall", "disk_floor", "external_or_transport",
})


def _h007_project_cancellation_source(source: Optional[str]) -> Optional[str]:
    """Keep cancellation attribution to the declared, body-free source set."""
    return source if source in _H007_CANCELLATION_SOURCES else None


def _h007_prompt_timeout_diagnostic(*, timed_out: bool,
                                    prompt_request_count: int,
                                    followup_count: int,
                                    last_request_timeout_seconds: Optional[float],
                                    last_request_elapsed_ms: Optional[int],
                                    remaining_wallclock_seconds_at_prompt_start: Optional[float],
                                    remaining_wallclock_seconds: Optional[float]
                                    ) -> dict[str, Any]:
    """Return bounded timeout timing; never persist the exception or provider text."""
    return {
        "schema": "h007_prompt_timeout_diagnostic_v1",
        "state": "observed" if timed_out else "not_observed",
        "prompt_request_count": prompt_request_count,
        "autonomous_followup_count": followup_count,
        "last_request_timeout_seconds": (
            round(last_request_timeout_seconds, 3)
            if isinstance(last_request_timeout_seconds, (int, float)) else None
        ),
        "last_request_elapsed_ms": last_request_elapsed_ms,
        "remaining_wallclock_seconds_at_prompt_start": (
            round(remaining_wallclock_seconds_at_prompt_start, 3)
            if isinstance(remaining_wallclock_seconds_at_prompt_start, (int, float))
            else None
        ),
        "remaining_wallclock_seconds_at_timeout": (
            round(remaining_wallclock_seconds, 3)
            if timed_out and isinstance(remaining_wallclock_seconds, (int, float))
            else None
        ),
        "verifier_reserve_seconds": H006_H007_VERIFIER_RESERVE_SECONDS,
        "exception_text_retained": False,
        "provider_text_retained": False,
    }


_H007_PROMPT_STOP_CLASSES = frozenset({
    "end_turn", "cancelled", "turn_timeout", "provider_timeout", "request_failure",
    "other",
})
_H007_CONTINUATION_DECISIONS = frozenset({
    "continue", "continuation_complete", "agent_terminal", "no_native_progress",
    "followup_limit", "wallclock_cap", "verifier_reserve", "turn_timeout",
    "turn_failure",
})
_H007_PROGRESS_STATUSES = frozenset({
    "complete", "completed", "pending", "failed", "in_progress", "not_started",
    "other",
})


def _h007_prompt_work_accounting(*, run_limit_seconds: float,
                                remaining_wallclock_seconds: Optional[float]
                                ) -> dict[str, Optional[int]]:
    """Reconcile consumed prompt work against the unchanged verifier reserve."""
    reserve_ms = int(H006_H007_VERIFIER_RESERVE_SECONDS * 1000)
    limit_ms = max(0, int(max(0.0, run_limit_seconds - H006_H007_VERIFIER_RESERVE_SECONDS)
                          * 1000))
    if not isinstance(remaining_wallclock_seconds, (int, float)):
        return {
            "prompt_work_limit_ms": limit_ms,
            "prompt_work_consumed_ms": None,
            "prompt_work_remaining_ms": None,
            "verifier_reserve_ms": reserve_ms,
        }
    available_ms = max(0, min(limit_ms, int(max(0.0,
        remaining_wallclock_seconds - H006_H007_VERIFIER_RESERVE_SECONDS) * 1000)))
    return {
        "prompt_work_limit_ms": limit_ms,
        "prompt_work_consumed_ms": limit_ms - available_ms,
        "prompt_work_remaining_ms": available_ms,
        "verifier_reserve_ms": reserve_ms,
    }


def _h007_prompt_progress_snapshot(fixture_root: Path,
                                   tool_calls: list[dict[str, Any]], *,
                                   case_prompt: Optional[str]) -> dict[str, Any]:
    """Project public continuation progress into a bounded, content-free snapshot."""
    stages_order = _h007_required_stages(case_prompt)
    gates_order = _h007_required_gates(case_prompt)
    if not _h007_case_requires_gate_block(case_prompt) or stages_order is None:
        return {"schema": "h007_prompt_progress_v1", "state": "unavailable",
                "reason": "case_contract_unavailable", "raw_note_retained": False}
    continuation = _h007_read_continuation(fixture_root)
    if continuation is None:
        return {"schema": "h007_prompt_progress_v1", "state": "unavailable",
                "reason": "checkpoint_missing_or_unreadable", "raw_note_retained": False}
    summary = h007_results.summarize_continuation_document(
        continuation,
        tool_calls,
        required_phases=_h007_required_phases(case_prompt),
        required_contract_phases=_h007_required_contract_phases(case_prompt),
        required_stages=stages_order,
        required_gates=gates_order,
        required_job_label="overlap",
        required_job_command=H007_S018_RELEASE_OVERLAP_COMMAND,
        required_success_job_labels=H007_S018_REQUIRED_SUCCESS_JOB_LABELS,
        required_repair_job_pairs=H007_S018_REQUIRED_REPAIR_JOB_PAIRS,
        require_all_test_jobs_terminal=True,
    )
    parsed_stages = h007_results.documented_stages(continuation)
    parsed_gates = h007_results.documented_gates(continuation)
    if (not isinstance(parsed_stages, dict) or not isinstance(parsed_gates, dict)
            or set(parsed_stages) != set(stages_order)
            or set(parsed_gates) != set(gates_order)):
        return {
            "schema": "h007_prompt_progress_v1",
            "state": "unavailable",
            "reason": "checkpoint_malformed",
            "checkpoint_sha256": summary.get("note_sha256"),
            "raw_note_retained": False,
            "raw_job_ids_retained": False,
            "raw_tool_arguments_retained": False,
            "raw_tool_outputs_retained": False,
        }
    statuses = {"complete", "completed", "pending", "failed", "in_progress", "not_started"}

    def status_map(value: Any, names: tuple[str, ...]) -> dict[str, str]:
        source = value if isinstance(value, dict) else {}
        return {name: source.get(name) if source.get(name) in statuses else "other"
                for name in names}

    stage_statuses = status_map(summary.get("stages"), stages_order)
    gate_statuses = status_map(summary.get("gates"), gates_order)
    complete_values = {"complete", "completed"}
    incomplete_stages = [name for name in stages_order
                         if stage_statuses[name] not in complete_values]
    incomplete_gates = [name for name in gates_order
                        if gate_statuses[name] not in complete_values]
    repair = summary.get("repair_job_diagnostic")
    repair_groups = repair.get("groups") if isinstance(repair, dict) else None
    repair_pairs = ({name: bool(value.get("qualified"))
                     for name, value in repair_groups.items()
                     if name in {pair[0] for pair in H007_S018_REQUIRED_REPAIR_JOB_PAIRS}
                     and isinstance(value, dict)}
                    if isinstance(repair_groups, dict) else {})
    success_jobs = summary.get("required_success_jobs")
    success_jobs = ({name: value.get("completed") is True
                     for name, value in success_jobs.items()
                     if name in H007_S018_REQUIRED_SUCCESS_JOB_LABELS
                     and isinstance(value, dict)}
                    if isinstance(success_jobs, dict) else {})
    artifacts = _h007_public_artifact_status(fixture_root)
    safe_next_stage = summary.get("next_stage")
    safe_next_gate = summary.get("next_gate")
    return {
        "schema": "h007_prompt_progress_v1",
        "state": "observed" if summary.get("present") is True else "unavailable",
        "checkpoint_sha256": summary.get("note_sha256"),
        "document_valid": summary.get("document_valid") is True,
        "stage_statuses": stage_statuses,
        "stage_complete_count": sum(value in complete_values
                                     for value in stage_statuses.values()),
        "stage_total_count": len(stages_order),
        "first_missing_stage": incomplete_stages[0] if incomplete_stages else None,
        "gate_statuses": gate_statuses,
        "gate_complete_count": sum(value in complete_values
                                    for value in gate_statuses.values()),
        "gate_total_count": len(gates_order),
        "first_missing_gate": incomplete_gates[0] if incomplete_gates else None,
        "next_stage": safe_next_stage if safe_next_stage in (*stages_order, "none")
        else "other",
        "next_gate": safe_next_gate if safe_next_gate in (*gates_order, "none")
        else "other",
        "repair_pairs": repair_pairs,
        "repair_pair_complete_count": sum(repair_pairs.values()),
        "success_jobs": success_jobs,
        "success_job_complete_count": sum(success_jobs.values()),
        "release_overlap": (
            "confirmed" if _h007_s018_overlap_confirmed(continuation, tool_calls)
            else "pending"
        ),
        "reviewer": (
            "observed" if _h007_required_review_observed(continuation, tool_calls)
            else "pending"
        ),
        "public_artifacts": artifacts,
        "public_artifact_complete_count": sum(artifacts.values()),
        "raw_note_retained": False,
        "raw_job_ids_retained": False,
        "raw_tool_arguments_retained": False,
        "raw_tool_outputs_retained": False,
    }


def _h007_progress_advanced(before: Mapping[str, Any], after: Mapping[str, Any]) -> bool:
    """Return true only for observed forward movement in a required public obligation."""
    if before.get("state") != "observed" or after.get("state") != "observed":
        return False
    for field in ("stage_complete_count", "gate_complete_count",
                  "repair_pair_complete_count", "success_job_complete_count",
                  "public_artifact_complete_count"):
        if isinstance(before.get(field), int) and isinstance(after.get(field), int) \
                and after[field] > before[field]:
            return True
    if before.get("release_overlap") != "confirmed" \
            and after.get("release_overlap") == "confirmed":
        return True
    return before.get("reviewer") != "observed" and after.get("reviewer") == "observed"


def _h007_boundary_no_progress_count(previous_count: int,
                                     before: Mapping[str, Any],
                                     after: Mapping[str, Any]) -> int:
    """Advance the no-progress ladder only when both checkpoint samples parse."""
    if _h007_progress_advanced(before, after):
        return 0
    if before.get("state") == "observed" and after.get("state") == "observed":
        return min(100, max(0, previous_count) + 1)
    return 0


def _h007_prompt_boundary_diagnostic(*, request_index: int,
                                     started_elapsed_ms: int,
                                     remaining_wallclock_start_ms: Optional[int],
                                     request_timeout_ms: int,
                                     request_elapsed_ms: Optional[int],
                                     active_work_span_start_ms: Optional[int],
                                     active_work_span_end_ms: Optional[int],
                                     tool_call_count_start: int,
                                     tool_call_count_end: int,
                                     stop_class: str,
                                     continuation_decision: Optional[str],
                                     reviewer_due_marker_received: bool,
                                     reviewer_due_marker_sent_next: Optional[bool],
                                     prompt_work_limit_ms: Optional[int] = None,
                                     prompt_work_consumed_ms_start: Optional[int] = None,
                                     prompt_work_remaining_ms_start: Optional[int] = None,
                                     prompt_work_consumed_ms_end: Optional[int] = None,
                                     prompt_work_remaining_ms_end: Optional[int] = None,
                                     verifier_reserve_ms: Optional[int] = None,
                                     prompt_progress_start: Optional[Mapping[str, Any]] = None,
                                     prompt_progress_end: Optional[Mapping[str, Any]] = None,
                                     no_progress_boundary_count_before: Optional[int] = None,
                                     no_progress_boundary_count: Optional[int] = None
                                     ) -> dict[str, Any]:
    """Project one prompt boundary using bounded timing and allowlisted states only."""
    stop = stop_class if stop_class in _H007_PROMPT_STOP_CLASSES else "other"
    decision = (continuation_decision
                if continuation_decision in _H007_CONTINUATION_DECISIONS else None)

    def bounded_ms(value: Optional[int]) -> Optional[int]:
        return value if isinstance(value, int) and not isinstance(value, bool) \
            and value >= 0 else None

    return {
        "request_index": max(0, request_index),
        "started_elapsed_ms": bounded_ms(started_elapsed_ms),
        "remaining_wallclock_start_ms": bounded_ms(remaining_wallclock_start_ms),
        "request_timeout_ms": bounded_ms(request_timeout_ms),
        "request_elapsed_ms": bounded_ms(request_elapsed_ms),
        "active_work_span_start_ms": bounded_ms(active_work_span_start_ms),
        "active_work_span_end_ms": bounded_ms(active_work_span_end_ms),
        "tool_call_count_start": max(0, tool_call_count_start),
        "tool_call_count_end": max(0, tool_call_count_end),
        "stop_class": stop,
        "continuation_decision": decision,
        "reviewer_due_marker_received": reviewer_due_marker_received is True,
        "reviewer_due_marker_sent_next": (
            reviewer_due_marker_sent_next
            if isinstance(reviewer_due_marker_sent_next, bool) else None
        ),
        "prompt_work_limit_ms": bounded_ms(prompt_work_limit_ms),
        "prompt_work_consumed_ms_start": bounded_ms(prompt_work_consumed_ms_start),
        "prompt_work_remaining_ms_start": bounded_ms(prompt_work_remaining_ms_start),
        "prompt_work_consumed_ms_end": bounded_ms(prompt_work_consumed_ms_end),
        "prompt_work_remaining_ms_end": bounded_ms(prompt_work_remaining_ms_end),
        "verifier_reserve_ms": bounded_ms(verifier_reserve_ms),
        "prompt_progress_start": dict(prompt_progress_start)
        if isinstance(prompt_progress_start, Mapping) else None,
        "prompt_progress_end": dict(prompt_progress_end)
        if isinstance(prompt_progress_end, Mapping) else None,
        "no_progress_boundary_count_before": max(0, no_progress_boundary_count_before)
        if isinstance(no_progress_boundary_count_before, int)
        and not isinstance(no_progress_boundary_count_before, bool) else None,
        "no_progress_boundary_count": max(0, no_progress_boundary_count)
        if isinstance(no_progress_boundary_count, int)
        and not isinstance(no_progress_boundary_count, bool) else None,
        "prompt_text_retained": False,
        "provider_text_retained": False,
        "exception_text_retained": False,
        "raw_tool_arguments_retained": False,
    }


def _h007_overlap_needs_productive_work(continuation: Optional[str],
                                       tool_calls: list[dict[str, Any]]) -> bool:
    """Check whether the required release job is still running without productive work."""
    if not isinstance(continuation, str):
        return False
    overlap_id = next((line.partition(":")[2].strip()
                       for line in continuation.splitlines()
                       if line.partition(":")[0].strip() == "job_id_overlap"), None)
    if not overlap_id or overlap_id == "pending":
        return False
    observations = [
        (index, call.get("job", {}).get("status"))
        for index, call in enumerate(tool_calls)
        if call.get("tool") in {"test", "job"}
        and isinstance(call.get("job"), Mapping)
        and call["job"].get("job_id") == overlap_id
        and call["job"].get("status") in {
            "queued", "running", "completed", "failed", "cancelled", "canceled", "timed_out",
        }
    ]
    running_positions = [index for index, state in observations if state == "running"]
    if not running_positions or observations[-1][1] != "running":
        return False
    latest_running = max(running_positions)
    productive = {"write", "edit", "replace", "apply_patch", "bash", "test"}
    return not any(
        index > latest_running and call.get("tool") in productive
        and call.get("status") == "completed" and call.get("failed") is not True
        for index, call in enumerate(tool_calls)
    )


def _h007_followup_prompt(fixture_root: Path,
                          tool_calls: Optional[list[dict[str, Any]]] = None,
                          tool_call_limit: Optional[int] = None,
                          provider_request_limit: Optional[int] = None,
                          case_prompt: Optional[str] = None,
                          active_work_span_ms: Optional[int] = None,
                          continuation_complete: bool = False,
                          remaining_work_seconds: Optional[float] = None,
                          no_progress_boundary_count: int = 0) -> str:
    """Give a same-session continuation the exact remaining public obligations."""
    calls = tool_calls or []
    measured_span = (active_work_span_ms if active_work_span_ms is not None
                     else _active_work_span_ms(calls))
    delayed_final_review = _h007_review_uses_30_minute_threshold(case_prompt)
    review_start_active_ms = (
        H007_S018_FINAL_REVIEW_START_ACTIVE_MS if delayed_final_review
        else H007_S018_REVIEW_START_ACTIVE_MS
    )
    review_start_marker = (
        H007_S018_FINAL_REVIEW_START_MARKER if delayed_final_review
        else H007_S018_REVIEW_START_MARKER
    )
    if continuation_complete and measured_span < H007_MIN_ACTIVE_DURATION_MS:
        return _h007_horizon_extension_prompt(
            measured_span, calls, tool_call_limit, provider_request_limit)
    continuation = _h007_read_continuation(fixture_root)
    final_success_jobs_confirmed = h007_results.summarize_required_success_jobs(
        continuation, calls, H007_S018_REQUIRED_SUCCESS_JOB_LABELS,
    )["complete"] if delayed_final_review else False
    separate_stage_ledger = _h007_case_uses_separate_stage_ledger(case_prompt)
    if separate_stage_ledger:
        stages = h007_results.documented_stages(continuation or "")
        phases = [name for name, status in (stages or {}).items()
                  if status in {"complete", "completed"}]
        phase_order = _h007_required_stages(case_prompt) or ()
    else:
        phases = _h007_documented_phases(continuation)
        phase_order = _h007_required_phases(case_prompt)
    phase_set = frozenset(phase_order)
    completed = set(phases) & phase_set
    missing = [phase for phase in phase_order if phase not in completed]
    next_task_pointer = "next_stage" if separate_stage_ledger else "next_phase"
    contract_phase_order = _h007_required_contract_phases(case_prompt) or ()
    contract_phases = h007_results.documented_contract_phases(continuation or "")
    missing_contract_phases = [
        phase for phase in contract_phase_order if phase not in (contract_phases or [])
    ]
    gate_checklist_required = _h007_case_requires_gate_block(case_prompt)
    repair_pairs_required = _h007_case_requires_repair_job_pairs(case_prompt)
    repair_job_diagnostic = (
        h007_results.summarize_required_repair_jobs(
            continuation, calls, H007_S018_REQUIRED_REPAIR_JOB_PAIRS,
        ) if repair_pairs_required else None
    )
    gate_order = _h007_required_gates(case_prompt)
    gate_set = frozenset(gate_order)
    gate_statuses = h007_results.documented_gates(continuation or "")
    missing_gates: list[str] = []
    if gate_checklist_required:
        if gate_statuses is None:
            missing_gates = ["the required `gates:` checklist (missing or malformed)"]
        else:
            missing_gates = [
                gate for gate in gate_order
                if gate_statuses.get(gate) not in {"complete", "completed"}
            ]
            missing_gates.extend(
                gate for gate, status in gate_statuses.items()
                if status not in {"complete", "completed"} and gate not in gate_set
            )
        if repair_pairs_required and repair_job_diagnostic is not None \
                and not repair_job_diagnostic["complete"]:
            missing_gates.append("two exact-ID observed failure-to-repair job pairs")
    if gate_checklist_required:
        public_artifact_status = _h007_public_artifact_status(fixture_root)
        missing_gates.extend(
            f"valid public artifact `{name}`"
            for name, valid in public_artifact_status.items() if not valid
        )
        if not _h007_required_review_observed(continuation, calls):
            missing_gates.append("terminal independent review result")
        if not _h007_s018_overlap_confirmed(continuation, calls):
            missing_gates.append("native exact release-scale overlap evidence")
    if missing:
        phase_requirement = _h007_phase_requirement(case_prompt, missing[0])
        detailed_requirement = (
            f" Acceptance for `{missing[0]}`: {phase_requirement}"
            if phase_requirement else ""
        )
        repair_prerequisite = (
            " Before implementation stages, finish the pending seeded repair gates in order: "
            "submit each untouched starter test as a background job, observe its exact ID "
            "fail, then repair only that defect and observe a new exact-ID test complete. "
            "Do not edit Rust source before both baseline failures are observed."
            if gate_checklist_required and gate_statuses is not None
            and any(gate_statuses.get(gate) not in {"complete", "completed"}
                    for gate in ("starter_repair_format", "starter_repair_store"))
            else ""
        )
        if repair_prerequisite:
            phase_instruction = (
                f"The current public note lists {len(completed)} of {len(phase_order)} required stages. "
                f"The next implementation stage is `{missing[0]}`, but the two seeded repair "
                "gates are prerequisites and remain pending. Complete those baseline-failure "
                "and repair cycles first; do not start the implementation stages until both "
                f"gates are complete. Do not trust `{next_task_pointer}: none` while any stage is missing."
                + repair_prerequisite
            )
        else:
            phase_instruction = (
                f"The current public note lists {len(completed)} of {len(phase_order)} required stages. "
                f"Its next required phase is `{missing[0]}`; remaining phases in order are: "
                + ", ".join(missing)
                + f". Do not trust `{next_task_pointer}: none` while any are missing. "
                "Make a functional source change and an assertion-bearing test for this phase in "
                "this follow-up before ending; do not only rewrite the progress note or repeat "
                "discovery on completed phases."
                + detailed_requirement
            )
    else:
        phase_instruction = (
            f"All {len(phase_order)} required stage names are listed, but the validator has not "
            "confirmed the exact public continuation evidence. Reconcile the note against native "
            "job observations and complete the named gates before ending."
        )
    if separate_stage_ledger and missing_contract_phases:
        phase_instruction += (
            " Track contract.json phases separately in `contract_phases`; the first incomplete "
            "contract phase is `" + missing_contract_phases[0] + "`. Set "
            "`next_contract_phase` to that name until all five are complete, then set it to "
            "`none`. Keep the seven H007 stages in `completed_phases` and `required_stages`."
        )
    gate_instruction = (
        "The required gate evidence is incomplete: " + ", ".join(dict.fromkeys(missing_gates)) +
        (". Keep incomplete gates pending. `completed_phases`/`next_phase` preserve the seven "
         "H007 phases expected by the frozen continuation format; `contract_phases`/"
         "`next_contract_phase` track the five contract.json phases; `required_stages`/"
         "`next_stage` provide the separate seven-stage checklist. Keep `next_gate` on the first incomplete "
         "gate until its evidence exists, then "
         if separate_stage_ledger else
         ". Keep any incomplete checklist entry pending; do not set `next_phase: none` until "
         "the listed gates and artifacts are complete, then ") +
        "update and read back CONTINUATION.md, then continue."
        if missing_gates else ""
    )
    if gate_checklist_required and not public_artifact_status.get(
            "HARNESS_PROBE_REPORT.json", False):
        gate_instruction += (
            " `HARNESS_PROBE_REPORT.json` must contain a valid JSON object or array, not just "
            "exist. From the fixture root, validate it with `python3 -m json.tool "
            "HARNESS_PROBE_REPORT.json >/dev/null`; keep `bounded_probes` pending until that "
            "command succeeds, then read the report back."
        )
    lifecycle_observed, productive_overlap_observed = h007_results._job_lifecycle(calls)
    submitted_job_ids = {
        call["job"]["job_id"] for call in calls
        if call.get("tool") == "test" and isinstance(call.get("job"), dict)
        and isinstance(call["job"].get("job_id"), str)
    }
    latest_job_states: dict[str, str] = {}
    for call in calls:
        job = call.get("job")
        job_id = job.get("job_id") if isinstance(job, dict) else None
        if (call.get("tool") in {"test", "job"} and isinstance(job, dict)
                and isinstance(job_id, str) and job_id in submitted_job_ids
                and isinstance(job.get("status"), str)):
            latest_job_states[job_id] = job["status"]
    running_overlap_jobs = sorted(
        job_id for job_id, state in latest_job_states.items() if state == "running")
    if gate_checklist_required and _h007_s018_overlap_confirmed(continuation, calls):
        overlap_instruction = (
            "The exact release-scale test command and its same-ID running, productive-edit, "
            "and completed observations are present in CONTINUATION.md and the native trace. "
            "Preserve that evidence; do not submit another overlap job."
        )
    elif gate_checklist_required:
        overlap_id = None
        if continuation:
            overlap_id = next((line.partition(":")[2].strip()
                               for line in continuation.splitlines()
                               if line.partition(":")[0].strip() == "job_id_overlap"), None)
        submitted_ids = {
            call["job"]["job_id"] for call in calls
            if call.get("tool") == "test" and isinstance(call.get("job"), dict)
            and isinstance(call["job"].get("job_id"), str)
        }
        latest_states: dict[str, str] = {}
        for call in calls:
            job = call.get("job")
            if (call.get("tool") in {"test", "job"} and isinstance(job, dict)
                    and job.get("job_id") in submitted_ids
                    and isinstance(job.get("status"), str)):
                latest_states[job["job_id"]] = job["status"]
        if (isinstance(overlap_id, str) and overlap_id != "pending"
                and _h007_s018_overlap_trace_confirmed(overlap_id, calls)):
            overlap_instruction = (
                "The native trace already proves that this exact recorded release-scale ID "
                "was submitted, observed running, had a productive dependent edit before its "
                "completed observation, and completed. Do not resubmit the release test or "
                "poll the completed job again. Repair only the public ledger for that same ID: "
                "preserve its `job_command_overlap` and `job_id_overlap`, and write these exact "
                "separate lines: `job_id_overlap_running: running` and "
                "`job_id_overlap_terminal: completed`. Then read CONTINUATION.md back."
            )
        elif (isinstance(overlap_id, str) and overlap_id != "pending"
                and latest_states.get(overlap_id) == "running"):
            overlap_instruction = (
                "The recorded release overlap ID is still observed running: " + overlap_id +
                ". Do not submit another test. Make the next native call a correctness-bearing "
                "edit in `catalog-query/src/lib.rs`, then query `job status` for this same ID; "
                "record the exact command, running state, edit, and terminal state with the "
                "separate lines `job_id_overlap_running: running` and "
                "`job_id_overlap_terminal: completed`."
            )
        else:
            boundary = ("The dependency-index boundary is complete; run it now. "
                        if "dependency_index" in completed else
                        "Run it immediately after dependency-index implementation. ")
            overlap_instruction = (
                "The required release-scale overlap is not proven for the exact command and ID. "
                + boundary + "Submit exactly `" + H007_S018_RELEASE_OVERLAP_COMMAND +
                "` through native `test` (`scope: custom`, `workdir: fixture`, "
                "`execution_mode: background`). Make the next native call `job status` for its "
                "returned ID. If running, edit `catalog-query/src/lib.rs` before the next status "
                "poll, then observe that same ID completed. Record `job_command_overlap: " +
                H007_S018_RELEASE_OVERLAP_COMMAND + "` and `job_id_overlap` in "
                "CONTINUATION.md. Add the separate literal lines `job_id_overlap_running: "
                "running` and `job_id_overlap_terminal: completed` only after the native trace "
                "shows those states for that same ID. Record the productive edit. Never "
                "substitute a unit, repair, workspace, or priority job."
            )
    elif lifecycle_observed and productive_overlap_observed:
        overlap_instruction = (
            "The native trace already proves one exact submitted test ID reached both running "
            "and completed with productive work between those observations. Preserve that ID "
            "and do not launch a replacement overlap job."
        )
    elif running_overlap_jobs:
        overlap_instruction = (
            "An exact submitted background test is still running (ID: "
            + running_overlap_jobs[0] + "). Do not submit another test. Make the next native "
            "tool call a substantive edit in the next dependent crate, then query `job status` "
            "for that same ID until it is observed completed. Do not status-poll consecutively."
        )
    elif lifecycle_observed:
        overlap_instruction = (
            "A submitted test ID was observed running and completed, but no productive native "
            "work occurred between those observations. The exact-ID overlap gate is still "
            "unsatisfied. At the next genuinely needed test after a correctness-bearing source "
            "change, submit that test as a background job, observe that exact ID running, make "
            "a substantive edit in the next dependent crate, and then observe the same ID "
            "completed. Do not repeat an unchanged passing test just to create an overlap."
        )
    else:
        overlap_instruction = (
            "The exact-ID overlap gate is still missing: no single submitted native test ID has "
            "been observed both running and completed with productive work between. At the "
            "earliest suitable boundary after dependency-index implementation, submit the "
            "genuinely needed `cargo test --offline --locked --release -p catalog-index --test "
            "corpus` through native `test` (`scope: custom`, `workdir: fixture`, "
            "`execution_mode: background`). Save its returned ID as `job_id_overlap`; make the "
            "very next native call `job status` for that exact ID. If it is running, the next "
            "call must be a substantive edit in `fixture/catalog-query/src/lib.rs`, followed by "
            "status on that same ID until completed. Do not use a child task as a substitute, "
            "do not poll consecutively, and do not claim overlap from CONTINUATION.md prose."
        )
    task_spawns = [call for call in (tool_calls or [])
                   if call.get("tool") == "task" and call.get("task_action") == "spawn"]
    verifier_spawn_attempts = [call for call in task_spawns
                               if call.get("task_spawn_agent") == "verifier"]
    id_bearing_spawns = [call for call in task_spawns
                         if call.get("task_spawn_ids_present") is True
                         and call.get("status") == "completed"
                         and call.get("failed") is not True]
    reviewer_calls = [call for call in id_bearing_spawns
                      if call.get("task_spawn_agent") == "verifier"]
    failed_terminal_status = False
    if reviewer_calls:
        spawn = reviewer_calls[0]
        spawn_index = next((index for index, call in enumerate(tool_calls or [])
                            if call is spawn), -1)
        exact_run_status = next((call for call in reversed(tool_calls or [])
                                 if call.get("tool") == "task"
                                 and call.get("task_action") in {"status", "wait"}
                                 and call.get("task_run_id_sha256")
                                 == spawn.get("task_spawn_run_id_sha256")
                                 and call.get("status") == "completed"
                                 and call.get("failed") is not True), None)
        exact_run_status_indices = [
            index for index, call in enumerate(tool_calls or [])
            if call.get("tool") == "task"
            and call.get("task_action") in {"status", "wait"}
            and call.get("task_run_id_sha256") == spawn.get("task_spawn_run_id_sha256")
        ]
        last_reviewer_checkpoint = max(exact_run_status_indices, default=spawn_index)
        non_task_calls_since_reviewer_checkpoint = sum(
            call.get("tool") != "task"
            for call in (tool_calls or [])[last_reviewer_checkpoint + 1:]
        )
        status_refused = exact_run_status is None and any(
            call.get("tool") == "task"
            and call.get("task_action") in {"status", "wait"}
            and call.get("task_run_id_sha256")
            == spawn.get("task_spawn_run_id_sha256")
            and (call.get("failed") is True or call.get("status") == "failed")
            for call in tool_calls or []
        )
        failed_terminal_status = any(
            call.get("tool") == "task"
            and call.get("task_action") in {"status", "wait"}
            and call.get("task_run_id_sha256") == spawn.get("task_spawn_run_id_sha256")
            and call.get("task_run_status") in {"failed", "cancelled", "timed_out"}
            and call.get("status") == "completed" and call.get("failed") is not True
            for call in tool_calls or []
        )
        reviewer_task_digest = spawn.get("task_spawn_task_id_sha256")
        exact_reviewer_gets = [
            call for call in (tool_calls or [])
            if call.get("tool") == "task" and call.get("task_action") == "get"
            and call.get("task_id_sha256") == reviewer_task_digest
        ]
        duplicate_reviewer_gets = max(0, len(exact_reviewer_gets) - 1)
        terminal_result_observed = any(
            call_index > status_index
            and call.get("tool") == "task" and call.get("task_action") == "get"
            and call.get("task_id_sha256") == spawn.get("task_spawn_task_id_sha256")
            and call.get("task_result_task_id_sha256")
            == spawn.get("task_spawn_task_id_sha256")
            and call.get("task_result_run_id_sha256")
            == spawn.get("task_spawn_run_id_sha256")
            and call.get("task_result_run_status") == "completed"
            and call.get("status") == "completed" and call.get("failed") is not True
            and status_call.get("tool") == "task"
            and status_call.get("task_action") in {"status", "wait"}
            and status_call.get("task_run_id_sha256")
            == spawn.get("task_spawn_run_id_sha256")
            and status_call.get("task_run_status") == "completed"
            and status_call.get("status") == "completed"
            and status_call.get("failed") is not True
            for call_index, call in enumerate(tool_calls or [])
            for status_index, status_call in enumerate(tool_calls or [])
            if status_index > spawn_index
        )
        if duplicate_reviewer_gets:
            reviewer_instruction = (
                "The exact verifier Task result was fetched more than once. The single-fetch "
                "review gate is now unsatisfied for this run. Do not call `task get` again, "
                "do not spawn a replacement reviewer, keep `independent_review` pending, and "
                "continue the next missing implementation or qualification work. Record the "
                "duplicate retrieval limitation accurately."
            )
        elif len(reviewer_calls) != 1:
            reviewer_instruction = (
                "More than one native `verifier` child was spawned. Do not create another. "
                "At final audit, retrieve one exact Task result and record the duplicate-review "
                "limitation honestly; this run cannot qualify as a single-review run."
            )
        elif status_refused:
            reviewer_instruction = (
                "The exact verifier Run ID status/wait request was refused. Do not retry the "
                "same control call, do not call task-get repeatedly, and do not treat a completed "
                "task-get result as a Run-status observation. Leave `independent_review` pending "
                "and record the typed refusal separately; never claim `review_status: completed` "
                "without a successful native status/wait result for this exact Run ID. Continue "
                "other required work without spawning a replacement verifier."
            )
        elif failed_terminal_status:
            reviewer_instruction = (
                "The exact verifier Run ended without status completed. Record its terminal "
                "failure accurately, leave `independent_review` pending, and do not retrieve a "
                "replacement or spawn another verifier. Continue any remaining required work."
            )
        elif terminal_result_observed:
            reviewer_instruction = (
                "The one-shot native `verifier` lifecycle is complete: its exact Run was "
                "observed completed and its exact Task result was fetched once. Record "
                "`review_agent: verifier`, the exact Task/Run IDs, `review_status: completed`, "
                "and concise evidence-backed findings in the existing CONTINUATION.md reviewer "
                "fields. Treat those fields as the durable review receipt. Do not invoke the "
                "native `task` tool again for this reviewer: no spawn, status, wait, or get, "
                "including at final audit. Reuse the checkpoint while continuing other missing "
                "qualification work."
            )
        elif exact_run_status is not None and exact_run_status.get("task_run_status") == "completed":
            reviewer_instruction = (
                "The exact native `verifier` Run ID was observed `completed` with task status. "
                "Now call native `task get` exactly once with its matching Task ID, after that "
                "status observation. An earlier completed `get` made before this status does "
                "not satisfy the required order. Confirm the same Task and Run are completed, "
                "then record `review_agent: verifier`, exact IDs, and concise evidence-backed "
                "findings. Do not call get again after this ordered result."
            )
        elif (reviewer_calls and any(
                call.get("tool") == "task" and call.get("task_action") == "get"
                and call.get("task_id_sha256") == reviewer_calls[0].get("task_spawn_task_id_sha256")
                and call.get("task_result_task_id_sha256")
                == reviewer_calls[0].get("task_spawn_task_id_sha256")
                and call.get("task_result_run_id_sha256")
                == reviewer_calls[0].get("task_spawn_run_id_sha256")
                and call.get("task_result_run_status") == "completed"
                and call.get("status") == "completed" and call.get("failed") is not True
                for call in tool_calls or []) and not terminal_result_observed):
            reviewer_instruction = (
                "A completed result was already returned by `task get`, but it happened before "
                "an exact completed Run-status observation. It does not satisfy the required "
                "status-then-get order. Do not call get again now. Make one native `task status` "
                "call with the exact verifier Run ID. If it reports completed, the next required "
                "review call is one `task get` with the matching Task ID; if it is not completed, "
                "leave independent_review pending. After that ordered get, do not repeat either "
                "call."
            )
        elif gate_checklist_required and missing:
            if non_task_calls_since_reviewer_checkpoint >= 3:
                reviewer_instruction = (
                    "The one native verifier child has both exact IDs. Since its last exact-ID "
                    "status checkpoint, at least three non-task tool operations have completed. "
                    "Make one native `task status` call now with that exact Run ID. If it reports "
                    "`completed`, call `task get` exactly once with the matching Task ID. If it "
                    "reports `queued` or `running`, do not call `task get`; continue productive "
                    "required work and check again only after at least three more non-task tool "
                    "operations. At final audit only, one `task wait` with the exact Run ID and "
                    "`timeout_ms: 30000` may replace a status call; a timeout means still running. "
                    "Never retrieve a live result, poll consecutively, repeat a completed get, or "
                    "spawn a replacement."
                )
            else:
                remaining = 3 - non_task_calls_since_reviewer_checkpoint
                reviewer_instruction = (
                    "The one native verifier child has both exact IDs. Continue productive "
                    "required work; after at least " + str(remaining) + " more non-task tool "
                    "operations, make one `task status` call with its exact Run ID. If it reports "
                    "`completed`, call `task get` exactly once with the matching Task ID. If it "
                    "reports `queued` or `running`, do not call `task get`; continue work and wait "
                    "for three more non-task operations before the next status. At final audit "
                    "only, one `task wait` with the exact Run ID and `timeout_ms: 30000` may "
                    "replace a status call; a timeout means still running. Never retrieve a live "
                    "result, poll consecutively, repeat a completed get, or spawn a replacement."
                )
        elif missing:
            reviewer_instruction = (
                "The native `verifier` child has both exact IDs and may remain active. Do not "
                "retrieve it until at least two additional required stages are complete or the "
                "final audit begins. Continue dependent implementation while it runs; research "
                "children do not replace this verifier. Use native `task status` or bounded "
                "`task wait` with the exact Run ID, never a different child's ID."
            )
        elif gate_checklist_required:
            reviewer_instruction = (
                "At final audit, make one native `task status` call using the exact verifier Run "
                "ID. If it reports `completed`, call `task get` exactly once with the matching "
                "Task ID. If it remains queued or running, do not call `task get`; one bounded "
                "`task wait` on the exact Run ID with `timeout_ms: 30000` is allowed. If that "
                "wait times out or reports a non-completed terminal state, leave "
                "`independent_review` pending. Never retrieve a live result, repeat a completed "
                "get, or spawn a replacement."
            )
        else:
            reviewer_instruction = (
                "The native `verifier` child has both exact IDs. At final audit, observe its "
                "Run with native `task status` using the exact Run ID; if still running, make "
                "one bounded `task wait` on that same Run ID while continuing productive work. "
                "After terminal `completed`, call `task get` exactly once with the matching "
                "Task ID. Record `review_agent: verifier`, both exact IDs, and evidence-backed "
                "findings. Do not retrieve it repeatedly or spawn a replacement."
            )
    elif verifier_spawn_attempts:
        reviewer_instruction = (
            "A native `verifier` spawn was already attempted but did not return one successful "
            "exact Task/Run pair. Do not retry it or create a replacement. Record the typed "
            "spawn limitation, leave `independent_review` pending, and continue the remaining "
            "required work."
        )
    elif id_bearing_spawns:
        reviewer_instruction = (
            "A native child was spawned, but it is not the required `verifier` agent. A "
            "research, explore, or general child is not an independent review and must not be "
            "reported as one. Spawn exactly one native task with `action: spawn` and `agent: "
            "verifier`, then record that child's exact Task and Run IDs. Do not cancel or "
            "mislabel existing research work."
        )
    elif task_spawns:
        reviewer_instruction = (
            "A native task spawn was attempted, but the trace did not observe both typed Task "
            "and Run IDs. Do not count task-board calls, queued placeholders, scheduler job IDs, "
            "or external-dispatch prose as a child. If this was not the dedicated `verifier` "
            "spawn, make the one required native task call with `action: spawn` and `agent: "
            "verifier`; if that exact spawn was refused or returned no Run ID, record the "
            "limitation and do not replay identical inputs."
        )
    elif (gate_checklist_required
          and measured_span < review_start_active_ms):
        threshold_minutes = review_start_active_ms // (60 * 1000)
        reviewer_instruction = (
            "Do not call the native `task` tool or spawn a reviewer before the measured "
            f"active-work span reaches {threshold_minutes} minutes. Continue productive "
            "implementation and evidence work. At the first same-session follow-up after the "
            f"{threshold_minutes}-minute threshold, "
            "spawn exactly one read-only verifier as directed; do not start it early."
        )
    elif gate_checklist_required and measured_span >= review_start_active_ms:
        threshold_minutes = review_start_active_ms // (60 * 1000)
        if delayed_final_review:
            final_job_state = (
                "Both exact final workspace-suite and Clippy jobs are confirmed completed."
                if final_success_jobs_confirmed else
                "One or both exact final workspace-suite and Clippy jobs are not yet confirmed "
                "completed; leave that gate open and continue observing their exact IDs in "
                "parallel with review."
            )
            reviewer_instruction = (
                review_start_marker + " Spawn exactly one native read-only task now with "
                "`action: spawn` and `agent: verifier`, regardless of final-job state. "
                + final_job_state + " Ask it to challenge the public contract traceability "
                "matrix, current implementation, and currently available evidence; identify "
                "unsupported claims or missing evidence, and never inspect hidden verifier "
                "files or output. Capture both exact Task and Run IDs, then continue required "
                "implementation and job work while it runs. After at least three non-task tool "
                "operations, make one exact-Run `task status` call. If it reports `completed`, "
                "call `task get` exactly once with the matching Task ID. If it reports `queued` "
                "or `running`, do not call `task get`; continue productive work and check again "
                "only after three more non-task operations. At final audit only, one `task wait` "
                "with the exact Run ID and `timeout_ms: 30000` may replace a status call; a "
                "timeout means still running. Never retrieve a live result, repeat a completed "
                "get, or spawn a replacement."
            )
        else:
            reviewer_instruction = (
                f"The {threshold_minutes}-minute independent-review start threshold has been reached. Spawn exactly "
                "one native read-only task now with `action: spawn` and `agent: verifier`, even if "
                "acceptance, probe, workspace, public-artifact, or overlap gates remain incomplete. "
                "The review is parallel work, not a prerequisite to implementation. Scope it to the "
                "public requirement matrix and currently available public evidence; identify missing "
                "evidence as missing and never inspect hidden verifier files or output. Capture both "
                "exact Task and Run IDs, then continue the next missing dependent stage and workspace "
                "qualification while it runs. After at least three non-task tool operations, make one "
                "exact-Run `task status` call. If it reports `completed`, call `task get` exactly "
                "once with the matching Task ID. If it reports `queued` or `running`, do not call "
                "`task get`; continue productive work and check again only after three more non-task "
                "operations. At final audit only, one `task wait` with the exact Run ID and "
                "`timeout_ms: 30000` may replace a status call; a timeout means still running. "
                "Never retrieve a live result, repeat a completed get, or spawn a replacement."
            )
    elif gate_checklist_required and (
        any(phase not in completed for phase in phase_order[:-1])
        or any(phase != phase_order[-1] for phase in missing)
        or any(gate not in {"workspace_qualification", "terminal independent review result"}
               for gate in missing_gates)
    ):
        reviewer_instruction = (
            "Defer the single read-only verifier until the first six dependent stages are "
            "complete and all earlier gates are complete, including public artifacts and the "
            "exact release-job overlap. Continue the next missing stage or gate now; do not "
            "spawn a reviewer early. Spawn exactly one verifier when `workspace_qualification` "
            "is next or already complete, and never replace it, even if its result is refused, "
            "failed, or expired."
        )
    elif gate_checklist_required:
        reviewer_instruction = (
            "The first six dependent stages and all earlier gates are complete; "
            "`workspace_qualification` is next or already complete. The independent review is "
            "now due: make exactly one native task spawn with `action: spawn` and `agent: "
            "verifier`. Scope the read-only review only to the seven-stage requirement-to-test "
            "matrix and the public acceptance/probe evidence; do not run tests or audit unrelated "
            "code. Return a concise matrix with file references and at most five concrete "
            "findings. Make only that one task call in this assistant response. "
            "Capture both exact Task and Run IDs, then continue useful workspace qualification "
            "while it runs. Do not call `task wait` or poll during implementation. At final audit "
            "make at most one exact-Run status call; only retrieve the exact Task once if that "
            "status is completed. Never spawn a second verifier or replacement; if its lifecycle "
            "is refused, failed, or expired, record that limitation and leave the review gate "
            "pending. Do not let it inspect hidden verifier files or output."
        )
    elif len(completed) < 5:
        reviewer_instruction = (
            ("Defer the single read-only subagent review until the first five required stages "
             "are complete. Do not spend this follow-up on task creation; implement the next "
             "missing stage first." if gate_checklist_required else
             "Defer the single read-only subagent review until the five original contract phases "
             "are complete. Do not spend this follow-up on task creation; implement the next "
             "missing phase first.")
        )
    else:
        reviewer_instruction = (
            "The required independent review is now due. Use the native task tool exactly once "
            "with `action: spawn`, `agent: verifier`, and a concise prompt to review the "
            "changed Rust code, public tests, and recorded acceptance evidence read-only. "
            "Capture both returned Task and Run IDs, then continue implementation while it runs. "
            + ("Retrieve its recorded result once with `task get` using the exact Task ID after "
               "the reviewer is terminal; require the same Run to be completed, do not retrieve "
               "it repeatedly, or let the review block source work. "
               if gate_checklist_required else
               "Check its exact Run ID and Task ID once after at least three more phases or at "
               "the final audit; do not poll repeatedly or let the review block source work. ")
            + "Record `review_agent: verifier` and concrete public-source findings. "
            "A board/workstream action requires a real existing group_id and is never a spawn. Do "
            "not use it as a substitute. The fixture's explicitly requested invalid-ID probes are "
            "separate negative tests; use their documented sentinel once, record the refusal, and "
            "never use a probe ID for child status/get. Never let the child inspect hidden verifier "
            "files or output."
        )
    if gate_checklist_required and "terminal independent review result" in missing_gates:
        if ((reviewer_calls and (status_refused or failed_terminal_status
                                 or duplicate_reviewer_gets > 0))
                or (verifier_spawn_attempts and not reviewer_calls)
                or len(reviewer_calls) > 1):
            reviewer_instruction += (
                " Do not write successful canonical review fields: the verifier lifecycle did "
                "not qualify. Keep `independent_review` pending and record only the observed "
                "IDs and typed limitation."
            )
        elif reviewer_calls and not terminal_result_observed:
            reviewer_instruction += (
                " Do not write successful review fields until the exact Run has a successful "
                "completed observation and its one matching Task result is completed. If either "
                "is missing, leave `independent_review` pending. Only after both observations "
                "succeed, write `review_agent: verifier`, `review_task: <exact Task ID>`, "
                "`review_run: <exact Run ID>`, `review_status: completed`, and "
                "`review_findings: <concise evidence-backed sentence>`."
            )
        else:
            reviewer_instruction += (
                " At final audit write exactly these five separate lines, using these key "
                "names without `_id` or `_terminal` suffixes: `review_agent: verifier`, "
                "`review_task: <exact Task ID>`, `review_run: <exact Run ID>`, "
                "`review_status: completed`, and "
                "`review_findings: <concise evidence-backed sentence>`."
            )
    pending_test_jobs = (
        h007_results.unobserved_test_job_ids(calls) if gate_checklist_required else []
    )
    if pending_test_jobs:
        job_instruction = (
            " Before submitting any additional native test job, observe every listed "
            "already-submitted test job to a terminal state using `job status` on its exact "
            "ID, one at a time. Submit no new test job while any earlier test job is "
            "nonterminal; after each submission, the next native call must be `job status` "
            "for that exact returned ID. Do not resubmit pending work or replace IDs. Before "
            "writing `workspace_qualification: complete` or ending this run, observe the "
            "listed jobs to terminal. This "
            "follow-up lists at most 40 outstanding IDs; after those are terminal, inspect the "
            "next follow-up for any remaining IDs: "
            + ", ".join(f"`{job_id}`" for job_id in pending_test_jobs[:40])
            + ". A failed terminal test may be repaired and rerun only after its original job "
            "is terminal. Keep workspace qualification pending until every submitted test job "
            "has a recorded terminal state. Also record the exact successful final workspace "
            "suite under `job_id_workspace_tests` / `job_state_workspace_tests: completed` "
            "and final Clippy job under `job_id_clippy` / `job_state_clippy: completed`; "
            "both IDs must have matching completed native-job observations."
        )
    elif gate_checklist_required:
        job_instruction = (
            " For every native test submission, use a background job and record its exact ID. "
            "The immediately following native call must be `job status` for that exact ID. "
            "Submit no second test job until the first is terminal; do productive non-test "
            "work between status observations when it remains running. Repair a failed test "
            "only after its exact job is terminal, then submit one new job and observe it "
            "the same way. Keep workspace qualification pending until all submitted test jobs "
            "have terminal observations and the required final workspace and Clippy jobs "
            "completed successfully. Use one final native background test job for each and "
            "record their exact IDs and completed states in `job_id_workspace_tests` / "
            "`job_state_workspace_tests: completed` and `job_id_clippy` / "
            "`job_state_clippy: completed`. Do not mark the gate complete unless both exact "
            "IDs have matching completed observations."
        )
    else:
        job_instruction = ""
    repair_instruction = ""
    if repair_pairs_required and repair_job_diagnostic is not None:
        if repair_job_diagnostic["complete"]:
            repair_instruction = (
                " Preserve the two qualified repair pairs exactly; do not rerun either seed "
                "or repair job. The observed one-to-one repair_cycles count is "
                f"{repair_job_diagnostic['observed_repair_cycle_count']}."
            )
        else:
            observed_repair_cycles = repair_job_diagnostic[
                "observed_repair_cycle_count"]
            documented_repair_cycles = repair_job_diagnostic[
                "documented_repair_cycle_count"]
            documented_count = (
                str(documented_repair_cycles)
                if isinstance(documented_repair_cycles, int)
                and not isinstance(documented_repair_cycles, bool)
                else "missing or malformed"
            )
            repair_instruction = (
                " Complete and document exactly these two seeded failure/repair pairs before "
                "marking their gates complete: catalog-format seed fields "
                "`job_id_catalog_format_seed: <exact ID>` and "
                "`job_state_catalog_format_seed: failed`, then repaired fields "
                "`job_id_catalog_format_repair: <new exact ID>` and "
                "`job_state_catalog_format_repair: completed`; repeat with the exact fields "
                "`job_id_catalog_store_seed`, `job_state_catalog_store_seed: failed`, "
                "`job_id_catalog_store_repair`, and `job_state_catalog_store_repair: completed`. "
                "Submit each background test "
                "once and make the immediately next native call `job status` for that exact "
                "ID. After the seed failure is terminal, make a correctness-bearing source "
                "edit before submitting its single repair rerun. Do not reuse IDs or count "
                "one success against multiple failures. Set `repair_cycles` to the exact "
                "number of distinct observed failed-job → productive-edit → later completed-job "
                "pairs, including at least these two named pairs; the named-pair requirement "
                "is a minimum and is not the total cycle count. Current live reconciliation: "
                f"the native-call projection observes {observed_repair_cycles} qualifying "
                f"cycle(s), while CONTINUATION.md documents {documented_count}. Update "
                f"`repair_cycles` to {observed_repair_cycles} now, then read the note back. "
                "If another qualifying cycle occurs later, the next follow-up will report a "
                "new current count; apply that latest value before ending. Do not try to read "
                "the private causal-trace file or infer its contents."
            )
            pair_feedback = _h007_incomplete_repair_pair_feedback(
                repair_job_diagnostic)
            if pair_feedback:
                repair_instruction += " " + pair_feedback
            repair_instruction = (
                "TOP PRIORITY — complete the still-unqualified named failure/repair "
                "pair(s) before optional probes, unrelated test jobs, or further reviewer "
                "polling. For each seed already observed failed, inspect its public failure "
                "result and make a correctness-bearing source edit before submitting its "
                "repair test; do not rerun unchanged source. Keep the gate pending until the "
                "exact failed → edit → distinct completed repair sequence is observed and "
                "recorded. Then continue the other required work. "
                + repair_instruction
            )
    call_count = len(calls)
    request_limit = (provider_request_limit
                     if provider_request_limit is not None else 750)
    call_budget_instruction = (
        f" The case allows at most {tool_call_limit} native calls; {call_count} are "
        f"already recorded, leaving {max(0, tool_call_limit - call_count)}. Keep the "
        "remaining calls within that limit, avoid repeating checks on unchanged source, "
        "and do not start another operation if it would exceed the budget."
        if tool_call_limit is not None else ""
    )
    if remaining_work_seconds is None:
        time_instruction = ""
    else:
        remaining_minutes = max(0, int(remaining_work_seconds // 60))
        if remaining_work_seconds <= 600:
            time_instruction = (
                f" Only about {remaining_minutes} minutes remain before the reserved verifier "
                "window. Stop optional edits and exploration now. Complete the final required "
                "workspace/Clippy checks, collect the one reviewer result if terminal, and "
                "write/read back the evidence-backed final continuation state in this turn. "
                "Keep any genuinely failing gate pending; do not claim it passed."
            )
        elif remaining_work_seconds <= 900:
            time_instruction = (
                f" About {remaining_minutes} minutes remain before the reserved verifier window. "
                "Do not start optional correctness experiments or expand scope. Prioritize the "
                "remaining required dependent stage, final workspace/Clippy checks, reviewer "
                "result, and final continuation evidence."
            )
        else:
            time_instruction = (
                f" About {remaining_minutes} minutes remain before the reserved verifier window. "
                "Keep implementation within the required stages; do not add optional work once "
                "the 30-minute active-work minimum is met and required qualification work remains."
            )
    if gate_checklist_required:
        stage_ledger_instruction = (
            "Keep `completed_phases` in the frozen seven-stage H007 order: "
            + ", ".join(f"`{phase}`" for phase in H007_S018_REQUIRED_STAGE_ORDER)
            + ". Track the same seven stages under `required_stages/next_stage`. Track the "
            "five exact contract.json phases separately under `contract_phases/"
            "next_contract_phase`, in order: "
            + ", ".join(f"`{phase}`" for phase in H007_S018_CONTRACT_PHASE_ORDER)
            + ". Set `next_phase: none` only when all seven H007 phases complete; set "
            "`next_stage: none` only when its seven-stage checklist is complete; set "
            "`next_contract_phase: none` only when all five contract phases are complete. "
            if separate_stage_ledger else ""
        )
        stage_completion_instruction = (
            "Once the seven H007 stages, five contract phases, and twelve gates are complete, "
            "set `next_phase: none`, `next_stage: none`, and `next_contract_phase: none`. "
            if separate_stage_ledger else
            "Once all seven stages and twelve gates are complete, set `next_phase: none`. "
        )
        s018_footer = (
            " Continue in this same CodeGG session and workspace. Use CONTINUATION.md as the "
            "canonical checklist; no todo/todowrite tool is available, and a task board is not "
            "a substitute. Keep every stage and gate pending until its evidence exists. "
            + stage_ledger_instruction +
            "Complete exactly these seven dependent stages in order: "
            + ", ".join(f"`{phase}`" for phase in phase_order) + ". Complete both observed "
            "starter failure-to-repair cycles, the exact release-scale overlap, all bounded "
            "probes, the acceptance audit and helper self-test, the offline locked workspace "
            "suite, Clippy with `-D warnings`, and one terminal read-only subagent review. "
            "Preserve the original frozen inputs, independent verifier boundary, and seeded "
            "repair evidence. Do not repeat a passing full workspace test on unchanged source. "
            "When a native tool returns an error, use its typed message to correct the "
            "arguments; do not replay identical failed inputs. Any Cargo, Clippy, scale, "
            "workspace, or acceptance operation likely to exceed 60 seconds must use a "
            "background native job. After each submission, make the next native call `job "
            "status` for that exact returned ID; never use `job wait`. For the release overlap, "
            "make the required dependent edit after observing that exact ID running and before "
            "polling it again. In CONTINUATION.md record the exact command and ID, then write "
            "the separate literal lines `job_id_overlap_running: running` and "
            "`job_id_overlap_terminal: completed` only after those exact native observations. "
            "Read the completed note back before the unchanged verifier runs. "
            "Do not claim a gate from prose alone or replace one job ID with another. "
            + stage_completion_instruction + "Complete at "
            "least 30 minutes of substantive active engineering, targeting 32 minutes. If the "
            "contract and gates finish early, continue with bounded correctness-bearing "
            "restart and failure-preservation cases in the existing packages; do not pad with "
            "waits, no-op work, or unchanged commands. Then stop and let the runner execute "
            "the unchanged independent verifier. Use only " + str(request_limit) +
            " provider requests and stay within the case's native-call, 3,600-second, "
            "10-GiB free-disk, and 300-second activity limits. Provider allowance and billed "
            "USD are separate; report billed USD as unknown if the route does not supply it."
            + call_budget_instruction
        )
        priority_prefixes: list[str] = []
        reviewer_due_now = (
            gate_checklist_required
            and measured_span >= review_start_active_ms
            and not verifier_spawn_attempts
        )
        if reviewer_due_now:
            priority_prefixes.append(
                "FIRST NATIVE ACTION — spawn exactly one native read-only task with "
                "`action: spawn` and `agent: verifier` now. Do not make a read, write, "
                "job-poll, or other native call before this reviewer spawn. Capture the exact "
                "Task and Run IDs, then continue required implementation while the reviewer "
                "runs; never spawn a replacement."
            )
        if no_progress_boundary_count >= 2 and gate_checklist_required:
            first_missing_gate = next((gate for gate in gate_order
                                       if gate_statuses is None or gate_statuses.get(gate)
                                       not in {"complete", "completed"}), None)
            if first_missing_gate is not None:
                progress_target = f"gate `{first_missing_gate}`"
            elif missing:
                progress_target = f"stage `{missing[0]}`"
            else:
                progress_target = "the first incomplete required checkpoint item"
            priority_prefixes.append(
                "NO-PROGRESS ESCALATION — two prompt boundaries have passed without "
                f"advancing {progress_target}. Work on that exact item now, produce its "
                "acceptance evidence, then update and read CONTINUATION.md back. Defer "
                "optional probes and unrelated checks."
            )
        if (gate_checklist_required and not _h007_s018_overlap_confirmed(continuation, calls)
                and _h007_overlap_needs_productive_work(continuation, calls)):
            priority_prefixes.append(
                "REQUIRED OVERLAP ACTION — the exact release job is observed running and no "
                "productive work followed its latest running observation. Before polling that "
                "ID again, make one correctness-bearing edit in the next dependent crate; then "
                "query `job status` for the same ID. Do not submit a replacement job."
            )
        prompt = (
            "Your previous turn ended normally, but the runner has not confirmed qualification. "
            + repair_instruction + " " + phase_instruction + " " + gate_instruction + " "
            + overlap_instruction + " " + reviewer_instruction + job_instruction
            + time_instruction + s018_footer
        )
        return ("\n\n".join(priority_prefixes) + "\n\n" if priority_prefixes else "") \
            + prompt
    return (
        "Your previous turn ended normally, but the runner has not confirmed qualification. "
        + repair_instruction + " " + phase_instruction + " " + gate_instruction + " "
        + overlap_instruction + " " + reviewer_instruction
        + " Continue in this same CodeGG session and workspace. Preserve "
        "the original fixture contract, frozen inputs, independent verifier boundary, and "
        "seeded repair evidence. Do not repeat a passing full workspace test on unchanged "
        "source. When a native tool returns an error, use its typed message to correct the "
        "arguments; do not replay identical failed inputs. The fixture's documented negative-ID "
        "probe is a separate expected refusal, not an ID to reuse. Keep working through "
        "the first missing dependent phase, then run the one "
        "required priority job and final locked workspace sweep at their specified points. "
        "Use only `job status` for polling; never use `job wait`. After each job submission, "
        "make the next native tool call `job status` for that exact ID. Record observed job "
        "evidence in CONTINUATION.md with the exact overlap ID as `job_id_overlap: <ID>`, "
        "`job_id_overlap_running: running`, and `job_id_overlap_terminal: completed` "
        "(the required `job_id: <exact overlap ID>` record); "
        "keep `job_id_priority` for the separate "
        "post-bundle release job and `sweep_workspace` for the final workspace job. Never "
        "substitute one job's ID for another or claim completion without matching native "
        "observations. Once all fifteen phases, the exact job evidence, and final workspace "
        "verification are complete, write one concise final audit and stop; do not repeat "
        "completed audits. Preserve the same " + str(request_limit) + "-request, "
        "3,600-second, 10-GiB disk, and 300-second activity limits. Do not add idle/no-op work; "
        "continue until the public contract and independent verifier are complete, or report "
        "a genuine blocker." + time_instruction + call_budget_instruction
    )


def _h007_followup_decision(*, stop_reason: Optional[str], continuation_confirmed: bool,
                            followup_count: int,
                            native_progress: bool,
                            remaining_wallclock_seconds: Optional[float],
                            active_work_span_ms: Optional[int] = None,
                            minimum_active_work_span_ms: Optional[int] = None) -> str:
    """Decide whether an incomplete normal end may continue in the same run."""
    if stop_reason != "end_turn":
        return "agent_terminal"
    horizon_complete = (
        minimum_active_work_span_ms is None or active_work_span_ms is None
        or active_work_span_ms >= minimum_active_work_span_ms
    )
    if continuation_confirmed and horizon_complete:
        return "continuation_complete"
    if not native_progress:
        return "no_native_progress"
    if followup_count >= H007_MAX_AUTONOMOUS_FOLLOWUP_TURNS:
        return "followup_limit"
    if remaining_wallclock_seconds is not None and remaining_wallclock_seconds <= 0:
        return "wallclock_cap"
    if (remaining_wallclock_seconds is not None
            and remaining_wallclock_seconds <= H006_H007_VERIFIER_RESERVE_SECONDS):
        return "verifier_reserve"
    return "continue"


def _provider_request_admissions(stderr_path: Path) -> Optional[int]:
    """Count safe provider-boundary request admissions from JSONL or CodeGG logs."""
    try:
        contents = stderr_path.read_text(encoding="utf-8")
    except OSError:
        return None
    if contents.lstrip().startswith("{"):
        try:
            rows = [json.loads(line) for line in contents.splitlines() if line.strip()]
        except (json.JSONDecodeError, UnicodeError):
            return None
        requests = [row for row in rows if isinstance(row, dict)
                    and row.get("kind", "request") == "request"]
        if all(isinstance(row.get("request_number"), int)
               and isinstance(row.get("limit"), int) for row in requests):
            return len(requests)
        return None
    return sum(1 for line in contents.splitlines()
               if "harness provider request admitted" in line
               and re.search(r"\brequest_number=\d+\b", line))


def _remaining_provider_request_cap(policy: budget.BudgetPolicy,
                                   guard: budget.SpendGuard) -> Optional[int]:
    """Return the request budget this lane may consume from a shared suite cap."""
    limit = policy.max_provider_requests
    if limit is None:
        return None
    return max(0, limit - guard.provider_requests)


def _preflight_h006_binary(binary: Path) -> Path:
    """Resolve CodeGG and require its trusted sandbox runfile before H006."""
    executable = binary.expanduser().resolve()
    if not executable.is_file() or not os.access(executable, os.X_OK):
        raise HarnessError(f"H006 CodeGG executable is not executable: {executable}")

    helper_name = "codegg-sandbox-helper.exe" if os.name == "nt" else "codegg-sandbox-helper"
    env = {key: os.environ[key] for key in
           ("PATH", "HOME", "USERPROFILE", "SYSTEMROOT", "WINDIR", "TMP", "TEMP")
           if key in os.environ}
    try:
        result = subprocess.run(
            [str(executable), "doctor", "installation"],
            cwd=str(executable.parent), capture_output=True, text=True,
            timeout=H006_INSTALLATION_PREFLIGHT_TIMEOUT_SECONDS, check=False, env=env,
        )
    except subprocess.TimeoutExpired as error:
        raise HarnessError("H006 CodeGG installation preflight timed out") from error
    except OSError as error:
        raise HarnessError(f"H006 CodeGG installation preflight could not run: {error}") from error

    report = (result.stdout or "") + "\n" + (result.stderr or "")
    prefix = f"Sandbox helper ({helper_name}): ".casefold()
    status_line = next((line.strip() for line in report.splitlines()
                        if line.strip().casefold().startswith(prefix)), None)
    if result.returncode != 0 or status_line is None \
            or not status_line.casefold().startswith(prefix + "present at "):
        detail = status_line or f"installation doctor exited {result.returncode} without helper status"
        raise HarnessError(f"H006 CodeGG runfile preflight failed: {detail}")
    return executable


def _is_missing_trusted_helper_failure(notification: dict[str, Any], *,
                                       tool_name: Optional[str] = None) -> bool:
    """Recognize the known, unrecoverable H006 process-launch failure.

    ACP carries the tool name on ``tool_call`` and carries only the matching
    ``toolCallId`` on ``tool_call_update``. The watcher supplies the name it
    correlated from the start event; terminal-event payloads are not expected
    to repeat it.
    """
    params = notification.get("params", {})
    update = params.get("update", {}) if isinstance(params, dict) else {}
    if not isinstance(update, dict) or update.get("sessionUpdate") != "tool_call_update" \
            or update.get("status") != "failed" \
            or tool_name not in {"test", "job"}:
        return False
    raw_output = update.get("rawOutput")
    if not isinstance(raw_output, str):
        return False
    lowered = raw_output.casefold()
    return "trusted sandbox helper" in lowered \
        and "codegg-sandbox-helper" in lowered \
        and "could not be resolved" in lowered


class _H006InfrastructureFailureWatcher:
    """Cancel H006 when its managed process cannot launch for a known reason."""

    def __init__(self, client: AcpClient, session_id: str, *,
                 poll_seconds: float = 0.05) -> None:
        self._client = client
        self._session_id = session_id
        self._poll_seconds = poll_seconds
        self._absorbed = 0
        self._tool_names_by_id: dict[str, str] = {}
        self._stop = threading.Event()
        self._thread: Optional[threading.Thread] = None
        self.failure_observed = False
        self.cancel_error: Optional[str] = None
        self.cancel_requested = False

    def start(self) -> None:
        self._thread = threading.Thread(target=self._run, name="h006-infrastructure-watcher",
                                        daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        if self._thread is not None:
            self._thread.join(timeout=5.0)
            self._thread = None
        self._absorb()
        if self.failure_observed:
            self._cancel()

    def _run(self) -> None:
        while not self._stop.is_set():
            self._absorb()
            if self.failure_observed:
                self._cancel()
                return
            self._stop.wait(self._poll_seconds)
        self._absorb()

    def _cancel(self) -> None:
        if self.cancel_requested:
            return
        self.cancel_requested = True
        try:
            self._client.notify("session/cancel", {"sessionId": self._session_id})
        except (HarnessError, OSError, subprocess.SubprocessError) as error:
            self.cancel_error = f"{type(error).__name__}: {error}"

    def _absorb(self) -> None:
        notifications = self._client.notifications
        while self._absorbed < len(notifications):
            notification = notifications[self._absorbed]
            self._absorbed += 1
            params = notification.get("params", {})
            update = params.get("update", {}) if isinstance(params, dict) else {}
            if not isinstance(update, dict):
                continue
            kind = update.get("sessionUpdate")
            call_id = update.get("toolCallId")
            if not isinstance(call_id, str):
                continue
            if kind == "tool_call":
                title = update.get("title")
                if isinstance(title, str) and title in {"test", "job"}:
                    self._tool_names_by_id[call_id] = title
                continue
            if kind != "tool_call_update":
                continue
            tool_name = self._tool_names_by_id.pop(call_id, None)
            if _is_missing_trusted_helper_failure(notification, tool_name=tool_name):
                self.failure_observed = True


def _provider_allowance_from_receipt(receipt_path: Path) -> Optional[float]:
    """Sum durable OpenCode Go allowance observations without reading model text."""
    try:
        rows = [json.loads(line) for line in receipt_path.read_text(encoding="utf-8").splitlines()
                if line.strip()]
    except (OSError, json.JSONDecodeError, UnicodeError):
        return None
    observations = [row for row in rows if isinstance(row, dict)
                    and row.get("kind") == "allowance_observed"]
    if not observations:
        return None
    if any(not isinstance(row.get("allowance_micros"), int)
           or row["allowance_micros"] < 0 for row in observations):
        return None
    return sum(row["allowance_micros"] for row in observations) / 1_000_000


def run_case(binary: Path, case: AutonomyCase, model: str, *,
             budget_policy: budget.BudgetPolicy, lane_root: Path,
             repository: Optional[Path] = None, profile: str = "workspace-write",
             timeout: float = 900.0, agent: str = "build",
             credential: Optional[str] = None,
             guard: Optional[budget.SpendGuard] = None,
             min_free_disk_bytes: Optional[int] = None,
             requested_manifest_sha256: Optional[str] = None,
             disable_placement_advisor: bool = False) -> dict[str, Any]:
    """Run one taxonomy case against a live provider route and return a receipt.

    The receipt separates three things that are easy to conflate: what the
    agent did (the transcript), whether the turn reached a terminal, and
    whether the case's independent verifier confirmed the task. A case is
    ``solved`` only when all three agree.

    ``guard`` lets a caller share one spend guard across a suite, so a
    declared request cap covers the whole suite instead of restarting at
    every case.

    A case may declare a ``fault``. The fault is applied to the lane's own
    copy of the fixture before the prompt is submitted, so the agent meets a
    deterministic failure at the tool boundary while the instruction never
    names it. The receipt records the fault, what the transcript shows about
    it, and whether the declared bound stopped the trial.
    """
    if "/" not in model:
        raise HarnessError("model must be a provider/model composite")
    if case.case_id == H006_CASE_ID:
        if budget_policy.max_provider_requests is None:
            raise HarnessError("H006 sustained qualification requires an explicit provider request cap")
        if model != "opencode_go/muse-spark-1.3-contributor":
            raise HarnessError("H005 is authorized only for opencode_go/muse-spark-1.3-contributor")
        if timeout <= 0 or timeout > 3600:
            raise HarnessError("H005 timeout must be positive and no greater than 3600 seconds")
        if min_free_disk_bytes != 10 * 1024**3:
            raise HarnessError("H005 requires the approved 10 GiB free-disk floor")
    elif case.case_id == H007_CASE_ID:
        if budget_policy.max_provider_requests is None:
            raise HarnessError("H007 requires a fresh explicit provider request cap")
        if budget_policy.max_provider_requests > int(case.budget["max_provider_requests"]):
            raise HarnessError("H007 provider request cap exceeds this case manifest ceiling")
        if budget_policy.max_wallclock_seconds is None \
                or budget_policy.max_wallclock_seconds > 3600:
            raise HarnessError("H007 requires an explicit wall-clock cap no greater than 3600 seconds")
        _validate_h007_model_route(model)
        if timeout <= 0 or timeout > 3600:
            raise HarnessError("H007 timeout must be positive and no greater than 3600 seconds")
        if min_free_disk_bytes is None or min_free_disk_bytes < 10 * 1024**3:
            raise HarnessError("H007 requires an explicit free-disk floor of at least 10 GiB")
    elif min_free_disk_bytes is not None and min_free_disk_bytes <= 0:
        raise HarnessError("minimum free disk bytes must be a positive integer")
    if profile not in {"full-host", "workspace-write"}:
        raise HarnessError("profile must be full-host or workspace-write")
    if case.support != "supported":
        raise HarnessError(f"case {case.case_id} is deferred, not runnable: "
                           f"{case.deferral_reason or 'no reason recorded'}")

    repository = repository or _repo_root()
    source = case_root(repository, case.case_id)
    if not (source / "fixture").is_dir() or not (source / "verifier").is_dir():
        raise HarnessError(f"the case body is missing: {source}")

    # The agent may edit the case's fixture and nothing else. The check is
    # the same one the candidate scope guard uses, so a verifier that a
    # candidate could reach is refused before a provider is ever called.
    verify_isolation(AutonomySuite(version=autonomy.AUTONOMY_SCHEMA_VERSION,
                                   cases=(case,), suite_sha256=""),
                     [f"{AUTONOMY_ROOT_RELPATH}/{case.case_id}/fixture"])

    if guard is None:
        admission = budget.admit_budget(budget_policy)
        if admission.status != budget.STATUS_ADMITTED:
            raise HarnessError("budget admission refused before the autonomy case started: "
                               + json.dumps(admission.as_dict(), sort_keys=True))
        guard = budget.SpendGuard(budget_policy)
    lane_root = lane_root.resolve()
    disk_probe = _nearest_existing_directory(lane_root)
    try:
        initial_free_bytes = shutil.disk_usage(disk_probe).free
    except OSError as error:
        if min_free_disk_bytes is not None:
            raise HarnessError("free disk state is unreadable; refusing the live case") from error
        initial_free_bytes = None
    if min_free_disk_bytes is not None and initial_free_bytes is not None \
            and initial_free_bytes < min_free_disk_bytes:
        raise HarnessError("free disk is below the required floor; refusing the live case")
    if case.case_id in {H006_CASE_ID, H007_CASE_ID}:
        # Check required managed runfiles before budget reservation or ACP startup.
        binary = _preflight_h006_binary(binary)
    case_seconds = float(case.budget["max_wallclock_seconds"])
    timeout = _turn_timeout(case.case_id, timeout, case_seconds,
                             guard.remaining_wallclock_seconds())

    disk_before_bytes = _directory_size_bytes(lane_root)
    supervisor.private_dir(lane_root)
    # Each case starts a fresh provider process, so its process-local cap must
    # be the unspent suite allowance, not the original full-suite limit.
    provider_request_limit = _remaining_provider_request_cap(budget_policy, guard)
    # The request cap is reserved only after the disk floor's initial sample.
    guard.admit()
    # The daemon binds its control socket inside the lane. A lane whose socket
    # path does not fit the platform's ``sun_path`` limit cannot start, and
    # the daemon's own failure message names the socket rather than the
    # cause. The runner refuses the lane up front instead, with the reason.
    socket_path = lane_root / "daemon" / "core.sock"
    if len(str(socket_path).encode()) >= supervisor.SUN_PATH_LIMIT:
        raise HarnessError(
            f"the lane root {lane_root} puts the daemon control socket at "
            f"{socket_path}, which is longer than the platform's sun_path limit; "
            "use a shorter lane root under ~/.codegg-harness-lanes")
    # The agent's workspace root holds the editable tree only. The verifier
    # and the transcript live in a private sibling directory, outside the
    # root the agent's tools are bound to, so the hidden contract cannot be
    # read or edited by the agent under test.
    agent_root = lane_root / "case"
    private_root = lane_root / "private"
    fixture_destination = agent_root / "fixture"
    if agent_root.exists():
        shutil.rmtree(agent_root)
    if private_root.exists():
        shutil.rmtree(private_root)
    fixture_destination.mkdir(parents=True, exist_ok=True)
    private_root.mkdir(parents=True, exist_ok=True)
    _seed_private_root_canary(private_root)
    _copy_tree(source / "fixture", fixture_destination)
    _seed_private_root_canary_link(fixture_destination, private_root)
    _copy_tree(source / "verifier", private_root / "verifier")
    if case.case_id == H006_CASE_ID:
        metadata = repository / "dev/harness_lab/fixtures/sustained_event_ledger.case.json"
        if not metadata.is_file():
            raise HarnessError("the H005 frozen case metadata is missing")
        shutil.copy2(metadata, private_root / "case-metadata.json")
    elif case.case_id == H007_CASE_ID:
        metadata = repository / "dev/harness_lab/fixtures/build_provenance_catalog.case.json"
        if not metadata.is_file():
            raise HarnessError("the H007 frozen case metadata is missing")
        shutil.copy2(metadata, private_root / "case-metadata.json")
    if case.family == "delegation":
        # Managed placement requires a real repository identity. Initialize a
        # private, lane-local Git repository around only the editable fixture;
        # the hidden verifier stays in the sibling private root.
        subprocess.run(["git", "init", "--quiet", str(agent_root)], check=True,
                       capture_output=True, text=True)
        subprocess.run(["git", "-C", str(agent_root), "add", "fixture"], check=True,
                       capture_output=True, text=True)
        subprocess.run(["git", "-C", str(agent_root), "-c", "user.name=Harness Lab",
                        "-c", "user.email=harness@example.invalid", "commit", "--quiet",
                        "-m", "frozen placement fixture"], check=True,
                       capture_output=True, text=True)

    spec = SubjectSpec.create(
        f"autonomy-{case.case_id}", binary, agent_root, lane_root, repository,
        provider_request_limit=provider_request_limit,
    )
    trace_recorder = causal_trace.Recorder(
        private_root / "causal-trace.jsonl", run_id=uuid.uuid4().hex,
        case_id=case.case_id, source_sha256=spec.revision,
        binary_sha256=spec.binary_sha256)
    env = spec.environment()
    _set_harness_private_root_env(env, private_root)
    env.pop("CODEGG_HARNESS_DISABLE_PLACEMENT_ADVISOR", None)
    if disable_placement_advisor:
        env["CODEGG_HARNESS_DISABLE_PLACEMENT_ADVISOR"] = "1"
    if guard.policy.max_provider_requests is not None:
        remaining_requests = min(
            guard.policy.max_provider_requests - max(0, guard.provider_requests - 1),
            int(case.budget["max_provider_requests"]),
        )
        if remaining_requests <= 0:
            raise HarnessError("provider request budget is exhausted before the case starts")
        env["CODEGG_HARNESS_MAX_PROVIDER_REQUESTS"] = str(remaining_requests)
        env["CODEGG_HARNESS_PROVIDER_REQUEST_RECEIPT"] = str(
            spec.root / "artifacts" / "provider-request-admissions.jsonl")
    if guard.policy.cost_limit_usd is not None:
        remaining_allowance = guard.policy.cost_limit_usd - guard.cost_usd
        if remaining_allowance <= 0:
            raise HarnessError("provider allowance budget is exhausted before the case starts")
        env["CODEGG_HARNESS_MAX_PROVIDER_ALLOWANCE_USD"] = str(
            remaining_allowance)
    _prepare_lane_home(env)

    key = credential if credential is not None else opencode_go_credential()
    if not key:
        raise HarnessError("no OpenCode Go credential is available for the approved route")
    env["OPENCODE_GO_API_KEY"] = key

    config_path = Path(env["CODEGG_TUI_CONFIG"])
    config_path.parent.mkdir(parents=True, exist_ok=True)
    run_config: dict[str, Any] = {"model": model}
    if case.case_id == H007_CASE_ID:
        # Keep H007 on its explicitly qualified cloud-route context profile.
        # The original correction followed a DeepSeek OpenCode Go run where
        # the generic name heuristic selected a 32k local profile despite a
        # 1,048,576-token route limit. Durable hybrid compaction stays enabled
        # for the current Muse OpenCode Go route as well; no profile is
        # inferred from whichever model name happens to be selected.
        model_id = model.rsplit("/", 1)[-1]
        run_config["model_profile"] = {
            model_id: {"context_window": 1_048_576},
        }
        subagent_config = _h007_subagent_config(case.case_id)
        if subagent_config is not None:
            run_config["subagent"] = subagent_config
        run_config["compaction"] = {
            "enabled": True,
            "auto": True,
            "mode": "hybrid",
            "policy": "balanced",
            "prune": True,
            "threshold": 0.65,
            "reserved": 16_000,
            "max_tool_output_tokens": 1_000,
            "max_summary_tokens": 800,
            "max_events": 50,
            "keep_recent_messages": 4,
            "validate": True,
            "preserve_evidence": True,
            "inject_context_frame": True,
        }
    config_path.write_text(json.dumps(run_config), encoding="utf-8")

    # A declared fault is applied to the lane's own copy of the fixture before
    # the prompt is submitted, so the agent meets a deterministic failure at
    # the tool boundary while the instruction never names it. A fault the seam
    # cannot honour refuses the trial: a case that declared a fault and ran
    # without one would read as a control that was exercised when it was not.
    fault_spec: Optional[faults.FaultSpec] = None
    applied_fault: Optional[faults.AppliedFault] = None
    fault_restore_error: Optional[str] = None
    if case.fault:
        try:
            fault_spec = faults.FaultSpec.parse(case.fault)
            applied_fault = faults.apply_fault(fault_spec, agent_root,
                                               private_root / "fault-stash")
        except faults.FaultError as error:
            raise HarnessError(f"case {case.case_id} declared a fault the seam cannot "
                               f"honour: {error}") from error

    client = AcpClient(binary, env, agent_root, timeout, profile)
    client.notification_observer = trace_recorder.observe
    started = time.monotonic()
    terminal = "failed"
    stop_reason: Optional[str] = None
    cancellation_source: Optional[str] = None
    failure: Optional[dict[str, Any]] = None
    result: dict[str, Any] = {}
    session_id: Optional[str] = None
    policy_receipt: Any = None
    watcher: Optional[_FaultWatcher] = None
    disk_watcher: Optional[_DiskFloorWatcher] = None
    stall_watcher: Optional[_ActivityStallWatcher] = None
    infrastructure_watcher: Optional[_H006InfrastructureFailureWatcher] = None
    resource_sampler: Optional[_ProcessTreeSampler] = None
    process_resources: Optional[dict[str, Any]] = None
    event_observer_drained = False
    daemon_process_count_after_cleanup: Optional[int] = None
    autonomous_followup_count = 0
    prompt_request_count = 0
    last_prompt_request_timeout_seconds: Optional[float] = None
    last_prompt_request_elapsed_ms: Optional[int] = None
    last_prompt_remaining_wallclock_seconds: Optional[float] = None
    turn_timeout_observed = False
    turn_timeout_remaining_wallclock_seconds: Optional[float] = None
    reviewer_due_prompt_count = 0
    first_reviewer_due_followup_number: Optional[int] = None
    first_reviewer_due_active_work_span_ms: Optional[int] = None
    continuation_stop_reason: Optional[str] = None
    last_prompt_tool_call_count = 0
    no_progress_boundary_count = 0
    prompt_boundary_rows: list[dict[str, Any]] = []
    current_prompt_boundary: Optional[dict[str, Any]] = None
    try:
        client.request("initialize", {"protocolVersion": ACP_PROTOCOL_VERSION,
                                      "clientInfo": {"name": "codegg-harness-lab", "version": "1"}})
        resource_sampler = _ProcessTreeSampler(client.process.pid)
        opened = client.request("session/new", {"cwd": str(agent_root),
                                                "title": f"harness-lab-autonomy-{case.case_id}"})
        session_id = opened.get("sessionId")
        if not isinstance(session_id, str) or not session_id:
            raise HarnessError("ACP did not return a session id for the autonomy case")
        trace_recorder.bind_daemon_generation(supervisor._metadata(spec, timeout).get("generation"))
        resource_sampler = _ProcessTreeSampler(client.process.pid)
        policy_receipt = opened.get("codeggPolicy")
        if case.case_id in {H006_CASE_ID, H007_CASE_ID}:
            infrastructure_watcher = _H006InfrastructureFailureWatcher(client, session_id)
            infrastructure_watcher.start()
        if fault_spec is not None:
            # The watcher stops the trial once the same call has failed the
            # declared number of times, so a faulted case is bounded even when
            # the agent never gives up.
            watcher = _FaultWatcher(client, fault_spec, session_id, applied=applied_fault)
            watcher.start()
        if min_free_disk_bytes is not None:
            disk_watcher = _DiskFloorWatcher(lane_root, min_free_disk_bytes,
                                             client, session_id)
            disk_watcher.start()
            if disk_watcher.error is not None or disk_watcher.triggered:
                stop_reason = "disk_floor"
                terminal = "infrastructure_error"
                failure = {"kind": "DiskFloor", "category": "infrastructure_error",
                           "usage_reason": disk_watcher.error or "minimum_free_space_breached"}
                raise HarnessError("disk floor unavailable or already breached")
        if case.case_id == H007_CASE_ID:
            stall_watcher = _ActivityStallWatcher(
                client, session_id, int(case.abort["stall_seconds"]))
            stall_watcher.start()
        prompt_text = case.prompt
        while True:
            remaining_seconds = guard.remaining_wallclock_seconds()
            request_timeout = _turn_timeout(
                case.case_id, timeout, case_seconds, remaining_seconds)
            if request_timeout <= 0:
                stop_reason = "verifier_reserve"
                continuation_stop_reason = "verifier_reserve"
                if terminal != "completed":
                    terminal = "timeout"
                result = {"error": "stopped agent prompts to preserve verifier reserve"}
                break
            prompt_request_count += 1
            last_prompt_request_timeout_seconds = request_timeout
            last_prompt_remaining_wallclock_seconds = remaining_seconds
            prompt_started = time.monotonic()
            if case.case_id == H007_CASE_ID:
                # A prompt is a new boundary: an earlier end_turn must never
                # survive if this request later fails or is cancelled.
                stop_reason = None
                calls_at_prompt_start = _observed_tool_calls(client)
                progress_start = _h007_prompt_progress_snapshot(
                    fixture_destination, calls_at_prompt_start, case_prompt=case.prompt,
                )
                work_start = _h007_prompt_work_accounting(
                    run_limit_seconds=case_seconds,
                    remaining_wallclock_seconds=remaining_seconds,
                )
                current_prompt_boundary = {
                    "request_index": prompt_request_count,
                    "started_elapsed_ms": max(0, int((prompt_started - started) * 1000)),
                    "remaining_wallclock_start_ms": (
                        max(0, int(remaining_seconds * 1000))
                        if isinstance(remaining_seconds, (int, float)) else None
                    ),
                    "request_timeout_ms": max(0, int(request_timeout * 1000)),
                    "request_elapsed_ms": None,
                    "active_work_span_start_ms": _active_work_span_ms(
                        calls_at_prompt_start),
                    "active_work_span_end_ms": None,
                    "tool_call_count_start": len(calls_at_prompt_start),
                    "tool_call_count_end": len(calls_at_prompt_start),
                    "prompt_progress_start": progress_start,
                    "prompt_progress_end": None,
                    "prompt_work_limit_ms": work_start["prompt_work_limit_ms"],
                    "prompt_work_consumed_ms_start": work_start[
                        "prompt_work_consumed_ms"],
                    "prompt_work_remaining_ms_start": work_start[
                        "prompt_work_remaining_ms"],
                    "prompt_work_consumed_ms_end": None,
                    "prompt_work_remaining_ms_end": None,
                    "verifier_reserve_ms": work_start["verifier_reserve_ms"],
                    "no_progress_boundary_count_before": no_progress_boundary_count,
                    "no_progress_boundary_count": None,
                    "stop_class": "request_failure",
                    "continuation_decision": None,
                    "reviewer_due_marker_received": (
                        H007_S018_FINAL_REVIEW_START_MARKER in prompt_text
                        if _h007_review_uses_30_minute_threshold(case.prompt)
                        else H007_S018_REVIEW_START_MARKER in prompt_text
                    ),
                    "reviewer_due_marker_sent_next": None,
                }
            try:
                result = client.request("session/prompt", {
                    "sessionId": session_id,
                    "model": model,
                    "codeggAgent": agent,
                    "prompt": [{"type": "text", "text": prompt_text}],
                }, timeout=request_timeout)
            finally:
                last_prompt_request_elapsed_ms = int(
                    (time.monotonic() - prompt_started) * 1000)
                if current_prompt_boundary is not None:
                    calls_at_prompt_end = _observed_tool_calls(client)
                    current_prompt_boundary["request_elapsed_ms"] = (
                        last_prompt_request_elapsed_ms)
                    current_prompt_boundary["active_work_span_end_ms"] = (
                        _active_work_span_ms(calls_at_prompt_end))
                    current_prompt_boundary["tool_call_count_end"] = len(
                        calls_at_prompt_end)
                    current_prompt_boundary["prompt_progress_end"] = (
                        _h007_prompt_progress_snapshot(
                            fixture_destination, calls_at_prompt_end,
                            case_prompt=case.prompt,
                        )
                    )
                    work_end = _h007_prompt_work_accounting(
                        run_limit_seconds=case_seconds,
                        remaining_wallclock_seconds=guard.remaining_wallclock_seconds(),
                    )
                    current_prompt_boundary["prompt_work_consumed_ms_end"] = (
                        work_end["prompt_work_consumed_ms"])
                    current_prompt_boundary["prompt_work_remaining_ms_end"] = (
                        work_end["prompt_work_remaining_ms"])
                    no_progress_boundary_count = _h007_boundary_no_progress_count(
                        current_prompt_boundary["no_progress_boundary_count_before"],
                        current_prompt_boundary["prompt_progress_start"],
                        current_prompt_boundary["prompt_progress_end"],
                    )
                    current_prompt_boundary["no_progress_boundary_count"] = (
                        no_progress_boundary_count)
                    prompt_boundary_rows.append(current_prompt_boundary)
                    current_prompt_boundary = None
            raw_stop_reason = result.get("stopReason") if isinstance(result, dict) else None
            stop_reason = (_h007_prompt_stop_class(raw_stop_reason)
                           if case.case_id == H007_CASE_ID else raw_stop_reason)
            if case.case_id == H007_CASE_ID and prompt_boundary_rows:
                prompt_boundary_rows[-1]["stop_class"] = stop_reason
            terminal = "cancelled" if stop_reason in ("cancelled", "canceled") else "completed"
            if case.case_id != H007_CASE_ID:
                break

            # ACP's end_turn is a turn boundary, not proof that the benchmark
            # task is complete. Preserve the same session and enforce the
            # existing run-wide request, wall-clock, disk, and native-activity
            # caps when the public continuation record still shows work.
            try:
                client.drain(timeout=min(2.0, max(0.2, request_timeout / 10)))
            except (HarnessError, OSError, subprocess.SubprocessError):
                pass
            observed_calls = _observed_tool_calls(client)
            if prompt_boundary_rows:
                latest_boundary = prompt_boundary_rows[-1]
                progress_end = _h007_prompt_progress_snapshot(
                    fixture_destination, observed_calls, case_prompt=case.prompt,
                )
                latest_boundary["prompt_progress_end"] = progress_end
                work_end = _h007_prompt_work_accounting(
                    run_limit_seconds=case_seconds,
                    remaining_wallclock_seconds=guard.remaining_wallclock_seconds(),
                )
                latest_boundary["prompt_work_consumed_ms_end"] = (
                    work_end["prompt_work_consumed_ms"])
                latest_boundary["prompt_work_remaining_ms_end"] = (
                    work_end["prompt_work_remaining_ms"])
                no_progress_boundary_count = _h007_boundary_no_progress_count(
                    latest_boundary.get("no_progress_boundary_count_before", 0),
                    latest_boundary["prompt_progress_start"], progress_end,
                )
                latest_boundary["no_progress_boundary_count"] = no_progress_boundary_count
            continuation_complete = _h007_continuation_confirmed(
                observed_calls, fixture_destination, case_prompt=case.prompt)
            continuation_stop_reason = _h007_followup_decision(
                stop_reason=stop_reason,
                continuation_confirmed=continuation_complete,
                followup_count=autonomous_followup_count,
                native_progress=len(observed_calls) > last_prompt_tool_call_count,
                remaining_wallclock_seconds=guard.remaining_wallclock_seconds(),
                active_work_span_ms=_active_work_span_ms(observed_calls),
                minimum_active_work_span_ms=H007_MIN_ACTIVE_DURATION_MS,
            )
            if prompt_boundary_rows:
                prompt_boundary_rows[-1]["continuation_decision"] = (
                    continuation_stop_reason
                )
            if continuation_stop_reason != "continue":
                break
            autonomous_followup_count += 1
            last_prompt_tool_call_count = len(observed_calls)
            remaining_work_seconds = guard.remaining_wallclock_seconds()
            if remaining_work_seconds is not None:
                remaining_work_seconds = max(
                    0.0, remaining_work_seconds - H006_H007_VERIFIER_RESERVE_SECONDS)
            prompt_text = _h007_followup_prompt(
                fixture_destination, observed_calls,
                case.budget.get("max_tool_calls"),
                case.budget.get("max_provider_requests"),
                case_prompt=case.prompt,
                active_work_span_ms=_active_work_span_ms(observed_calls),
                continuation_complete=continuation_complete,
                remaining_work_seconds=remaining_work_seconds,
                no_progress_boundary_count=no_progress_boundary_count,
            )
            review_marker = (
                H007_S018_FINAL_REVIEW_START_MARKER
                if _h007_review_uses_30_minute_threshold(case.prompt)
                else H007_S018_REVIEW_START_MARKER
            )
            if review_marker in prompt_text:
                reviewer_due_prompt_count += 1
                if first_reviewer_due_followup_number is None:
                    first_reviewer_due_followup_number = autonomous_followup_count
                    first_reviewer_due_active_work_span_ms = _active_work_span_ms(
                        observed_calls)
            if prompt_boundary_rows:
                prompt_boundary_rows[-1]["reviewer_due_marker_sent_next"] = (
                    review_marker in prompt_text)
    except HarnessTurnFailure as error:
        continuation_stop_reason = "turn_failure"
        if case.case_id == H007_CASE_ID:
            stop_reason = _h007_prompt_stop_class(None, failure_category=error.category)
        if prompt_boundary_rows:
            prompt_boundary_rows[-1]["continuation_decision"] = "turn_failure"
            if case.case_id == H007_CASE_ID:
                prompt_boundary_rows[-1]["stop_class"] = stop_reason
        failure = {"kind": type(error).__name__, "category": error.category,
                   "usage_reason": error.usage_unavailable_reason}
        if error.category == "provider_timeout":
            trace_recorder.record("provider_timeout")
        # A cancelled turn is a terminal of its own, not a generic failure.
        terminal = "cancelled" if error.category == "cancelled" else "failed"
    except HarnessTimeout as error:
        continuation_stop_reason = "turn_timeout"
        if prompt_boundary_rows:
            prompt_boundary_rows[-1]["continuation_decision"] = "turn_timeout"
            prompt_boundary_rows[-1]["stop_class"] = "turn_timeout"
        turn_timeout_observed = True
        turn_timeout_remaining_wallclock_seconds = guard.remaining_wallclock_seconds()
        if case.case_id == H007_CASE_ID:
            stop_reason = "turn_timeout"
            if prompt_boundary_rows:
                prompt_boundary_rows[-1]["stop_class"] = "turn_timeout"
        terminal = "timeout"
        result = {"error": str(error)}
    finally:
        if disk_watcher is not None:
            disk_watcher.stop()
        if stall_watcher is not None:
            stall_watcher.stop()
        if watcher is not None:
            watcher.stop()
        try:
            client.drain(timeout=min(2.0, max(0.2, timeout / 10)))
            event_observer_drained = True
        except (HarnessError, OSError, subprocess.SubprocessError):
            pass
        if case.case_id == H007_CASE_ID and prompt_boundary_rows:
            final_calls = _observed_tool_calls(client)
            prompt_boundary_rows[-1]["active_work_span_end_ms"] = (
                _active_work_span_ms(final_calls))
            prompt_boundary_rows[-1]["tool_call_count_end"] = len(final_calls)
        if terminal == "cancelled":
            cause = "disk_floor" if disk_watcher is not None and disk_watcher.triggered else \
                "activity_stall" if stall_watcher is not None and stall_watcher.triggered else "user"
            trace_recorder.record("cancel_requested", detail=cause)
            if case.case_id == H007_CASE_ID:
                cancellation_source = _h007_project_cancellation_source(
                    "disk_floor" if disk_watcher is not None and disk_watcher.triggered else
                    "activity_stall" if stall_watcher is not None and stall_watcher.triggered else
                    "external_or_transport"
                )
        if infrastructure_watcher is not None:
            infrastructure_watcher.stop()
        if applied_fault is not None:
            fault_restore_error = applied_fault.restore()
        client.close()
        try:
            supervisor.stop_lane_daemon(spec)
        except (HarnessError, OSError, subprocess.TimeoutExpired) as error:
            daemon_stop_error = f"{type(error).__name__}: {error}"
        else:
            daemon_stop_error = None
        daemon_process_count_after_cleanup = _owned_lane_daemon_process_count(spec)
        if resource_sampler is not None:
            process_resources = resource_sampler.finish()

    duration_ms = int((time.monotonic() - started) * 1000)
    provider_attempt_diagnostic = (
        diagnostics.summarize_provider_attempt_log(
            agent_root / "codegg_debug.log", run_id=trace_recorder.run_id
        )
        if case.case_id == H007_CASE_ID else None
    )
    if infrastructure_watcher is not None and infrastructure_watcher.failure_observed:
        stop_reason = "missing_trusted_sandbox_helper"
        terminal = "infrastructure_error"
        failure = {
            "kind": "MissingTrustedSandboxHelper",
            "category": "infrastructure_error",
            "cancel_error": infrastructure_watcher.cancel_error,
        }
    tool_calls = _observed_tool_calls(client)
    if case.case_id == H007_CASE_ID:
        required_repair_ids = (
            h007_results.documented_repair_job_id_pairs(
                _h007_read_continuation(agent_root / "fixture"),
                H007_S018_REQUIRED_REPAIR_JOB_PAIRS,
            ) if _h007_case_requires_repair_job_pairs(case.prompt) else ()
        )
        observed_repair_cycles = h007_results._observed_repair_cycle_pairs(
            tool_calls, preferred_job_pairs=required_repair_ids,
        )
    else:
        observed_repair_cycles = []
    for pair in observed_repair_cycles:
        trace_recorder.record(
            "repair_cycle", detail="passed",
            elapsed_ms=pair["repair_terminal_elapsed_ms"],
            related_job_ids=(pair["failed_job_id"], pair["repair_job_id"]),
        )
    if _h007_continuation_confirmed(
            tool_calls, agent_root / "fixture", case_prompt=case.prompt):
        trace_recorder.record("continuation", elapsed_ms=duration_ms)
    observation_receipt = event_observation.observe_notifications(
        client.notifications, drained=event_observer_drained
    )
    agent_terminal = terminal
    if observation_receipt["event_gap_count"]:
        # The observation has a hole in it. The trial is unmeasured: it says
        # nothing about the candidate, and it is never counted as one.
        terminal = "infrastructure_error"
        failure = {"kind": "EventStreamGap", "category": "infrastructure_error",
                   "usage_reason": None}
    if disk_watcher is not None and (disk_watcher.triggered or disk_watcher.error is not None):
        terminal = "infrastructure_error"
        failure = {"kind": "DiskFloor", "category": "infrastructure_error",
                   "usage_reason": disk_watcher.error or "minimum_free_space_breached"}

    fault_receipt: Optional[dict[str, Any]] = None
    if fault_spec is not None:
        observation = faults.observe_fault(tool_calls, fault_spec)
        fault_receipt = dict(applied_fault.as_dict()) if applied_fault is not None \
            else fault_spec.as_dict()
        fault_receipt.update(observation)
        fault_receipt["bound_reached"] = bool(observation["bound_reached"]
                                              or (watcher is not None and watcher.bound_reached))
        fault_receipt["restore_error"] = fault_restore_error
        fault_receipt["cancel_error"] = watcher.cancel_error if watcher is not None else None
        fault_receipt["reapplied"] = watcher.reapplied if watcher is not None else 0

    transcript = build_transcript(case, tool_calls, terminal=terminal,
                                  stop_reason=stop_reason, failure=failure,
                                  fault=fault_receipt)
    transcript_path = private_root / "TRANSCRIPT.json"
    transcript_path.write_text(json.dumps(transcript, indent=2, sort_keys=True), encoding="utf-8")
    serialized_transcript_diagnostic = None
    if case.case_id == H007_CASE_ID:
        serialized_transcript_diagnostic = diagnostics.audit_serialized_transcript(
            transcript_path,
            tool_calls,
            expected_case_id=case.case_id,
            expected_schema_version=transcript["schema_version"],
        )

    # The verifier runs in the **host** environment, not the subject's. The
    # lane isolates HOME, CARGO_HOME and the config roots on purpose: the
    # subject under test must not see the developer's registry, config or
    # credential store. The verifier is harness code, and it has to be able
    # to run the very toolchain the developer's fixture was written for — a
    # rustup shim with an overridden HOME resolves no default toolchain and
    # every Rust case would fail for a reason that has nothing to do with the
    # agent. The verifier still only ever sees the case's own tree.
    verifier_env = dict(os.environ)
    verifier_env[CASE_ROOT_ENV] = str(agent_root)
    verifier_env[TRANSCRIPT_ENV] = str(transcript_path)
    verifier_timeout = _verifier_timeout(
        case.case_id, timeout, guard.remaining_wallclock_seconds())
    trace_recorder.record("verifier_started", elapsed_ms=duration_ms)
    verification = _run_verifier(list(case.verifier["command"]), private_root,
                                 verifier_env, verifier_timeout,
                                 diagnostics_only=case.case_id == H007_CASE_ID)
    trace_recorder.record("verifier_terminal", elapsed_ms=verification.get("duration_ms"),
                          detail=("passed" if verification.get("exit_code") == 0 else
                                  "failed" if isinstance(verification.get("exit_code"), int) else "unknown"))
    trace_receipt = trace_recorder.finish()

    raw_usage = result.get("codeggUsage") if isinstance(result, dict) else None
    usage = None
    if isinstance(raw_usage, dict):
        usage = {
            "turn_id": raw_usage.get("turnId"),
            "input_tokens": raw_usage.get("inputTokens"),
            "output_tokens": raw_usage.get("outputTokens"),
            "cached_tokens": raw_usage.get("cachedTokens"),
            "reasoning_tokens": raw_usage.get("reasoningTokens"),
            "provider_allowance_usd": raw_usage.get("providerAllowanceUsd"),
        }
    provider_receipt_path = spec.root / "artifacts" / "provider-request-admissions.jsonl"
    receipt_allowance = _provider_allowance_from_receipt(provider_receipt_path)
    if receipt_allowance is not None and (
            not isinstance(usage, dict) or usage.get("provider_allowance_usd") is None):
        usage = dict(usage or {})
        usage["provider_allowance_usd"] = receipt_allowance
    provider_request_count = _provider_request_admissions(provider_receipt_path)
    provider_request_count_source = "codegg_provider_boundary_receipt"
    if provider_request_count is None:
        provider_request_count = _provider_request_admissions(client.stderr_path)
        provider_request_count_source = (
            "codegg_provider_boundary_log" if provider_request_count is not None
            else "unavailable_conservative_trial_reservation"
        )
    guard.observe(provider_requests=(provider_request_count if provider_request_count is not None
                                    else 1),
                  input_tokens=int((usage or {}).get("input_tokens") or 0),
                  output_tokens=int((usage or {}).get("output_tokens") or 0),
                  cost_usd=(usage or {}).get("provider_allowance_usd"),
                  cost_source=("opencode_go_allowance_receipt"
                               if receipt_allowance is not None
                               else "opencode_go_allowance"
                               if (usage or {}).get("provider_allowance_usd") is not None
                               else None))

    exit_code = verification.get("exit_code")
    verifier_confirmed = exit_code == 0
    # A protected control is never a solved task. Its task is impossible by
    # construction, so "solved" would only ever mean "the harness credited an
    # impossible task" — and the control must never count toward a score.
    activity_times = [
        call.get("finished_elapsed_ms") for call in tool_calls
        if isinstance(call.get("finished_elapsed_ms"), int)
    ]
    first_activity_elapsed_ms = min(activity_times) if activity_times else None
    last_activity_elapsed_ms = max(activity_times) if activity_times else None
    active_work_span_ms = _active_work_span_ms(tool_calls)
    maximum_activity_gap_ms = _maximum_activity_gap_ms(duration_ms, tool_calls)
    duration_qualified = _duration_qualified(case.case_id, duration_ms, tool_calls)
    solved = bool(verifier_confirmed and terminal == "completed" and not case.protected
                  and (case.case_id == H007_CASE_ID or duration_qualified))

    verifier_tests_observed = diagnostics.verifier_tests_observed(
        verification.get("diagnostic")
    )
    h007_projection = None
    if case.case_id == H007_CASE_ID:
        repair_cycles = len(observed_repair_cycles)
        continuation_confirmed = _h007_continuation_confirmed(
            tool_calls, agent_root / "fixture", case_prompt=case.prompt)
        h007_projection = h007_results.project_result(
            terminal=terminal,
            verifier_ran=verifier_tests_observed,
            verifier_exit_code=verification.get("exit_code"),
            verifier_state=(verification.get("diagnostic", {}).get("state")
                            if isinstance(verification.get("diagnostic"), dict) else None),
            duration_ms=duration_ms,
            tool_calls=tool_calls,
            measurement_complete=(observation_receipt.get("event_gap_count") == 0
                                 and all(call.get("status") != "unknown"
                                         for call in tool_calls)),
            continuation_confirmed=continuation_confirmed,
            repair_cycles=repair_cycles,
            protected=case.protected,
            tool_call_limit=case.budget.get("max_tool_calls"),
        )
        solved = h007_projection["task_outcome"] == "passed"

    receipt: dict[str, Any] = {
        "schema_version": (3 if case.case_id == H006_CASE_ID else
                           7 if case.case_id == H007_CASE_ID else 1),
        "case_id": case.case_id,
        "family": case.family,
        "group_id": case.group_id,
        "protected": case.protected,
        "model": model,
        "agent": agent,
        "profile": profile,
        "placement_advisor_enabled": not disable_placement_advisor,
        "binary_sha256": spec.binary_sha256,
        "source_revision": spec.revision,
        "requested_manifest_sha256": requested_manifest_sha256,
        "session_id": session_id,
        "terminal": terminal,
        "agent_terminal": agent_terminal,
        "stop_reason": stop_reason,
        "cancellation_source": cancellation_source,
        "duration_ms": duration_ms,
        "duration_qualified": duration_qualified,
        "first_activity_elapsed_ms": first_activity_elapsed_ms,
        "last_activity_elapsed_ms": last_activity_elapsed_ms,
        "active_work_span_ms": active_work_span_ms,
        "minimum_active_duration_ms": (H006_MIN_ACTIVE_DURATION_MS
                                        if case.case_id == H006_CASE_ID else None),
        "maximum_activity_gap_ms": (maximum_activity_gap_ms
                                    if case.case_id == H006_CASE_ID else None),
        "disk_floor": None if disk_watcher is None else {
            "minimum_free_bytes": min_free_disk_bytes,
            "last_free_bytes": disk_watcher.free_bytes,
            "lowest_observed_free_bytes": disk_watcher.minimum_free_bytes,
            "triggered": disk_watcher.triggered,
            "measurement_error": disk_watcher.error,
        },
        "resource_usage": {
            **(process_resources or {
                "cpu_seconds": None, "peak_rss_bytes": None, "sample_count": 0,
            }),
            "lane_disk_growth_bytes": max(
                0, _directory_size_bytes(lane_root) - disk_before_bytes),
        },
        "tool_call_count": len(tool_calls),
        "tool_calls": tool_calls,
        "provider_usage": usage,
        "provider_request_count": provider_request_count,
        "provider_request_count_source": provider_request_count_source,
        "provider_attempt_diagnostic": provider_attempt_diagnostic,
        "usage_unavailable_reason": None if usage else "not_reported_by_the_route",
        "verification": verification,
        "verifier_tests_observed": verifier_tests_observed,
        "verifier_confirmed": verifier_confirmed,
        "solved": solved,
        "repair_trace_diagnostic": ({
            "observed_repair_cycle_count": len(observed_repair_cycles),
            "persisted_correlated_repair_events": trace_receipt[
                "correlated_repair_cycles"],
            "event_count_matches": (
                len(observed_repair_cycles)
                == trace_receipt["correlated_repair_cycles"]
            ),
            "trace_complete": trace_receipt["state"] == "complete",
            "raw_job_ids_retained": False,
        } if h007_projection is not None else None),
        "repeated_identical_failure": _repeated_identical_failure(tool_calls),
        "fault": fault_receipt,
        "event_gap": observation_receipt,
        "causal_trace": trace_receipt,
        "attempt_id": f"{case.case_id}:0",
        "measurement_failures": observation_receipt["event_gap_count"],
        "measurement_retry_state": (
            "unresolved" if observation_receipt["event_gap_count"] else "not_needed"
        ),
        "control_exercised": (
            _declared_control_trigger_exercised(case, tool_calls)
            or _control_exercised({"fault": fault_receipt,
                                   "repeated_identical_failure":
                                   _repeated_identical_failure(tool_calls)})),
        "daemon_stop_error": daemon_stop_error,
        "daemon_process_count_after_cleanup": daemon_process_count_after_cleanup,
        "policy_receipt": policy_receipt if isinstance(policy_receipt, dict) else None,
        "budget": guard.as_dict(),
        "lane_root": str(lane_root),
        "transcript_path": str(transcript_path),
    }
    if case.case_id == "sustained-event-ledger":
        metadata_path = repository / "dev/harness_lab/fixtures/sustained_event_ledger.case.json"
        receipt["fixture_metadata_sha256"] = hashlib.sha256(metadata_path.read_bytes()).hexdigest()
        receipt["continuation_evidence"] = {
            "job_observations": [call["job"] for call in tool_calls if call.get("job")],
            "timed_tool_calls": sum(1 for call in tool_calls
                                     if isinstance(call.get("started_elapsed_ms"), int)
                                     and isinstance(call.get("finished_elapsed_ms"), int)),
        }
    if h007_projection is not None:
        receipt.update(h007_projection)
        receipt["event_measurement_complete"] = h007_projection[
            "horizon_evidence"]["event_measurement_complete"]
        continuation_text = _h007_read_continuation(agent_root / "fixture")
        gate_checklist_required = _h007_case_requires_gate_block(case.prompt)
        continuation_diagnostic = h007_results.summarize_continuation_document(
            continuation_text,
            tool_calls,
            required_phases=_h007_required_phases(case.prompt),
            required_contract_phases=_h007_required_contract_phases(case.prompt),
            required_stages=_h007_required_stages(case.prompt),
            required_gates=(_h007_required_gates(case.prompt)
                            if gate_checklist_required else None),
            required_job_label="overlap" if gate_checklist_required else None,
            required_job_command=(H007_S018_RELEASE_OVERLAP_COMMAND
                                  if gate_checklist_required else None),
            required_success_job_labels=(
                H007_S018_REQUIRED_SUCCESS_JOB_LABELS if gate_checklist_required else None
            ),
            required_repair_job_pairs=(
                H007_S018_REQUIRED_REPAIR_JOB_PAIRS
                if _h007_case_requires_repair_job_pairs(case.prompt) else None
            ),
            require_all_test_jobs_terminal=gate_checklist_required,
        )
        continuation_diagnostic["release_overlap_confirmed"] = (
            _h007_s018_overlap_confirmed(continuation_text, tool_calls)
            if gate_checklist_required else None
        )
        continuation_diagnostic["required_review_observed"] = (
            _h007_required_review_observed(continuation_text, tool_calls)
            if gate_checklist_required else None
        )
        continuation_diagnostic["all_public_artifacts_present"] = (
            _h007_required_public_artifacts_present(agent_root / "fixture")
            if gate_checklist_required else None
        )
        continuation_diagnostic["public_artifact_status"] = (
            _h007_public_artifact_status(agent_root / "fixture")
            if gate_checklist_required else None
        )
        continuation_diagnostic["invalid_public_artifacts"] = (
            [name for name, valid in
             _h007_public_artifact_status(agent_root / "fixture").items() if not valid]
            if gate_checklist_required else None
        )
        receipt["continuation_diagnostic"] = continuation_diagnostic
        job_sequence_diagnostic = diagnostics.summarize_job_tool_sequence(tool_calls)
        receipt["task_completion_diagnostic"] = h007_results.summarize_task_completion(
            terminal=terminal,
            task_outcome=h007_projection["task_outcome"],
            verifier_outcome=h007_projection["verifier_outcome"],
            continuation=continuation_diagnostic,
            verifier_diagnostic=(
                verification.get("diagnostic")
                if isinstance(verification.get("diagnostic"), Mapping) else None
            ),
            horizon_evidence=h007_projection.get("horizon_evidence"),
            job_sequence_diagnostic=job_sequence_diagnostic,
            serialized_transcript_diagnostic=serialized_transcript_diagnostic,
        )
        receipt["serialized_transcript_diagnostic"] = serialized_transcript_diagnostic
        reviewer_diagnostic = diagnostics.summarize_reviewer_lifecycle(tool_calls)
        receipt["reviewer_diagnostic"] = reviewer_diagnostic
        receipt["job_tool_sequence_diagnostic"] = job_sequence_diagnostic
        receipt["ordered_tool_activity_diagnostic"] = (
            diagnostics.summarize_ordered_tool_activity(tool_calls)
        )
        receipt["open_tool_calls_at_boundary_diagnostic"] = (
            diagnostics.summarize_open_tool_calls_at_boundary(
                tool_calls, boundary_elapsed_ms=duration_ms, terminal=terminal,
            )
        )
        receipt["reviewer_durable_state_diagnostic"] = (
            diagnostics.summarize_reviewer_durable_state(
                lane_root / "data" / "codegg" / "codegg.db",
                reviewer_task_id_sha256=reviewer_diagnostic.get("reviewer_task_id_sha256"),
                reviewer_run_id_sha256=reviewer_diagnostic.get("reviewer_run_id_sha256"),
            )
        )
        delayed_final_review = _h007_review_uses_30_minute_threshold(case.prompt)
        receipt["reviewer_schedule_diagnostic"] = {
            "start_threshold_active_ms": (
                H007_S018_FINAL_REVIEW_START_ACTIVE_MS if delayed_final_review
                else H007_S018_REVIEW_START_ACTIVE_MS
            ),
            "due_prompt_marker": (
                H007_S018_FINAL_REVIEW_START_MARKER if delayed_final_review
                else H007_S018_REVIEW_START_MARKER
            ),
            "due_prompt_count": reviewer_due_prompt_count,
            "first_due_followup_number": first_reviewer_due_followup_number,
            "first_due_active_work_span_ms": first_reviewer_due_active_work_span_ms,
            "reviewer_spawn_count": reviewer_diagnostic["reviewer_spawn_count"],
            "task_tool_requested_by_case": "task" in case.allowed_tools,
            "task_tool_call_count": sum(call.get("tool") == "task" for call in tool_calls),
        }
        receipt["autonomous_followup_count"] = autonomous_followup_count
        receipt["continuation_stop_reason"] = continuation_stop_reason
        receipt["turn_timeout_diagnostic"] = _h007_prompt_timeout_diagnostic(
            timed_out=turn_timeout_observed,
            prompt_request_count=prompt_request_count,
            followup_count=autonomous_followup_count,
            last_request_timeout_seconds=last_prompt_request_timeout_seconds,
            last_request_elapsed_ms=last_prompt_request_elapsed_ms,
            remaining_wallclock_seconds_at_prompt_start=(
                last_prompt_remaining_wallclock_seconds),
            remaining_wallclock_seconds=turn_timeout_remaining_wallclock_seconds,
        )
        if case.case_id == H007_CASE_ID:
            receipt["prompt_boundary_timeline"] = {
                "schema": "h007_prompt_boundary_timeline_v1",
                "entries": [
                    _h007_prompt_boundary_diagnostic(**row)
                    for row in prompt_boundary_rows
                ],
                "count": len(prompt_boundary_rows),
                "prompt_text_retained": False,
                "provider_text_retained": False,
                "exception_text_retained": False,
                "raw_tool_arguments_retained": False,
            }
        receipt["activity_stall"] = {
            "stall_limit_seconds": stall_watcher.stall_seconds if stall_watcher else None,
            "triggered": stall_watcher.triggered if stall_watcher else False,
            "cancel_error": stall_watcher.cancel_error if stall_watcher else None,
        }
        receipt["cost_outcome"] = (
            "reported" if isinstance((usage or {}).get("provider_allowance_usd"), (int, float))
            else "unknown"
        )
        metadata_path = repository / "dev/harness_lab/fixtures/build_provenance_catalog.case.json"
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
        receipt["fixture_metadata_sha256"] = hashlib.sha256(metadata_path.read_bytes()).hexdigest()
        receipt["manifest_sha256"] = metadata["manifest_sha256"]
        receipt["hidden_verifier_sha256"] = metadata["hidden_verifier_sha256"]
    if failure is not None:
        receipt["terminal_failure"] = failure
    else:
        receipt["terminal_failure"] = None
    return receipt


def run_suite(binary: Path, suite: AutonomySuite, model: str, *,
              budget_policy: budget.BudgetPolicy, lane_root: Path,
              repository: Optional[Path] = None, profile: str = "workspace-write",
              timeout: float = 900.0, agent: str = "build",
              credential: Optional[str] = None,
              case_ids: Optional[list[str]] = None,
              min_free_disk_bytes: Optional[int] = None) -> dict[str, Any]:
    """Run every supported case of a suite in its own private lane.

    Each case gets its own lane so a case that leaves a daemon behind, a
    workspace, or a mutated fixture cannot influence the next case.

    The suite shares **one** spend guard, so the declared request cap covers
    the whole suite instead of restarting at every case. A case that the cap
    refuses is recorded as refused rather than silently skipped, and the
    cases after it are refused too.
    """
    if not case_ids:
        case_ids = [case.case_id for case in suite.cases if case.support == "supported"]
    admission = budget.admit_budget(budget_policy)
    if admission.status != budget.STATUS_ADMITTED:
        raise HarnessError("budget admission refused before the autonomy suite started: "
                           + json.dumps(admission.as_dict(), sort_keys=True))
    guard = budget.SpendGuard(budget_policy)
    runs: list[dict[str, Any]] = []
    for index, case_id in enumerate(case_ids):
        case = suite.case(case_id)
        if case.support != "supported":
            raise HarnessError(f"case {case_id} is deferred, not runnable")
        # The lane directory is named by position, not by case id: the daemon
        # binds its control socket inside the lane, and the platform's
        # sun_path limit is short enough that a 31-character case id can push
        # the socket path past it. The case id stays in the receipt.
        case_lane = lane_root / f"case{index:02d}"
        try:
            receipt = run_case(binary, case, model, budget_policy=budget_policy,
                               lane_root=case_lane, repository=repository,
                               profile=profile, timeout=timeout, agent=agent,
                               credential=credential, guard=guard,
                               min_free_disk_bytes=min_free_disk_bytes,
                               requested_manifest_sha256=suite.suite_sha256)
        except (HarnessError, budget.BudgetExhausted) as error:
            receipt = {"case_id": case_id, "terminal": "refused", "ok": False,
                       "error": str(error), "solved": False, "protected": case.protected}
        runs.append(receipt)
    h007_runs = [run for run in runs if run.get("case_id") == H007_CASE_ID
                 and "task_outcome" in run]
    result = {"schema_version": 1, "model": model,
              "binary_sha256": runs[0].get("binary_sha256") if runs else None,
              "case_ids": list(case_ids), "runs": runs,
              "budget": guard.as_dict(),
              "solved_cases": sum(1 for run in runs if run.get("solved"))}
    if h007_runs:
        result["h007_outcomes"] = h007_results.summarize_h007(h007_runs)
    return result


def run_paired_comparison(baseline_binary: Path, candidate_binary: Path,
                          suite: AutonomySuite, model: str, *,
                          budget_policy: budget.BudgetPolicy, lane_root: Path,
                          repetitions: int = 3, seed: int = 20261028,
                          repository: Optional[Path] = None,
                          profile: str = "workspace-write", timeout: float = 900.0,
                          case_ids: Optional[list[str]] = None) -> dict[str, Any]:
    """Run each baseline/advisor case pair in randomized order under one budget.

    One SpendGuard owns the whole comparison, including both binary arms.
    Provider request and allowance limits are forwarded into each fresh daemon
    and enforced at the OpenCode Go request boundary.
    """
    if not isinstance(repetitions, int) or isinstance(repetitions, bool) or repetitions < 1:
        raise HarnessError("comparison repetitions must be a positive integer")
    admission = budget.admit_budget(budget_policy)
    if admission.status != budget.STATUS_ADMITTED:
        raise HarnessError("budget admission refused before the comparison started: "
                           + json.dumps(admission.as_dict(), sort_keys=True))
    selected = case_ids or [case.case_id for case in suite.cases
                            if case.support == "supported"]
    if not selected:
        raise HarnessError("comparison requires at least one supported case")
    for case_id in selected:
        if suite.case(case_id).support != "supported":
            raise HarnessError(f"case {case_id} is deferred, not runnable")

    guard = budget.SpendGuard(budget_policy)
    sides: dict[str, list[dict[str, Any]]] = {"baseline": [], "candidate": []}
    schedule: list[dict[str, Any]] = []
    exhausted = False
    schedule_rows = paired_schedule(selected, repetitions, seed)
    lane_root.mkdir(parents=True, exist_ok=True)
    checkpoint_path = lane_root / "comparison-progress.jsonl"
    if checkpoint_path.exists():
        raise HarnessError(f"comparison checkpoint already exists: {checkpoint_path}")
    for ordinal, row in enumerate(schedule_rows):
        repetition = row["repetition"]
        case_id = row["case_id"]
        arm = row["arm"]
        case = suite.case(case_id)
        binary = baseline_binary if arm == "baseline" else candidate_binary
        trial_lane = lane_root / f"trial{ordinal:02d}"
        if exhausted:
            result = {"case_id": case_id, "repetition": repetition,
                      "arm": arm, "terminal": "refused", "solved": False,
                      "error": guard.exhausted_cap or "comparison_budget_exhausted"}
        else:
            try:
                result = run_case(
                    binary, case, model, budget_policy=budget_policy,
                    lane_root=trial_lane, repository=repository,
                    profile=profile, timeout=timeout, guard=guard,
                    requested_manifest_sha256=suite.suite_sha256,
                    disable_placement_advisor=(
                        case.case_id.startswith("a005-") and arm == "baseline"))
            except (HarnessError, budget.BudgetExhausted) as error:
                result = {"case_id": case_id, "terminal": "refused",
                          "solved": False, "error": str(error),
                          "protected": case.protected}
                exhausted = guard.exhausted_cap is not None
            result["repetition"] = repetition
            result["arm"] = arm
        row["terminal"] = result.get("terminal")
        row["provider_request_count"] = result.get("provider_request_count")
        schedule.append(row)
        sides[arm].append(result)
        # Preserve a safe receipt after every trial. If an error occurs while
        # building the final aggregate, the already completed and refused
        # trials remain auditable without replaying provider work.
        with checkpoint_path.open("a", encoding="utf-8") as checkpoint:
            checkpoint.write(json.dumps({"schedule": row, "result": result},
                                        sort_keys=True) + "\n")
            checkpoint.flush()
            os.fsync(checkpoint.fileno())
    comparison = compare_runs(scoring.default_contract(), sides["baseline"],
                              sides["candidate"])
    h007_outcomes = {
        arm: h007_results.summarize_h007(
            [run for run in sides[arm] if run.get("case_id") == H007_CASE_ID
             and "task_outcome" in run])
        for arm in ("baseline", "candidate")
    }
    result = {"schema_version": 1, "model": model, "seed": seed,
            "repetitions": repetitions, "case_ids": list(selected),
            "baseline_binary_sha256": sha256_file(baseline_binary),
            "candidate_binary_sha256": sha256_file(candidate_binary),
            "schedule": schedule,
            "baseline": {"runs": sides["baseline"]},
            "candidate": {"runs": sides["candidate"]},
            "comparison": comparison, "budget": guard.as_dict()}
    if any(run.get("case_id") == H007_CASE_ID for arm in sides.values() for run in arm):
        result["h007_outcomes"] = h007_outcomes
    return result


def resume_paired_comparison(checkpoint_path: Path, baseline_binary: Path,
                             candidate_binary: Path, suite: AutonomySuite, model: str, *,
                             budget_policy: budget.BudgetPolicy, lane_root: Path,
                             repetitions: int, seed: int,
                             case_ids: Optional[list[str]] = None,
                             prior_provider_requests: int = 0,
                             prior_allowance_usd: float = 0.0,
                             prior_elapsed_seconds: float = 0.0,
                             profile: str = "workspace-write",
                             timeout: float = 900.0) -> dict[str, Any]:
    """Complete refused rows from a paired checkpoint under a new residual cap."""
    admission = budget.admit_budget(budget_policy)
    if admission.status != budget.STATUS_ADMITTED:
        raise HarnessError("budget admission refused before the resumed comparison started: "
                           + json.dumps(admission.as_dict(), sort_keys=True))
    if min(prior_provider_requests, prior_elapsed_seconds) < 0 or prior_allowance_usd < 0:
        raise HarnessError("prior comparison budget counters must be non-negative")
    try:
        checkpoint_rows = [json.loads(line) for line in
                           checkpoint_path.read_text(encoding="utf-8").splitlines()
                           if line.strip()]
    except (OSError, json.JSONDecodeError, UnicodeError) as error:
        raise HarnessError("comparison checkpoint is unreadable") from error
    selected = case_ids or [case.case_id for case in suite.cases
                            if case.support == "supported"]
    expected = paired_schedule(selected, repetitions, seed)
    if len(checkpoint_rows) != len(expected):
        raise HarnessError("comparison checkpoint row count does not match its frozen schedule")
    for expected_row, checkpoint in zip(expected, checkpoint_rows, strict=True):
        schedule = checkpoint.get("schedule") if isinstance(checkpoint, dict) else None
        if not isinstance(schedule, dict) or any(
                schedule.get(key) != expected_row.get(key)
                for key in ("ordinal", "case_id", "repetition", "arm")):
            raise HarnessError("comparison checkpoint does not match the frozen paired schedule")
    baseline_hash = sha256_file(baseline_binary)
    candidate_hash = sha256_file(candidate_binary)
    if baseline_hash != candidate_hash:
        raise HarnessError("resumed A005 comparison requires identical binary hashes")
    for checkpoint in checkpoint_rows:
        result = checkpoint.get("result", {})
        recorded_hash = result.get("binary_sha256")
        if recorded_hash is not None and recorded_hash != baseline_hash:
            raise HarnessError("resumed comparison binary differs from its checkpoint")

    guard = budget.SpendGuard(budget_policy)
    lane_root.mkdir(parents=True, exist_ok=True)
    progress_path = lane_root / "resume-progress.jsonl"
    if progress_path.exists():
        raise HarnessError(f"resumption checkpoint already exists: {progress_path}")
    resumed = 0
    for checkpoint in checkpoint_rows:
        row = checkpoint["schedule"]
        previous = checkpoint["result"]
        if previous.get("terminal") != "refused" or previous.get("provider_request_count") is not None:
            continue
        case = suite.case(row["case_id"])
        try:
            result = run_case(
                baseline_binary if row["arm"] == "baseline" else candidate_binary,
                case, model, budget_policy=budget_policy, lane_root=lane_root /
                f"trial{row['ordinal']:02d}", profile=profile, timeout=timeout, guard=guard,
                requested_manifest_sha256=suite.suite_sha256,
                disable_placement_advisor=(case.case_id.startswith("a005-")
                                           and row["arm"] == "baseline"))
        except (HarnessError, budget.BudgetExhausted) as error:
            result = {"case_id": case.case_id, "terminal": "refused", "solved": False,
                      "error": str(error), "protected": case.protected}
        result["repetition"] = row["repetition"]
        result["arm"] = row["arm"]
        row["terminal"] = result.get("terminal")
        row["provider_request_count"] = result.get("provider_request_count")
        checkpoint["result"] = result
        with progress_path.open("a", encoding="utf-8") as progress:
            progress.write(json.dumps(checkpoint, sort_keys=True) + "\n")
            progress.flush()
            os.fsync(progress.fileno())
        resumed += 1

    sides = {arm: [checkpoint["result"] for checkpoint in checkpoint_rows
                   if checkpoint["schedule"]["arm"] == arm]
             for arm in ("baseline", "candidate")}
    final_schedule = []
    for checkpoint in checkpoint_rows:
        row = dict(checkpoint["schedule"])
        row["terminal"] = checkpoint["result"].get("terminal")
        row["provider_request_count"] = checkpoint["result"].get("provider_request_count")
        final_schedule.append(row)
    comparison = compare_runs(scoring.default_contract(), sides["baseline"],
                              sides["candidate"])
    cumulative_allowance = prior_allowance_usd + guard.cost_usd
    h007_outcomes = {
        arm: h007_results.summarize_h007(
            [run for run in sides[arm] if run.get("case_id") == H007_CASE_ID
             and "task_outcome" in run])
        for arm in ("baseline", "candidate")
    }
    result = {"schema_version": 1, "model": model, "seed": seed,
            "repetitions": repetitions, "case_ids": selected,
            "baseline_binary_sha256": baseline_hash,
            "candidate_binary_sha256": candidate_hash,
            "schedule": final_schedule,
            "baseline": {"runs": sides["baseline"]},
            "candidate": {"runs": sides["candidate"]},
            "comparison": comparison,
            "budget": guard.as_dict(),
            "resumption": {"resumed_trials": resumed,
                           "source_checkpoint": str(checkpoint_path),
                           "prior_provider_requests": prior_provider_requests,
                           "prior_allowance_usd": prior_allowance_usd,
                           "prior_elapsed_seconds": prior_elapsed_seconds,
                           "cumulative_provider_requests":
                               prior_provider_requests + guard.provider_requests,
                           "cumulative_allowance_usd": cumulative_allowance,
                           "cumulative_elapsed_seconds":
                               prior_elapsed_seconds + guard.as_dict()["elapsed_seconds"]}}
    if any(run.get("case_id") == H007_CASE_ID for arm in sides.values() for run in arm):
        result["h007_outcomes"] = h007_outcomes
    return result


def paired_schedule(case_ids: list[str], repetitions: int, seed: int) -> list[dict[str, Any]]:
    """Derive a deterministic randomized schedule with one pair per case/repeat."""
    rng = random.Random(seed)
    schedule: list[dict[str, Any]] = []
    for repetition in range(1, repetitions + 1):
        cases = list(case_ids)
        rng.shuffle(cases)
        for case_id in cases:
            arms = ["baseline", "candidate"]
            rng.shuffle(arms)
            for arm in arms:
                schedule.append({"ordinal": len(schedule), "case_id": case_id,
                                 "repetition": repetition, "arm": arm})
    return schedule


def _observation(contract: scoring.ScoreContract, criterion_id: str,
                 value: Optional[float]) -> scoring.Observation:
    """Return one observation, keeping an unmeasured metric distinct from zero."""
    if value is None:
        return scoring.Observation(criterion_id=criterion_id, value=None, measured=False)
    return scoring.Observation(criterion_id=criterion_id, value=float(value))


def _pass_rate(runs: list[dict[str, Any]], protected: bool) -> Optional[float]:
    """Return the solved fraction over the task cases on one side."""
    selected = [run for run in runs if bool(run.get("protected")) is protected]
    if not selected:
        return None
    return sum(1 for run in selected if run.get("solved")) / len(selected)


def _repeated_identical_failure(tool_calls: list[dict[str, Any]]) -> bool:
    """Return whether an identical call really failed more than once.

    A protected control only means something when the situation it is about
    actually happened: the same call, with the same arguments, failing again.
    This is an **observation** of the transcript, not a judgement of it — the
    case's own verifier decides what to make of it.
    """
    attempts: dict[tuple[Any, Any], int] = {}
    failures: dict[tuple[Any, Any], int] = {}
    for call in tool_calls:
        key = (call.get("tool"), call.get("arguments_sha256"))
        attempts[key] = attempts.get(key, 0) + 1
        if call.get("status") == "failed":
            failures[key] = failures.get(key, 0) + 1
    return any(attempts[key] >= 2 and failures.get(key, 0) >= 2 for key in attempts)


def _control_exercised(run: dict[str, Any]) -> bool:
    """Return whether the harness actually put the trial in the control's situation.

    A protected control only means something when the situation it is about
    really happened. A faulted control was exercised when the faulted call
    really failed again; a control with no fault was exercised when the same
    call really failed more than once. A control that was never exercised is
    reported as such and is **not** counted as a control that failed to hold.
    """
    if run.get("control_exercised") is True:
        return True
    fault = run.get("fault")
    if isinstance(fault, Mapping):
        return bool(fault.get("exercised"))
    return bool(run.get("repeated_identical_failure"))


def _declared_control_trigger_exercised(case: AutonomyCase,
                                        tool_calls: list[dict[str, Any]]) -> bool:
    """Check an explicitly declared, case-specific negative-control trigger."""
    if case.control_trigger != "unavailable_managed_checkout":
        return False
    return any(
        call.get("tool") == "task"
        and isinstance(call.get("task_placement"), Mapping)
        and call["task_placement"].get("requested") == "existing_managed_read_only"
        and call["task_placement"].get("worktree_id_present") is True
        for call in tool_calls
    )


def _control_rate(runs: list[dict[str, Any]]) -> Optional[float]:
    """Return the fraction of exercised controls that still held.

    A protected negative control's verifier is the case's own statement of
    what "as expected" means, so the control holds when that verifier
    **confirms** the trial. A control that was never exercised — the agent
    never put the harness in the situation the control is about — is excluded
    rather than counted as a control that failed to hold, and when no control
    was exercised the metric is **unmeasured**, not zero: "we did not see it
    happen" is not "it went wrong".

    Counting the *absence* of a solved task instead would be vacuous here: a
    control's task is impossible by construction, so it is never solved and
    the rate would always be 1.0.
    """
    controls = [run for run in runs if bool(run.get("protected"))]
    exercised = [run for run in controls if _control_exercised(run)]
    if not exercised:
        return None
    return sum(1 for run in exercised if run.get("verifier_confirmed")) / len(exercised)


def _control_exercise_counts(runs: list[dict[str, Any]]) -> dict[str, Any]:
    """Return how many controls ran and how many of them were exercised."""
    controls = [run for run in runs if bool(run.get("protected"))]
    exercised = [run for run in controls if _control_exercised(run)]
    return {"controls": len(controls),
            "exercised": len(exercised),
            "held": sum(1 for run in exercised if run.get("verifier_confirmed")),
            "never_exercised": [run.get("case_id") for run in controls
                                if not _control_exercised(run)]}


def _success_rate(runs: list[dict[str, Any]]) -> Optional[float]:
    """Return the fraction of trials that completed without an avoidable failure.

    A trial counts as successful when its turn reached a terminal on its own
    terms — it was not stopped as a stall, it did not time out, it was not
    cancelled, it was not refused, and it did not end in an infrastructure or
    provider failure.

    This metric is about the **run**, not the **outcome**. Whether the agent
    solved the task is `correctness.pass_rate`'s business; folding it in here
    would count one fact twice and make a reliability move depend on the
    task, which is how a metric starts to mean two things at once. An
    individual failed tool call is normal in an agent loop and does not make
    a trial unreliable; a trial that ended in an avoidable terminal failure
    does.
    """
    if not runs:
        return None
    # An infrastructure error is an **unmeasured** trial: the harness, the
    # daemon or the transport failed before the trial could produce an
    # observation, so it says nothing about the candidate. It is excluded from
    # the denominator rather than counted as a failure — and a retried trial
    # does not add a second row to it.
    measured = [run for run in runs if run.get("terminal") != "infrastructure_error"]
    if not measured:
        return None
    good = 0
    for run in measured:
        if run.get("terminal") != "completed":
            continue
        if run.get("terminal_failure"):
            continue
        good += 1
    return good / len(measured)


def _median_duration_ms(runs: list[dict[str, Any]]) -> Optional[float]:
    """Return the median measured duration, or None when nothing was measured."""
    durations = sorted(float(run["duration_ms"]) for run in runs
                       if isinstance(run.get("duration_ms"), (int, float)))
    if not durations:
        return None
    middle = len(durations) // 2
    if len(durations) % 2:
        return durations[middle]
    return (durations[middle - 1] + durations[middle]) / 2.0


def compare_runs(contract: scoring.ScoreContract,
                 baseline: list[dict[str, Any]],
                 candidate: list[dict[str, Any]]) -> dict[str, Any]:
    """Score a candidate against a baseline with the shared score contract.

    The protected negative controls are scored separately from the task
    cases: a control's expected outcome is that the task is **not** solved, so
    a control that starts being credited as solved is a regression, not an
    improvement.
    """
    def observations(runs: list[dict[str, Any]], side: str) -> list[scoring.Observation]:
        return [
            _observation(contract, "correctness.pass_rate", _pass_rate(runs, protected=False)),
            _observation(contract, "reliability.success_rate", _success_rate(runs)),
            _observation(contract, "efficiency.duration_ms", _median_duration_ms(runs)),
            _observation(contract, "control.no_regression", _control_rate(runs)),
        ]

    paired = sorted({run.get("case_id") for run in baseline}
                    & {run.get("case_id") for run in candidate})
    baseline_observations = observations(baseline, "baseline")
    candidate_observations = observations(candidate, "candidate")
    outcome = scoring.score_candidate(contract, baseline_observations,
                                      candidate_observations,
                                      evidence_pairs=len(paired))
    return {"outcome": outcome.as_dict(),
            "baseline_cases": len(baseline),
            "candidate_cases": len(candidate),
            "paired_case_ids": paired,
            # How many protected controls ran and how many of them were
            # actually exercised. A control that never ran is named here
            # rather than hidden inside an average.
            "controls": {"baseline": _control_exercise_counts(baseline),
                         "candidate": _control_exercise_counts(candidate)},
            # The measured values, so a reader can see what moved and what
            # did not. A verdict without its numbers is an assertion.
            "observations": {
                "baseline": [{"criterion_id": observation.criterion_id,
                              "value": observation.value,
                              "measured": observation.measured}
                             for observation in baseline_observations],
                "candidate": [{"criterion_id": observation.criterion_id,
                               "value": observation.value,
                               "measured": observation.measured}
                              for observation in candidate_observations],
            }}


def rescore_recorded_runs(baseline_runs: list[dict[str, Any]],
                           candidate_runs: list[dict[str, Any]]) -> dict[str, Any]:
    """Re-score two recorded sides with the current score contract.

    A score contract can be corrected after a run — the version-2 correction
    of this project is the precedent — and the honest way to re-decide a
    recorded pair is to re-derive the verdict from the **same recorded
    trials**, not to run the provider again. This function does exactly that:
    it takes the two sides' receipts and scores them with the contract as it
    stands now.

    The recorded receipts are the evidence. The decision is a derivation from
    them, so a derivation that changes when the contract changes is expected;
    a *trial* that changes is not, and this function never re-runs anything.

    A recorded receipt that predates an observation is not left without it:
    the observation is derived from the recorded transcript, which is the same
    evidence the run was judged on.
    """
    def with_observations(runs: list[dict[str, Any]]) -> list[dict[str, Any]]:
        observed = []
        for run in runs:
            if "repeated_identical_failure" not in run:
                run = dict(run)
                run["repeated_identical_failure"] = _repeated_identical_failure(
                    list(run.get("tool_calls") or []))
            observed.append(run)
        return observed

    contract = scoring.default_contract()
    comparison = compare_runs(contract, with_observations(baseline_runs),
                               with_observations(candidate_runs))
    comparison["contract_version"] = contract.version
    comparison["rescored_from_recorded_runs"] = True
    return comparison

"""Versioned append-only experiment receipts with independent replayable checks."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import tempfile
import time
import uuid
import sqlite3
from typing import Any


SCHEMA_VERSION = 2
#: Schema versions this reader accepts. A stored v1 pilot ledger stays readable
#: and reportable; every new row is written with `SCHEMA_VERSION`.
SUPPORTED_SCHEMA_VERSIONS = (1, 2)
TERMINALS = {"passed", "assertion_failed", "execution_failed", "cancelled", "denied", "timeout", "budget_exhausted", "infrastructure_error", "indeterminate", "interrupted_unknown"}
#: Terminal recorded for an attempt whose outcome is unknown because the
#: controller crashed mid-flight. It is never a pass and never a failure.
INTERRUPTED_UNKNOWN = "interrupted_unknown"
#: Named reasons the provider usage is absent. Mirrors `UsageAvailability` in
#: `crates/codegg-protocol/src/core.rs`. `None` means usage WAS measured.
USAGE_UNAVAILABLE_REASONS = ("reported", "not_observed", "not_provided", "lost_at_projection", "unspecified")
#: Reason reported for a stored v1 row written before the named vocabulary.
LEGACY_USAGE_UNAVAILABLE = "not_reported"
#: Provider token counts. A missing count is never read as zero.
USAGE_TOKEN_FIELDS = ("input_tokens", "output_tokens", "cached_tokens", "reasoning_tokens")
#: Every measured count a run manifest carries. A missing measurement is never
#: read as zero, and an attempt with an unknown outcome carries none of them.
_MEASUREMENT_FIELDS = ("tool_started", "tool_finished", "provider_calls", "wall_time_ms",
                        *USAGE_TOKEN_FIELDS, "cost_usd", "repeated_tool_calls", "recovery_followups")
INDEX_NAME = "index.sqlite3"


def canonical_digest(value: Any) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def _supported_version(value: object) -> bool:
    """True when `value` is a supported ledger schema version.

    `bool` is rejected explicitly: `True == 1` in Python, so a boolean would
    otherwise pass an equality check against a version number.
    """
    return isinstance(value, int) and not isinstance(value, bool) and value in SUPPORTED_SCHEMA_VERSIONS


def _measured_usage(usage: dict[str, Any]) -> bool:
    """True when at least one provider token count was actually observed."""
    return any(usage.get(field) is not None for field in USAGE_TOKEN_FIELDS)


def _derive_usage_unavailable(receipt: dict[str, Any], usage: dict[str, Any]) -> str | None:
    """Named reason the provider usage is absent, or `None` when it was measured.

    The reason is derived from the receipt, never hand-authored, and a missing
    measurement is never reported as zero usage.
    """
    if _measured_usage(usage):
        return None
    reason = receipt.get("usage_unavailable_reason")
    return reason if reason in USAGE_UNAVAILABLE_REASONS else "unspecified"


def _stored_usage_unavailable(manifest: dict[str, Any]) -> str | None:
    """Read the named usage reason back, defaulting a stored v1 row."""
    if "usage_unavailable" in manifest:
        return manifest["usage_unavailable"]
    metrics = manifest.get("metrics") or {}
    return None if _measured_usage(metrics) else LEGACY_USAGE_UNAVAILABLE


def _sum_measured(rows: list[dict[str, Any]], field: str) -> int | None:
    """Sum only measured values.

    A missing count is never summed as zero: the aggregate is `None` when no
    row measured the field.
    """
    values = [row[field] for row in rows if row.get(field) is not None]
    return sum(values) if values else None


def _atomic_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=".receipt-", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as output:
            json.dump(value, output, sort_keys=True, indent=2)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def _read_jsonl(path: Path, label: str) -> list[dict[str, Any]]:
    """Decode a JSONL log, tolerating exactly one torn final line.

    A crash during an append leaves the final line without its terminating
    newline. That single torn tail is tolerated so the ledger stays readable
    and appendable; corruption anywhere else stays a hard error.
    """
    text = path.read_text(encoding="utf-8")
    if not text:
        return []
    lines = text.split("\n")
    terminated = lines[-1] == ""
    if terminated:
        lines.pop()
    records: list[dict[str, Any]] = []
    for position, line in enumerate(lines):
        torn_tail = position == len(lines) - 1 and not terminated
        if not line.strip():
            if torn_tail:
                break
            raise ValueError(f"{label} has a blank line at position {position + 1}")
        try:
            record = json.loads(line)
        except json.JSONDecodeError as error:
            if torn_tail:
                break
            raise ValueError(f"{label} has a corrupt line at position {position + 1}") from error
        if not isinstance(record, dict):
            raise ValueError(f"{label} has a malformed record at position {position + 1}")
        records.append(record)
    return records


def _repair_index_tail(root: Path) -> None:
    """Make the shared index appendable again after a crash during an append.

    A torn final line never became a committed row, so it is dropped and the
    next append lands on a line boundary. A complete record that only lost its
    newline is preserved and terminated instead.
    """
    index = root / "index.jsonl"
    if not index.exists():
        return
    text = index.read_text(encoding="utf-8")
    if not text or text.endswith("\n"):
        return
    head, _, tail = text.rpartition("\n")
    keep = f"{head}\n" if head else ""
    try:
        record = json.loads(tail)
        committed = isinstance(record, dict) and isinstance(record.get("run_id"), str)
    except json.JSONDecodeError:
        committed = False
    if committed:
        with index.open("a", encoding="utf-8") as output:
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        return
    with index.open("r+", encoding="utf-8") as output:
        output.truncate(len(keep.encode("utf-8")))
        output.flush()
        os.fsync(output.fileno())


def _write_index_row(root: Path, row: dict[str, Any]) -> None:
    database = root / INDEX_NAME
    # ``sqlite3.Connection`` as a context manager commits but does not close;
    # an explicit close keeps the ledger from leaking a handle per recorded run.
    connection = sqlite3.connect(database)
    try:
        with connection:
            connection.execute("PRAGMA journal_mode=DELETE")
            connection.execute("""CREATE TABLE IF NOT EXISTS runs (
                run_id TEXT PRIMARY KEY, task_id TEXT NOT NULL, group_id TEXT NOT NULL,
                repeat INTEGER NOT NULL, candidate TEXT NOT NULL, manifest_sha256 TEXT NOT NULL)""")
            connection.execute("""CREATE TRIGGER IF NOT EXISTS runs_no_update
                BEFORE UPDATE ON runs BEGIN SELECT RAISE(ABORT, 'ledger index is append-only'); END""")
            connection.execute("""CREATE TRIGGER IF NOT EXISTS runs_no_delete
                BEFORE DELETE ON runs BEGIN SELECT RAISE(ABORT, 'ledger index is append-only'); END""")
            connection.execute("CREATE INDEX IF NOT EXISTS runs_by_task ON runs(task_id, repeat)")
            connection.execute("""INSERT INTO runs(run_id, task_id, group_id, repeat, candidate, manifest_sha256)
                VALUES (?, ?, ?, ?, ?, ?)""",
                               (row["run_id"], row["task_id"], row["group_id"], row["repeat"],
                                row["candidate"], row["manifest_sha256"]))
    finally:
        connection.close()


def _verify_sqlite_index(root: Path, rows: list[dict[str, Any]]) -> None:
    database = root / INDEX_NAME
    expected = [(row["run_id"], row["task_id"], row["group_id"], row["repeat"],
                 row["candidate"], row["manifest_sha256"]) for row in rows]
    if not database.exists():
        for row in rows:
            _write_index_row(root, row)
        return
    connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    try:
        with connection:
            actual = connection.execute("SELECT run_id, task_id, group_id, repeat, candidate, manifest_sha256 FROM runs ORDER BY rowid").fetchall()
    except sqlite3.Error as error:
        raise ValueError("SQLite ledger index is unreadable or has an unsupported schema") from error
    finally:
        connection.close()
    if actual != expected:
        raise ValueError("SQLite index disagrees with the append-only ledger manifest index")


def verify_file(workspace: Path, relative: str, expected_text: str | None = None, expected_sha256: str | None = None,
                expected_exists: bool = True) -> dict[str, Any]:
    root = workspace.resolve(strict=True)
    target = (root / relative).resolve(strict=False)
    if not target.is_relative_to(root):
        raise ValueError("verifier path escapes workspace")
    if not target.is_file():
        return {"status": "assertion_failed" if expected_exists else "passed", "path": relative, "observed_sha256": None}
    data = target.read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    passed = expected_exists and (expected_text is None or data.decode("utf-8") == expected_text) and (expected_sha256 is None or digest == expected_sha256)
    return {"status": "passed" if passed else "assertion_failed", "path": relative, "observed_sha256": digest}


def record(root: Path, receipt: dict[str, Any], *, task_id: str, group_id: str, candidate: str,
           workspace: Path, expected_file: str, expected_text: str | None = None,
           expected_sha256: str | None = None, expected_exists: bool = True,
           timeout_seconds: float = 120.0, repeat: int = 0,
           suite_sha256: str | None = None, case_sha256: str | None = None,
           trace: list[dict[str, Any]] | None = None,
           terminal_override: str | None = None, failure_class: str | None = None,
           expected_terminal: str = "passed") -> dict[str, Any]:
    if timeout_seconds <= 0:
        raise ValueError("timeout must be positive")
    if expected_terminal not in TERMINALS:
        raise ValueError("expected terminal must be a supported terminal classification")
    root = root.expanduser().resolve()
    root.mkdir(parents=True, exist_ok=True)
    receipt_snapshot = json.loads(json.dumps(receipt))
    run_id = str(uuid.uuid4())
    verification = verify_file(workspace, expected_file, expected_text, expected_sha256, expected_exists)
    terminal = terminal_override or verification["status"]
    if terminal not in TERMINALS:
        raise ValueError("unsupported terminal classification")
    usage = receipt_snapshot.get("codegg_usage")
    if not isinstance(usage, dict):
        usage = {}
    input_tokens = usage.get("input_tokens")
    output_tokens = usage.get("output_tokens")
    cached_tokens = usage.get("cached_tokens")
    reasoning_tokens = usage.get("reasoning_tokens")
    for value in (input_tokens, output_tokens, cached_tokens, reasoning_tokens):
        if value is not None and (not isinstance(value, int) or isinstance(value, bool) or value < 0):
            raise ValueError("provider usage fields must be non-negative integer counts")
    # A missing measurement is recorded with its named reason, never as zero.
    usage_unavailable = _derive_usage_unavailable(receipt_snapshot, usage)
    # The failure category is the redacted native category as projected. An
    # absent category means "not projected" and is never relabelled.
    failure_category = receipt_snapshot.get("failure_category")
    if failure_category is not None and not isinstance(failure_category, str):
        raise ValueError("failure category must be a string or null")
    tool_starts = [item for item in (trace or []) if item.get("kind") == "tool_started"]
    call_keys = [(item.get("tool"), item.get("arguments_sha256")) for item in tool_starts
                 if item.get("tool") and item.get("arguments_sha256")]
    seen_calls: set[tuple[str, str]] = set()
    repeated_calls = 0
    for key in call_keys:
        if key in seen_calls:
            repeated_calls += 1
        seen_calls.add(key)
    recovery_followups = 0
    failed_call = False
    for item in trace or []:
        if item.get("kind") == "tool_finished":
            failed_call = item.get("status") == "failed"
        elif item.get("kind") == "tool_started":
            if failed_call:
                recovery_followups += 1
            failed_call = False
    unavailable = ["provider_cost", "cpu", "rss", "model-visible context projection"]
    turn_id = usage.get("turn_id")
    if turn_id is None:
        unavailable.append("native turn id")
    if input_tokens is None:
        unavailable.append("input_tokens")
    if output_tokens is None:
        unavailable.append("output_tokens")
    if cached_tokens is None:
        unavailable.append("cached_tokens")
    if reasoning_tokens is None:
        unavailable.append("reasoning_tokens")
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "run_id": run_id,
        "task_id": task_id,
        "group_id": group_id,
        "repeat": repeat,
        "candidate": candidate,
        "suite_sha256": suite_sha256,
        "case_sha256": case_sha256,
        "created_unix_ms": int(time.time() * 1000),
        "subject": receipt_snapshot.get("daemon", {}),
        "policy": receipt_snapshot.get("policy"),
        "session_id": receipt_snapshot.get("session_id"),
        "turn_id": turn_id,
        "terminal": terminal,
        # The terminal this trial was *supposed* to reach. A paired comparison
        # reads it to decide which arm satisfied the case, so a protected
        # negative control is judged against its declared failure rather than
        # against the literal string "passed".
        "expected_terminal": expected_terminal,
        "failure_class": failure_class,
        "failure_category": failure_category,
        "usage_unavailable": usage_unavailable,
        "verifier": {"kind": "workspace_file", "path": expected_file, "expected_sha256": expected_sha256, "expected_exists": expected_exists,
                     "expected_text_sha256": hashlib.sha256(expected_text.encode()).hexdigest() if expected_text is not None else None,
                     **verification},
        "metrics": {"tool_started": receipt_snapshot.get("tool_started"), "tool_finished": receipt_snapshot.get("tool_finished"),
                    "provider_calls": receipt_snapshot.get("provider_calls"), "wall_time_ms": receipt_snapshot.get("wall_time_ms"),
                    "input_tokens": input_tokens, "output_tokens": output_tokens,
                    "cached_tokens": cached_tokens, "reasoning_tokens": reasoning_tokens,
                    "cost_usd": None, "repeated_tool_calls": repeated_calls,
                    "recovery_followups": recovery_followups, "unavailable": unavailable},
        "receipt_digest": canonical_digest(receipt_snapshot),
        "trace_count": len(trace or []),
        "receipt": receipt_snapshot,
    }
    run_dir = root / "runs" / run_id
    run_dir.mkdir(parents=True)
    source = (workspace.resolve() / expected_file).resolve(strict=False)
    artifact_path = None
    if source.is_file() and source.is_relative_to(workspace.resolve()):
        artifact_path = "artifacts/verified-output"
        artifact = run_dir / artifact_path
        artifact.parent.mkdir(parents=True, exist_ok=True)
        artifact.write_bytes(source.read_bytes())
        manifest["verifier"]["artifact_path"] = artifact_path
    _atomic_json(run_dir / "manifest.json", manifest)
    events = []
    for seq, item in enumerate(trace or [], start=1):
        if item.get("kind") not in {"tool_started", "tool_finished", "permission_request", "turn_terminal"}:
            raise ValueError("trace contains an unsupported event kind")
        events.append({"schema_version": SCHEMA_VERSION, "seq": seq, "run_id": run_id,
                       "kind": item["kind"], "tool": item.get("tool"), "status": item.get("status"),
                       "arguments_sha256": item.get("arguments_sha256"),
                       "elapsed_ms": item.get("elapsed_ms")})
    events.append({"schema_version": SCHEMA_VERSION, "seq": len(events) + 1, "run_id": run_id,
                   "kind": "terminal", "terminal": terminal,
                   "receipt_digest": manifest["receipt_digest"]})
    with (run_dir / "events.jsonl").open("x", encoding="utf-8") as output:
        for event in events:
            output.write(json.dumps(event, sort_keys=True, separators=(",", ":")) + "\n")
        output.flush()
        os.fsync(output.fileno())
    index_row: dict[str, Any]
    _repair_index_tail(root)
    with (root / "index.jsonl").open("a", encoding="utf-8") as output:
        manifest_digest = hashlib.sha256((run_dir / "manifest.json").read_bytes()).hexdigest()
        index_row = {"schema_version": SCHEMA_VERSION, "run_id": run_id, "task_id": task_id,
                     "group_id": group_id, "repeat": repeat, "candidate": candidate,
                     "manifest_sha256": manifest_digest}
        output.write(json.dumps(index_row, sort_keys=True) + "\n")
        output.flush()
        os.fsync(output.fileno())
    if (root / INDEX_NAME).exists():
        _write_index_row(root, index_row)
    _verify_sqlite_index(root, _read_jsonl(root / "index.jsonl", "ledger index"))
    return manifest


def _load_indexed_run(root: Path, row: dict[str, Any]) -> dict[str, Any]:
    """Load and re-verify one committed run against its immutable index row."""
    run_id = row["run_id"]
    manifest_path = root / "runs" / run_id / "manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if not _supported_version(manifest.get("schema_version")):
        raise ValueError("run manifest has unsupported schema version")
    if manifest.get("run_id") != run_id or manifest.get("terminal") not in TERMINALS:
        raise ValueError("run manifest identity or terminal status is invalid")
    if hashlib.sha256(manifest_path.read_bytes()).hexdigest() != row.get("manifest_sha256"):
        raise ValueError("run manifest checksum does not match the immutable ledger index")
    decoded_events = _read_jsonl(manifest_path.parent / "events.jsonl", "run event log")
    if not decoded_events:
        raise ValueError("run event log is empty")
    if any(not _supported_version(event.get("schema_version"))
           or event.get("run_id") != run_id or event.get("seq") != index
           for index, event in enumerate(decoded_events, start=1)):
        raise ValueError("run event identity or sequence is invalid")
    terminal_event = decoded_events[-1]
    if terminal_event.get("kind") != "terminal" or terminal_event.get("terminal") != manifest["terminal"]:
        raise ValueError("terminal event disagrees with its run manifest")
    if manifest.get("trace_count") != len(decoded_events) - 1:
        raise ValueError("run trace count does not match event log")
    manifest["usage_unavailable"] = _stored_usage_unavailable(manifest)
    return manifest


def _reconcile_orphan(root: Path, run_id: str) -> dict[str, Any]:
    """Reconcile an attempt directory the index never committed.

    The crash between the manifest write and the index append leaves an
    attempt whose outcome is unknown. It is recorded as an explicit
    `interrupted_unknown` terminal: never a pass, never a failure, and never
    carrying a usage measurement it cannot prove.
    """
    manifest_path = root / "runs" / run_id / "manifest.json"
    manifest: dict[str, Any] | None = None
    if manifest_path.is_file():
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest is None:
        metrics = {field: None for field in _MEASUREMENT_FIELDS}
        metrics["unavailable"] = ["provider_cost", "cpu", "rss", "model-visible context projection",
                                   "native turn id", *USAGE_TOKEN_FIELDS]
        return {"schema_version": SCHEMA_VERSION, "run_id": run_id, "terminal": INTERRUPTED_UNKNOWN,
                "recorded_terminal": None, "reconciled": True, "task_id": None, "group_id": None,
                "repeat": 0, "candidate": None, "suite_sha256": None, "case_sha256": None,
                "created_unix_ms": None, "subject": {}, "policy": None, "session_id": None,
                "turn_id": None, "failure_class": None, "failure_category": None,
                "usage_unavailable": "unspecified", "metrics": metrics, "trace_count": None}
    reconciled = dict(manifest)
    reconciled["terminal"] = INTERRUPTED_UNKNOWN
    reconciled["recorded_terminal"] = manifest.get("terminal")
    reconciled["reconciled"] = True
    metrics = dict(manifest.get("metrics") or {})
    # The attempt's outcome is unknown, so it contributes no measurement: the
    # reconciled record never reports a usage or count it cannot attribute to a
    # finished trial. The raw manifest on disk stays the evidence.
    for field in _MEASUREMENT_FIELDS:
        metrics[field] = None
    metrics["unavailable"] = sorted(set(metrics.get("unavailable") or []) | set(_MEASUREMENT_FIELDS))
    reconciled["metrics"] = metrics
    reconciled["usage_unavailable"] = "unspecified"
    return reconciled


def _reconcile_orphans(root: Path, committed: set[str]) -> list[dict[str, Any]]:
    """Reconcile every attempt directory that has no committed index row."""
    runs_dir = root / "runs"
    if not runs_dir.is_dir():
        return []
    reconciled = []
    for entry in sorted(runs_dir.iterdir()):
        if not entry.is_dir() or entry.name in committed:
            continue
        reconciled.append(_reconcile_orphan(root, entry.name))
    return reconciled


def load_runs(root: Path) -> list[dict[str, Any]]:
    index = root / "index.jsonl"
    runs: list[dict[str, Any]] = []
    seen: set[str] = set()
    if index.exists():
        rows = _read_jsonl(index, "ledger index")
        for row in rows:
            if not _supported_version(row.get("schema_version")):
                raise ValueError("ledger index has unsupported schema version")
            run_id = row.get("run_id")
            if not isinstance(run_id, str) or not run_id:
                raise ValueError("ledger index has a malformed run id")
            if run_id in seen:
                raise ValueError("ledger index has duplicate run id")
            seen.add(run_id)
            runs.append(_load_indexed_run(root, row))
        _verify_sqlite_index(root, rows)
    runs.extend(_reconcile_orphans(root, seen))
    return runs


def validate_candidate_resume_rows(root: Path, candidate: str, cases: list[dict[str, Any]],
                                  suite_sha256: str, default_group_id: str) -> dict[tuple[str, int], dict[str, Any]]:
    """Return immutable completed task/repeat rows after validating the frozen suite."""
    by_task = {case["task_id"]: case for case in cases}
    existing: dict[tuple[str, int], dict[str, Any]] = {}
    for row in load_runs(root):
        if row["candidate"] != candidate:
            continue
        if row.get("terminal") == INTERRUPTED_UNKNOWN:
            # An interrupted attempt is not a completed row: the case is retried
            # under a new attempt id, never overwritten with a success.
            continue
        key = (row["task_id"], row.get("repeat", 0))
        case = by_task.get(key[0])
        if (case is None or key[1] >= case.get("repeats", 1)
                or row.get("suite_sha256") != suite_sha256
                or row.get("case_sha256") != canonical_digest(case)
                or row.get("group_id") != case.get("group_id", default_group_id)
                or key in existing):
            raise ValueError("existing candidate ledger rows do not match the immutable suite")
        existing[key] = row
    return existing


def replay(root: Path, run_id: str) -> dict[str, Any]:
    manifest = next((row for row in load_runs(root) if row["run_id"] == run_id), None)
    if manifest is None:
        raise ValueError("run id is not present in the ledger")
    verifier = manifest.get("verifier") or {}
    expected_text_digest = verifier.get("expected_text_sha256")
    artifact_rel = verifier.get("artifact_path")
    artifact = root / "runs" / run_id / artifact_rel if artifact_rel else None
    if artifact is None or not artifact.is_file():
        actual = None
        replay_status = "passed" if artifact is None and not verifier.get("expected_exists", True) else "indeterminate"
    else:
        data = artifact.read_bytes()
        actual = hashlib.sha256(data).hexdigest()
        matches_hash = verifier.get("expected_exists", True) and (verifier.get("expected_sha256") is None or verifier["expected_sha256"] == actual)
        matches_text = expected_text_digest is None or actual == expected_text_digest
        replay_status = "passed" if matches_hash and matches_text and actual == verifier["observed_sha256"] else "assertion_failed"
    return {"run_id": run_id, "recorded_terminal": manifest["terminal"], "observed_sha256": actual,
            "expected_sha256": verifier.get("expected_sha256"), "expected_text_sha256": expected_text_digest,
            "replay_status": replay_status}


def report(root: Path) -> dict[str, Any]:
    runs = load_runs(root)
    counts: dict[str, int] = {}
    for run in runs:
        counts[run["terminal"]] = counts.get(run["terminal"], 0) + 1
    measured_usage = [row["metrics"] for row in runs
                      if row["metrics"].get("input_tokens") is not None
                      and row["metrics"].get("output_tokens") is not None]
    unavailable_reasons: dict[str, int] = {}
    for run in runs:
        reason = run.get("usage_unavailable")
        if reason is not None:
            unavailable_reasons[reason] = unavailable_reasons.get(reason, 0) + 1
    measured_metrics = [row["metrics"] for row in runs]
    return {"schema_version": SCHEMA_VERSION, "runs": len(runs), "terminals": counts,
            "tool_started": _sum_measured(measured_metrics, "tool_started"),
            "token_usage": ({"measured_runs": len(measured_usage),
                             "input_tokens": _sum_measured(measured_usage, "input_tokens"),
                             "output_tokens": _sum_measured(measured_usage, "output_tokens"),
                             "cached_tokens": _sum_measured(measured_usage, "cached_tokens"),
                             "reasoning_tokens": _sum_measured(measured_usage, "reasoning_tokens")}
                            if measured_usage else None),
            "cost_usd": None,
            "usage_unavailable": unavailable_reasons,
            "repeated_tool_calls": _sum_measured(measured_metrics, "repeated_tool_calls"),
            "recovery_followups": _sum_measured(measured_metrics, "recovery_followups"),
            "missing_metrics": sorted({item for row in runs for item in row["metrics"]["unavailable"]})}


#: Identity fields every trial of ONE candidate must agree on. The subject
#: identity is pinned per candidate: a candidate that changed binary, source
#: revision, agent, model, policy or tool surface mid-suite is not one
#: comparable arm.
_ARM_IDENTITY_FIELDS = (
    ("suite digest", lambda row: row.get("suite_sha256")),
    ("subject binary fingerprint", lambda row: (row.get("subject") or {}).get("binary_sha256")),
    ("revision", lambda row: (row.get("subject") or {}).get("revision")),
    ("agent", lambda row: (row.get("receipt") or {}).get("agent")),
    ("model", lambda row: (row.get("receipt") or {}).get("model")),
    ("policy", lambda row: row.get("policy")),
    ("tool surface digest", lambda row: (row.get("receipt") or {}).get("tool_surface_sha256")),
)

#: Identity fields a PAIRED trial must share across the two candidates. The
#: subject binary fingerprint is deliberately absent: the two candidates are
#: different builds by construction, so their fingerprints are reported rather
#: than compared. A differing model or tool surface is a stratified
#: comparison, which the plan allows.
_PAIR_IDENTITY_FIELDS = (
    ("revision", lambda row: (row.get("subject") or {}).get("revision")),
    ("agent", lambda row: (row.get("receipt") or {}).get("agent")),
    ("policy", lambda row: row.get("policy")),
)


def _identity_key(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def _require_arm_identity(candidate: str, rows: dict[tuple[str, int], dict[str, Any]]) -> None:
    """Refuse a candidate whose own trials disagree on their subject identity."""
    for label, extract in _ARM_IDENTITY_FIELDS:
        values = {_identity_key(extract(row)) for row in rows.values() if extract(row) is not None}
        if len(values) > 1:
            raise ValueError(f"candidate {candidate!r} trials disagree on {label}")


def _arm_identity(rows: dict[tuple[str, int], dict[str, Any]]) -> dict[str, Any]:
    identity: dict[str, Any] = {}
    for label, extract in _ARM_IDENTITY_FIELDS:
        values = [extract(row) for row in rows.values() if extract(row) is not None]
        identity[label] = values[0] if values else None
    return identity


def _expected_terminals(expected_terminals: dict[tuple[str, int], str] | None,
                        left: dict[tuple[str, int], dict[str, Any]],
                        right: dict[tuple[str, int], dict[str, Any]],
                        paired: list[tuple[str, int]]) -> dict[tuple[str, int], str]:
    """Resolve the declared expected terminal for every paired trial.

    The explicit mapping wins: a caller that owns the frozen suite knows the
    expectation for every case and can therefore compare against the *declared*
    contract even if a recorded row forgot to carry it. Otherwise the rows'
    own recorded ``expected_terminal`` is used. A trial with no known
    expectation is refused rather than assumed: a comparison that guesses what
    a trial was supposed to prove cannot be evidence of anything.
    """
    resolved: dict[tuple[str, int], str] = {}
    for task_id, repeat in paired:
        expected: str | None = None
        if expected_terminals is not None:
            expected = expected_terminals.get((task_id, repeat))
        if expected is None:
            for arm in (left, right):
                stored = arm.get((task_id, repeat), {}).get("expected_terminal")
                if isinstance(stored, str):
                    expected = stored
                    break
        if not isinstance(expected, str) or expected not in TERMINALS:
            raise ValueError(
                f"paired trial {task_id!r}/{repeat} has no declared expected terminal; "
                "refusing to compare a trial whose expectation is unknown")
        resolved[(task_id, repeat)] = expected
    return resolved


def compare(root: Path, left_candidate: str, right_candidate: str,
            expected_terminals: dict[tuple[str, int], str] | None = None) -> dict[str, Any]:
    """Compare two candidate arms trial by trial.

    A regression is a trial the **left** arm got right and the **right** arm got
    wrong, where "right" means *the recorded terminal matched that trial's
    declared expectation*. The expectation is the case's ``expected_terminal``
    (``passed`` unless the case says otherwise), not the literal string
    ``passed``.

    Comparing raw terminals instead would score a protected negative control
    backwards. A control expects a non-``passed`` terminal, so a baseline that
    happened to record ``passed`` for it did not *satisfy* the control; a
    candidate that recorded the expected failure *did*. Under the raw-terminal
    rule that candidate was reported as a regression for being more correct
    than the baseline. Under the expectation-relative rule the baseline counts
    as an unmet control and the candidate counts as a satisfied one, and a
    candidate that breaks a control the baseline satisfied is still a
    regression.
    """
    if left_candidate == right_candidate:
        raise ValueError("comparison requires two distinct candidates; refusing to compare a candidate against itself")
    runs = load_runs(root)
    left: dict[tuple[str, int], dict[str, Any]] = {}
    right: dict[tuple[str, int], dict[str, Any]] = {}
    left_unknown: list[dict[str, Any]] = []
    right_unknown: list[dict[str, Any]] = []
    for row in runs:
        candidate = row.get("candidate")
        if row.get("terminal") == INTERRUPTED_UNKNOWN:
            # An unknown outcome is not a finished trial: it stays visible and
            # keeps the pairing incomplete, it is never scored.
            if candidate == left_candidate:
                left_unknown.append(row)
            elif candidate == right_candidate:
                right_unknown.append(row)
            continue
        target = left if candidate == left_candidate else right if candidate == right_candidate else None
        if target is not None:
            key = (row["task_id"], row.get("repeat", 0))
            if key in target:
                raise ValueError("candidate contains duplicate paired task/repeat records")
            target[key] = row
    _require_arm_identity(left_candidate, left)
    _require_arm_identity(right_candidate, right)
    left_suites = {row.get("suite_sha256") for row in left.values()}
    right_suites = {row.get("suite_sha256") for row in right.values()}
    if not left_suites or not right_suites:
        raise ValueError("comparison has an empty expected denominator: no completed trials to pair")
    if left_suites != right_suites:
        raise ValueError("paired candidate trials do not share the same frozen suite")
    suite = next(iter(left_suites))
    expected = {(row["task_id"], row.get("repeat", 0)) for row in runs if row.get("suite_sha256") == suite}
    if not expected:
        raise ValueError("comparison has an empty expected denominator for the frozen suite")
    paired = sorted(set(left) & set(right))
    if len(paired) != len(expected):
        missing = sorted(expected - set(paired))
        raise ValueError(f"paired candidate trials are incomplete: {len(paired)} of {len(expected)} "
                         f"expected (task, repeat) pairs are paired; missing "
                         f"{[f'{task}/{repeat}' for task, repeat in missing]}")
    for key in paired:
        left_row, right_row = left[key], right[key]
        if (left_row.get("group_id"), left_row.get("case_sha256"), left_row.get("suite_sha256")) != (
                right_row.get("group_id"), right_row.get("case_sha256"), right_row.get("suite_sha256")):
            raise ValueError("paired candidate trials do not share the same frozen task definition")
        for label, extract in _PAIR_IDENTITY_FIELDS:
            left_value = extract(left_row)
            right_value = extract(right_row)
            if left_value is None or right_value is None:
                continue
            if _identity_key(left_value) != _identity_key(right_value):
                raise ValueError(f"paired candidate trials disagree on {label}")
    terminal_pairs = [{"task_id": task_id, "repeat": repeat,
                       "left_terminal": left[(task_id, repeat)]["terminal"],
                       "right_terminal": right[(task_id, repeat)]["terminal"]}
                      for task_id, repeat in paired]
    expectations = _expected_terminals(expected_terminals, left, right, paired)
    for row in terminal_pairs:
        expected = expectations[(row["task_id"], row["repeat"])]
        row["expected_terminal"] = expected
        row["left_matched"] = row["left_terminal"] == expected
        row["right_matched"] = row["right_terminal"] == expected
    regressions = [row for row in terminal_pairs
                   if row["left_matched"] and not row["right_matched"]]
    improvements = [row for row in terminal_pairs
                    if row["right_matched"] and not row["left_matched"]]
    unmet = [{"task_id": task_id, "repeat": repeat,
              "left_terminal": left[(task_id, repeat)]["terminal"],
              "right_terminal": right[(task_id, repeat)]["terminal"],
              "expected_terminal": expectations[(task_id, repeat)]}
             for task_id, repeat in paired
             if left[(task_id, repeat)]["terminal"] != expectations[(task_id, repeat)]
             or right[(task_id, repeat)]["terminal"] != expectations[(task_id, repeat)]]
    measured_usage = {side: [arm[key]["metrics"] for key in paired
                             if arm[key]["metrics"].get("input_tokens") is not None
                             and arm[key]["metrics"].get("output_tokens") is not None]
                      for side, arm in (("left", left), ("right", right))}
    return {"left": left_candidate, "right": right_candidate,
            "paired_trials": [{"task_id": task_id, "repeat": repeat} for task_id, repeat in paired],
            "paired_task_ids": sorted({task_id for task_id, _ in paired}),
            "terminal_pairs": terminal_pairs, "regressions": regressions,
            "improvements": improvements, "unmet_expectations": unmet,
            "left_passed": sum(left[key]["terminal"] == "passed" for key in paired),
            "right_passed": sum(right[key]["terminal"] == "passed" for key in paired),
            "denominator": len(paired), "unpaired_left": sorted(set(left) - set(right)),
            "unpaired_right": sorted(set(right) - set(left)), "tokens": None, "cost_usd": None,
            "measured_token_usage": {side: ({"measured_runs": len(rows),
                                             "input_tokens": _sum_measured(rows, "input_tokens"),
                                             "output_tokens": _sum_measured(rows, "output_tokens"),
                                             "cached_tokens": _sum_measured(rows, "cached_tokens"),
                                             "reasoning_tokens": _sum_measured(rows, "reasoning_tokens")}
                                            if rows else None)
                                    for side, rows in measured_usage.items()},
            "interrupted_trials": {"left": [{"run_id": row["run_id"], "task_id": row.get("task_id"),
                                             "repeat": row.get("repeat", 0)} for row in left_unknown],
                                   "right": [{"run_id": row["run_id"], "task_id": row.get("task_id"),
                                              "repeat": row.get("repeat", 0)} for row in right_unknown]},
            "subject_identity": {"left": _arm_identity(left), "right": _arm_identity(right)}}

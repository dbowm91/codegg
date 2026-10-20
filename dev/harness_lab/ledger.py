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


SCHEMA_VERSION = 1
TERMINALS = {"passed", "assertion_failed", "execution_failed", "cancelled", "denied", "timeout", "budget_exhausted", "infrastructure_error", "indeterminate"}
INDEX_NAME = "index.sqlite3"


def canonical_digest(value: Any) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


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


def _write_index_row(root: Path, row: dict[str, Any]) -> None:
    database = root / INDEX_NAME
    with sqlite3.connect(database) as connection:
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


def _verify_sqlite_index(root: Path, rows: list[dict[str, Any]]) -> None:
    database = root / INDEX_NAME
    expected = [(row["run_id"], row["task_id"], row["group_id"], row["repeat"],
                 row["candidate"], row["manifest_sha256"]) for row in rows]
    if not database.exists():
        for row in rows:
            _write_index_row(root, row)
        return
    try:
        with sqlite3.connect(f"file:{database}?mode=ro", uri=True) as connection:
            actual = connection.execute("SELECT run_id, task_id, group_id, repeat, candidate, manifest_sha256 FROM runs ORDER BY rowid").fetchall()
    except sqlite3.Error as error:
        raise ValueError("SQLite ledger index is unreadable or has an unsupported schema") from error
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
           terminal_override: str | None = None, failure_class: str | None = None) -> dict[str, Any]:
    if timeout_seconds <= 0:
        raise ValueError("timeout must be positive")
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
        "failure_class": failure_class,
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
    _verify_sqlite_index(root, [json.loads(line) for line in (root / "index.jsonl").read_text(encoding="utf-8").splitlines()])
    return manifest


def load_runs(root: Path) -> list[dict[str, Any]]:
    index = root / "index.jsonl"
    if not index.exists():
        return []
    seen: set[str] = set()
    runs = []
    for line in index.read_text(encoding="utf-8").splitlines():
        row = json.loads(line)
        if row.get("schema_version") != SCHEMA_VERSION or row["run_id"] in seen:
            raise ValueError("ledger index has unsupported schema or duplicate run id")
        seen.add(row["run_id"])
        manifest = json.loads((root / "runs" / row["run_id"] / "manifest.json").read_text(encoding="utf-8"))
        if manifest.get("run_id") != row["run_id"] or manifest.get("terminal") not in TERMINALS:
            raise ValueError("run manifest identity or terminal status is invalid")
        manifest_path = root / "runs" / row["run_id"] / "manifest.json"
        if hashlib.sha256(manifest_path.read_bytes()).hexdigest() != row.get("manifest_sha256"):
            raise ValueError("run manifest checksum does not match the immutable ledger index")
        events = (manifest_path.parent / "events.jsonl").read_text(encoding="utf-8").splitlines()
        if not events:
            raise ValueError("run event log is empty")
        decoded_events = [json.loads(line) for line in events]
        if any(event.get("run_id") != row["run_id"] or event.get("seq") != index
               for index, event in enumerate(decoded_events, start=1)):
            raise ValueError("run event identity or sequence is invalid")
        terminal_event = decoded_events[-1]
        if terminal_event.get("kind") != "terminal" or terminal_event.get("terminal") != manifest["terminal"]:
            raise ValueError("terminal event disagrees with its run manifest")
        if manifest.get("trace_count") != len(decoded_events) - 1:
            raise ValueError("run trace count does not match event log")
        runs.append(manifest)
    _verify_sqlite_index(root, [json.loads(line) for line in index.read_text(encoding="utf-8").splitlines()])
    return runs


def validate_candidate_resume_rows(root: Path, candidate: str, cases: list[dict[str, Any]],
                                  suite_sha256: str, default_group_id: str) -> dict[tuple[str, int], dict[str, Any]]:
    """Return immutable completed task/repeat rows after validating the frozen suite."""
    by_task = {case["task_id"]: case for case in cases}
    existing: dict[tuple[str, int], dict[str, Any]] = {}
    for row in load_runs(root):
        if row["candidate"] != candidate:
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
    verifier = manifest["verifier"]
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
    return {"schema_version": SCHEMA_VERSION, "runs": len(runs), "terminals": counts,
            "tool_started": sum(row["metrics"]["tool_started"] or 0 for row in runs),
            "token_usage": ({"measured_runs": len(measured_usage),
                             "input_tokens": sum(row["input_tokens"] for row in measured_usage),
                             "output_tokens": sum(row["output_tokens"] for row in measured_usage),
                             "cached_tokens": sum(row["cached_tokens"] or 0 for row in measured_usage),
                             "reasoning_tokens": sum(row["reasoning_tokens"] or 0 for row in measured_usage)}
                            if measured_usage else None),
            "cost_usd": None,
            "repeated_tool_calls": sum(row["metrics"].get("repeated_tool_calls", 0) for row in runs),
            "recovery_followups": sum(row["metrics"].get("recovery_followups", 0) for row in runs),
            "missing_metrics": sorted({item for row in runs for item in row["metrics"]["unavailable"]})}


def compare(root: Path, left_candidate: str, right_candidate: str) -> dict[str, Any]:
    runs = load_runs(root)
    left: dict[tuple[str, int], dict[str, Any]] = {}
    right: dict[tuple[str, int], dict[str, Any]] = {}
    for row in runs:
        target = left if row["candidate"] == left_candidate else right if row["candidate"] == right_candidate else None
        if target is not None:
            key = (row["task_id"], row.get("repeat", 0))
            if key in target:
                raise ValueError("candidate contains duplicate paired task/repeat records")
            target[key] = row
    paired = sorted(set(left) & set(right))
    for key in paired:
        left_row, right_row = left[key], right[key]
        if (left_row.get("group_id"), left_row.get("case_sha256"), left_row.get("suite_sha256")) != (
                right_row.get("group_id"), right_row.get("case_sha256"), right_row.get("suite_sha256")):
            raise ValueError("paired candidate trials do not share the same frozen task definition")
    terminal_pairs = [{"task_id": task_id, "repeat": repeat,
                       "left_terminal": left[(task_id, repeat)]["terminal"],
                       "right_terminal": right[(task_id, repeat)]["terminal"]}
                      for task_id, repeat in paired]
    regressions = [row for row in terminal_pairs
                   if row["left_terminal"] == "passed" and row["right_terminal"] != "passed"]
    return {"left": left_candidate, "right": right_candidate,
            "paired_trials": [{"task_id": task_id, "repeat": repeat} for task_id, repeat in paired],
            "paired_task_ids": sorted({task_id for task_id, _ in paired}),
            "terminal_pairs": terminal_pairs, "regressions": regressions,
            "left_passed": sum(left[key]["terminal"] == "passed" for key in paired),
            "right_passed": sum(right[key]["terminal"] == "passed" for key in paired),
            "denominator": len(paired), "unpaired_left": sorted(set(left) - set(right)),
            "unpaired_right": sorted(set(right) - set(left)), "tokens": None, "cost_usd": None}

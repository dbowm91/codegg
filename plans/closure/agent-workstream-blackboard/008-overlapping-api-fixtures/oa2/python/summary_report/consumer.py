from __future__ import annotations

import json
import re
from datetime import datetime, timezone
from typing import Any


SCHEMA = "codegg.project-summary.v1"
KINDS = ("file_changed", "test_passed", "test_failed")


class SummaryError(ValueError):
    """Invalid or unsupported project-summary document."""


def load_summary(text: str) -> dict[str, Any]:
    """Parse a summary JSON document, rejecting non-standard constants."""
    def reject_constant(value: str) -> None:
        raise SummaryError(f"invalid JSON constant: {value}")

    try:
        value = json.loads(text, parse_constant=reject_constant)
    except json.JSONDecodeError as exc:
        raise SummaryError(f"invalid JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise SummaryError("summary must be a JSON object")
    return value


def _object(value: Any, name: str, required: set[str]) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise SummaryError(f"{name} must be an object")
    missing = required - value.keys()
    extra = value.keys() - required
    if missing:
        raise SummaryError(f"{name} missing required field(s): {', '.join(sorted(missing))}")
    if extra:
        raise SummaryError(f"{name} has unknown field(s): {', '.join(sorted(extra))}")
    return value


def _count(value: Any, name: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise SummaryError(f"{name} must be a nonnegative integer count")
    return value


def _safe_path(path: Any) -> bool:
    return (isinstance(path, str) and bool(path) and not path.startswith("/")
            and not (len(path) >= 2 and path[1] == ":")
            and "\\" not in path and not any(ord(char) < 32 or ord(char) == 127 for char in path)
            and all(part not in ("", ".", "..") for part in path.split("/")))


def _timestamp(value: Any) -> datetime:
    if not isinstance(value, str) or not re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", value):
        raise SummaryError("occurred_at must be a UTC timestamp in YYYY-MM-DDTHH:MM:SSZ format")
    try:
        parsed = datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
    except ValueError as exc:
        raise SummaryError(f"invalid occurred_at timestamp: {exc}") from exc
    return parsed


def validate_summary(summary: dict[str, Any]) -> dict[str, Any]:
    """Validate the complete v1 interface and all derived aggregates."""
    summary = _object(summary, "summary", {"schema", "source_revision", "events", "totals", "by_kind"})
    if not isinstance(summary["schema"], str) or summary["schema"] != SCHEMA:
        raise SummaryError(f"unsupported schema/version {summary['schema']!r}")
    if not isinstance(summary["source_revision"], str) or not summary["source_revision"]:
        raise SummaryError("source_revision must be a nonempty string")
    events = summary["events"]
    if not isinstance(events, list):
        raise SummaryError("events must be an array")

    counts = {kind: 0 for kind in KINDS}
    totals = {"events": 0, "files_changed": 0, "insertions": 0, "deletions": 0}
    seen: set[str] = set()
    previous: tuple[str, str] | None = None
    for index, event in enumerate(events, 1):
        if not isinstance(event, dict):
            raise SummaryError(f"event {index} must be an object")
        kind = event.get("kind")
        required = {"id", "occurred_at", "kind"} | ({"path", "insertions", "deletions"} if kind == "file_changed" else set())
        event = _object(event, f"event {index}", required)
        event_id = event["id"]
        if not isinstance(event_id, str) or not event_id:
            raise SummaryError(f"event {index} id must be a nonempty string")
        if event_id in seen:
            raise SummaryError(f"duplicate event id {event_id!r}; IDs must be unique")
        seen.add(event_id)
        if kind not in KINDS:
            raise SummaryError(f"event {index} has invalid kind {kind!r}")
        timestamp = event["occurred_at"]
        parsed = _timestamp(timestamp)
        key = (parsed.strftime("%Y-%m-%dT%H:%M:%SZ"), event_id)
        if previous is not None and key < previous:
            raise SummaryError(f"events are not sorted by occurred_at and id at event {index}")
        previous = key
        counts[kind] += 1
        totals["events"] += 1
        if kind == "file_changed":
            if not _safe_path(event["path"]):
                raise SummaryError(f"event {index} has unsafe or non-normalized path")
            insertions = _count(event["insertions"], f"event {index} insertions")
            deletions = _count(event["deletions"], f"event {index} deletions")
            totals["files_changed"] += 1
            totals["insertions"] += insertions
            totals["deletions"] += deletions

    expected_totals = _object(summary["totals"], "totals", set(totals))
    for name, expected in totals.items():
        if _count(expected_totals[name], f"totals.{name}") != expected:
            raise SummaryError(f"totals.{name} aggregate does not match events")
    by_kind = _object(summary["by_kind"], "by_kind", set(KINDS))
    for kind, expected in counts.items():
        if _count(by_kind[kind], f"by_kind.{kind}") != expected:
            raise SummaryError(f"by_kind.{kind} aggregate does not match events")
    return summary


def _escape_cell(value: str) -> str:
    return value.replace("\\", "\\\\").replace("|", "\\|").replace("\n", "<br>").replace("\r", "")


def render_report(summary: dict[str, Any]) -> str:
    """Validate and render a deterministic Markdown report."""
    validate_summary(summary)
    totals = summary["totals"]
    lines = [
        "# Project summary", "",
        f"- Source revision: `{summary['source_revision']}`",
        f"- Events: {totals['events']}",
        f"- Files changed: {totals['files_changed']}",
        f"- Insertions: {totals['insertions']}",
        f"- Deletions: {totals['deletions']}",
        "", "## Events by kind", "", "| Kind | Count |", "| --- | ---: |",
    ]
    for kind in KINDS:
        lines.append(f"| {kind} | {summary['by_kind'][kind]} |")
    lines.extend(["", "## File changes", "", "| Occurred at (UTC) | Event ID | Path | Insertions | Deletions |", "| --- | --- | --- | ---: | ---: |"])
    for event in summary["events"]:
        if event["kind"] == "file_changed":
            lines.append(f"| {event['occurred_at']} | {_escape_cell(event['id'])} | {_escape_cell(event['path'])} | {event['insertions']} | {event['deletions']} |")
    return "\n".join(lines) + "\n"

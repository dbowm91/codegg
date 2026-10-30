"""Offline, evidence-limited attribution for the seven H007 live attempts."""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path, PurePosixPath
from typing import Any

SCHEMA = "h007-attribution-v1"
DEFAULT_MANIFEST = Path(
    "plans/closure/harness-lab-self-improvement-resumption/evidence/s001-input-manifest.json"
)
EXPECTED_TRIALS = tuple(f"H007-{number}" for number in range(1, 8))
TASK_OUTCOMES = {"passed", "failed", "inconclusive"}
HORIZON_OUTCOMES = {"qualified", "not_qualified", "not_observed"}
HORIZON_REASONS = {
    "activity_span_short", "activity_gap_exceeded", "job_lifecycle_missing",
    "repair_cycles_insufficient", "continuation_missing", "task_completed_before_threshold",
    "measurement_incomplete", None,
}


class AttributionInputError(ValueError):
    """Raised when pinned attribution inputs are missing or changed."""


def _digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _relative_path(value: Any) -> str:
    if not isinstance(value, str) or not value:
        raise AttributionInputError("input path must be a nonempty relative path")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        raise AttributionInputError("input path escapes or is not normalized")
    return path.as_posix()


def _repo_bytes(repo_root: Path, relative: str) -> bytes:
    rel = _relative_path(relative)
    try:
        resolved = (repo_root / rel).resolve(strict=True)
        resolved.relative_to(repo_root.resolve(strict=True))
        return resolved.read_bytes()
    except (OSError, ValueError) as exc:
        raise AttributionInputError(f"repository input unavailable: {rel}") from exc


def _source_blob(repo_root: Path, revision: str, relative: str) -> bytes:
    rel = _relative_path(relative)
    if not re.fullmatch(r"[0-9a-f]{40,64}", revision):
        raise AttributionInputError("baseline source revision is not a full object ID")
    commit = subprocess.run(
        ["git", "cat-file", "-e", f"{revision}^{{commit}}"], cwd=repo_root,
        stdin=subprocess.DEVNULL, capture_output=True, check=False, timeout=15,
    )
    if commit.returncode:
        # Hosted CI uses a shallow checkout. The frozen inputs also live in the
        # checked-out tree and are verified against their recorded digests below.
        return _repo_bytes(repo_root, rel)
    result = subprocess.run(
        ["git", "cat-file", "blob", f"{revision}:{rel}"], cwd=repo_root,
        stdin=subprocess.DEVNULL, capture_output=True, check=False, timeout=15,
    )
    if result.returncode:
        raise AttributionInputError(f"pinned source file unavailable: {rel}")
    return result.stdout


def _read_json(data: bytes, label: str) -> dict[str, Any]:
    try:
        value = json.loads(data)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise AttributionInputError(f"invalid JSON input: {label}") from exc
    if not isinstance(value, dict):
        raise AttributionInputError(f"JSON input must be an object: {label}")
    return value


def _verify_case_identity(repo_root: Path, revision: str,
                          identity: dict[str, Any]) -> dict[str, Any]:
    paths = {
        "manifest": _relative_path(identity.get("manifest_path")),
        "metadata": _relative_path(identity.get("metadata_path")),
        "verifier": _relative_path(identity.get("verifier_path")),
        "fixture_root": _relative_path(identity.get("fixture_root")),
    }
    digests = {
        "manifest": identity.get("manifest_sha256"),
        "metadata": identity.get("metadata_sha256"),
        "verifier": identity.get("verifier_sha256"),
    }
    for name, path in paths.items():
        if name == "fixture_root":
            continue
        expected = digests[name]
        if not isinstance(expected, str) or not re.fullmatch(r"[0-9a-f]{64}", expected):
            raise AttributionInputError(f"missing pinned {name} digest")
        if _digest(_source_blob(repo_root, revision, path)) != expected:
            raise AttributionInputError(f"pinned {name} digest does not match baseline source")

    metadata = _read_json(_source_blob(repo_root, revision, paths["metadata"]), paths["metadata"])
    if metadata.get("case_id") != "build-provenance-catalog":
        raise AttributionInputError("pinned case metadata has unexpected case ID")
    if metadata.get("manifest_sha256") != digests["manifest"]:
        raise AttributionInputError("case metadata does not bind the pinned manifest")
    if metadata.get("hidden_verifier_sha256") != digests["verifier"]:
        raise AttributionInputError("case metadata does not bind the pinned verifier")

    fixture_files = identity.get("fixture_files")
    if not isinstance(fixture_files, dict) or fixture_files != metadata.get("frozen_sha256"):
        raise AttributionInputError("frozen fixture file list differs from case metadata")
    for relative, expected in sorted(fixture_files.items()):
        rel = _relative_path(relative)
        if not isinstance(expected, str) or not re.fullmatch(r"[0-9a-f]{64}", expected):
            raise AttributionInputError(f"invalid frozen fixture digest: {rel}")
        path = f"{paths['fixture_root']}/{rel}"
        if _digest(_source_blob(repo_root, revision, path)) != expected:
            raise AttributionInputError(f"frozen fixture changed at baseline: {rel}")
    return {**paths, **{f"{name}_sha256": value for name, value in digests.items()}}


def _job_summary(receipt: dict[str, Any]) -> dict[str, Any]:
    job = receipt.get("job_lifecycle")
    if not isinstance(job, dict):
        return {"same_id_running_and_terminal_observed": None,
                "productive_overlap": None, "submit_count": None,
                "status_observation_count": None}
    observations = job.get("observations")
    if isinstance(observations, list):
        actions = [row for row in observations if isinstance(row, dict)]
    else:
        jobs = job.get("jobs")
        actions = [action for item in jobs if isinstance(item, dict)
                   for action in item.get("source_actions", []) if isinstance(action, dict)] \
            if isinstance(jobs, list) else []
    same = job.get("same_id_running_and_terminal_observed")
    overlap = job.get("productive_work_during_job", job.get("productive_overlap"))
    return {
        "same_id_running_and_terminal_observed": same if isinstance(same, bool) else None,
        "productive_overlap": overlap if isinstance(overlap, bool) else None,
        "submit_count": sum(row.get("action") == "submit" for row in actions),
        "status_observation_count": sum(row.get("action") in {"status", "wait"}
                                        for row in actions),
    }


def _verifier_summary(receipt: dict[str, Any]) -> dict[str, Any]:
    verifier = receipt.get("independent_verifier")
    if not isinstance(verifier, dict):
        return {"state": "unknown", "exit_code": None, "tests_run": None,
                "failure_summary": None}
    exit_code = verifier.get("exit_code")
    tests = verifier.get("tests_run")
    if isinstance(exit_code, int) and exit_code == 0:
        state = "passed"
    elif isinstance(tests, int) and tests == 0 and exit_code not in (None, 0):
        state = "error_before_tests_completed"
    elif isinstance(exit_code, int):
        state = "tests_failed"
    else:
        state = "unknown"
    return {
        "state": state,
        "exit_code": exit_code if isinstance(exit_code, int) else None,
        "tests_run": tests if isinstance(tests, int) and tests >= 0 else None,
        "failure_summary": verifier.get("failure_summary")
        if isinstance(verifier.get("failure_summary"), str) else None,
    }


def _classify(trial_id: str, receipt: dict[str, Any],
              operator_findings: dict[str, Any]) -> dict[str, Any]:
    failure = receipt.get("terminal_failure")
    note = operator_findings.get(trial_id)
    note = note if isinstance(note, dict) else {}
    timeout = isinstance(failure, dict) and failure.get("category") == "provider_timeout"
    if not timeout and isinstance(note, dict):
        timeout = note.get("typed_provider_timeout") is True
    verifier = _verifier_summary(receipt)
    if timeout:
        classification, owner = "provider_boundary_timeout", "provider route"
        symptom = "Typed provider timeout observed; underlying transport cause is unresolved."
    elif verifier["state"] == "error_before_tests_completed":
        classification = "candidate_or_verifier_setup_failure"
        owner = note.get("suspected_owner", "unknown")
        symptom = "Verifier setup ended before contract tests completed."
    elif receipt.get("task_outcome") == "failed":
        classification, owner = "task_or_qualification_failure", "agent-task"
        symptom = "The recorded task or qualification criteria failed; model intent is not inferred."
    else:
        classification, owner = "inconclusive_task_or_measurement", "unknown"
        symptom = "The receipt does not establish task completion or a CodeGG-layer cause."

    job = _job_summary(receipt)
    reason_note = receipt.get("horizon_reason_projection_note")
    reason_consistent = not isinstance(reason_note, dict) or (
        receipt.get("horizon_reason") == reason_note.get("clarified_reason")
    )
    operator_summary = note.get("summary")
    if not isinstance(operator_summary, str) and not reason_consistent:
        operator_summary = (
            "Historical receipt reason conflicts with its own clarification: "
            f"{receipt.get('horizon_reason')} vs {reason_note.get('clarified_reason')}."
        )
    return {
        "trial_id": trial_id,
        "receipt_sha256": None,
        "implementation_revision": receipt.get("implementation_revision"),
        "binary_sha256": receipt.get("binary_sha256"),
        "classification": classification,
        "observed_outcome": {
            "terminal": receipt.get("terminal"),
            "task_outcome": receipt.get("task_outcome"),
            "horizon_outcome": receipt.get("horizon_outcome"),
            "horizon_reason": receipt.get("horizon_reason"),
            "active_work_span_ms": receipt.get("active_work_span_ms"),
            "repair_cycles": receipt.get("repair_cycles"),
            "continuation_confirmed": receipt.get("continuation_confirmed"),
            "verifier": verifier,
            "job_lifecycle": job,
            "event_capture": receipt.get("event_capture"),
            "provider_requests": receipt.get("provider_requests"),
        },
        "evidence_state": "observed",
        "recorded_horizon_reason_consistent": reason_consistent,
        "reason_evidence_state": "observed" if reason_consistent else "contradicted",
        "symptom": symptom,
        "suspected_owner": owner,
        "cause_assertion": {
            "established": False,
            "assertion": None,
            "evidence_pointers": [
                "receipt#/terminal_failure",
                "receipt#/independent_verifier",
                "receipt#/event_capture",
            ],
        },
        "codegg_defect_reproduced": False,
        "next_action": "No CodeGG patch: carry the evidence-qualified disposition into S002.",
        "operator_note": operator_summary if isinstance(operator_summary, str) else None,
    }


def build_report(repo_root: Path, manifest_path: Path) -> dict[str, Any]:
    """Validate pinned inputs and produce a stable, credential-free diagnosis."""
    repo = repo_root.resolve(strict=True)
    manifest_rel = manifest_path.as_posix()
    manifest_bytes = _repo_bytes(repo, manifest_rel)
    manifest = _read_json(manifest_bytes, manifest_rel)
    if manifest.get("schema_version") != 1 or manifest.get("report_schema") != SCHEMA:
        raise AttributionInputError("unsupported attribution input manifest schema")
    if manifest.get("case_id") != "build-provenance-catalog":
        raise AttributionInputError("input manifest has unexpected case ID")
    revision = manifest.get("baseline_source_sha")
    identity = manifest.get("case_identity")
    if not isinstance(identity, dict) or not isinstance(revision, str):
        raise AttributionInputError("missing source or case identity")
    verified_identity = _verify_case_identity(repo, revision, identity)

    receipt_rows = manifest.get("receipts")
    if not isinstance(receipt_rows, list):
        raise AttributionInputError("receipt list is missing")
    ids = [row.get("trial_id") for row in receipt_rows if isinstance(row, dict)]
    if len(ids) != len(receipt_rows) or len(set(ids)) != len(ids):
        raise AttributionInputError("receipt trial IDs must be present and unique")
    if tuple(ids) != EXPECTED_TRIALS:
        raise AttributionInputError("receipt set must contain H007-1 through H007-7 in order")

    expected_identity = {
        "manifest_sha256": identity.get("manifest_sha256"),
        "fixture_metadata_sha256": identity.get("metadata_sha256"),
        "hidden_verifier_sha256": identity.get("verifier_sha256"),
    }
    findings = manifest.get("operator_findings", {})
    if not isinstance(findings, dict):
        raise AttributionInputError("operator findings must be an object")
    rows = []
    for spec in receipt_rows:
        trial_id = spec["trial_id"]
        rel = _relative_path(spec.get("path"))
        expected = spec.get("sha256")
        if not isinstance(expected, str) or not re.fullmatch(r"[0-9a-f]{64}", expected):
            raise AttributionInputError(f"receipt digest missing for {trial_id}")
        raw = _repo_bytes(repo, rel)
        if _digest(raw) != expected:
            raise AttributionInputError(f"receipt digest changed for {trial_id}")
        receipt = _read_json(raw, rel)
        if receipt.get("schema_version") != 1 or receipt.get("case_id") != manifest["case_id"]:
            raise AttributionInputError(f"unexpected receipt schema or case for {trial_id}")
        if receipt.get("task_outcome") not in TASK_OUTCOMES:
            raise AttributionInputError(f"invalid task outcome for {trial_id}")
        if receipt.get("horizon_outcome") not in HORIZON_OUTCOMES:
            raise AttributionInputError(f"invalid horizon outcome for {trial_id}")
        if receipt.get("horizon_reason") not in HORIZON_REASONS:
            raise AttributionInputError(f"invalid horizon reason for {trial_id}")
        owner = findings.get(trial_id)
        if isinstance(owner, dict) and owner.get("suspected_owner") not in {
            "agent-task", "fixture", "CodeGG tool/ACP", "provider route", "evaluator", "unknown"
        }:
            raise AttributionInputError(f"invalid suspected owner for {trial_id}")
        requests = receipt.get("provider_requests")
        source = receipt.get("provider_request_count_source")
        if not isinstance(requests, int) or isinstance(requests, bool) or not 0 <= requests <= 60:
            raise AttributionInputError(f"provider request count is invalid for {trial_id}")
        if source != "codegg_provider_boundary_receipt":
            raise AttributionInputError(f"provider request provenance is invalid for {trial_id}")
        if receipt.get("billed_usd") != "unknown":
            raise AttributionInputError(f"billed USD must remain unknown for {trial_id}")
        for field, expected_value in expected_identity.items():
            actual = receipt.get(field)
            if actual is not None and actual != expected_value:
                raise AttributionInputError(f"case identity mismatch in {trial_id}: {field}")
        if not isinstance(receipt.get("independent_verifier"), dict):
            raise AttributionInputError(f"verifier evidence missing for {trial_id}")
        if not isinstance(receipt.get("event_capture"), dict):
            raise AttributionInputError(f"event evidence missing for {trial_id}")
        row = _classify(trial_id, receipt, findings)
        row["evidence_state"] = (
            "contradicted" if not row["recorded_horizon_reason_consistent"] else "observed"
        )
        row["receipt_sha256"] = expected
        row["receipt_path"] = rel
        row["case_identity_state"] = (
            "explicit_in_receipt" if all(receipt.get(field) == value
                                          for field, value in expected_identity.items())
            else "shared_pinned_case_identity; receipt_fields_unavailable"
        )
        rows.append(row)

    outcomes = [row["observed_outcome"] for row in rows]
    jobs = [row["job_lifecycle"]["same_id_running_and_terminal_observed"] for row in outcomes]
    repairs = [row["repair_cycles"] for row in outcomes if isinstance(row["repair_cycles"], int)]
    spans = [row["active_work_span_ms"] for row in outcomes
             if isinstance(row["active_work_span_ms"], int)]
    event_gaps = [row["event_capture"].get("gap_count") for row in outcomes
                  if isinstance(row.get("event_capture"), dict)]
    sidecars = manifest.get("companion_inventory")
    if not isinstance(sidecars, dict) or sidecars.get("committed_sanitized_receipts") != 7:
        raise AttributionInputError("companion inventory must record all seven committed receipts")

    return {
        "schema_version": 1,
        "report_schema": SCHEMA,
        "baseline_source_sha": revision,
        "input_manifest_sha256": _digest(manifest_bytes),
        "case_identity": verified_identity,
        "companion_inventory": sidecars,
        "attempt_count": len(rows),
        "summary": {
            "task_outcomes": {outcome: sum(row["observed_outcome"]["task_outcome"] == outcome
                                             for row in rows)
                              for outcome in sorted(TASK_OUTCOMES)},
            "provider_boundary_timeouts": sum(
                row["classification"] == "provider_boundary_timeout" for row in rows),
            "qualified_job_lifecycles": sum(value is True for value in jobs),
            "repair_cycles_recorded": repairs,
            "maximum_active_work_span_ms": max(spans, default=0),
            "event_gap_counts": event_gaps,
            "verified_codegg_defects": 0,
        },
        "causal_limit": (
            "A zero-gap summary describes only the recorded observation window. "
            "No full redacted event ledger was committed; no root cause is inferred "
            "from end_turn, missing job calls, or a verifier setup error."
        ),
        "candidate_fix_queue": [],
        "disposition": "no_reproducible_codegg_defect; proceed to S002 offline instrumentation qualification",
        "trials": rows,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args(argv)
    try:
        report = build_report(args.repo_root, args.manifest)
    except (AttributionInputError, OSError, subprocess.TimeoutExpired) as exc:
        print(f"H007 attribution refused: {exc}", file=sys.stderr)
        return 2
    rendered = json.dumps(report, sort_keys=True, indent=2) + "\n"
    if args.output is None:
        sys.stdout.write(rendered)
        return 0
    output = args.output if args.output.is_absolute() else args.repo_root / args.output
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(rendered, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

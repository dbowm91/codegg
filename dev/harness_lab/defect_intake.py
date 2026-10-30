"""Deterministic, no-promotion defect intake built from the frozen S001 report."""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path, PurePosixPath
from typing import Any

from . import h007_attribution

SCHEMA = "defect_dossier_v1"
DEFAULT_REPORT = Path(
    "plans/closure/harness-lab-self-improvement-resumption/evidence/s001-attribution.json"
)
HISTORICAL_C003 = Path("plans/closure/harness-lab-post-pilot-corrective/003-status.md")


class IntakeError(ValueError):
    """Raised when evidence is missing, changed, or too weak for intake."""


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _repo_read(root: Path, relative: str) -> bytes:
    path = PurePosixPath(relative)
    if path.is_absolute() or ".." in path.parts or not path.parts:
        raise IntakeError("evidence path is not a normalized repository-relative path")
    try:
        resolved = (root / path.as_posix()).resolve(strict=True)
        resolved.relative_to(root.resolve(strict=True))
        return resolved.read_bytes()
    except (OSError, ValueError) as exc:
        raise IntakeError(f"evidence file unavailable or outside repository: {relative}") from exc


def _load_object(data: bytes, name: str) -> dict[str, Any]:
    try:
        parsed = json.loads(data)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise IntakeError(f"invalid JSON: {name}") from exc
    if not isinstance(parsed, dict):
        raise IntakeError(f"expected JSON object: {name}")
    return parsed


def _row_intake(row: dict[str, Any]) -> dict[str, Any]:
    trial_id = row.get("trial_id")
    if row.get("classification") == "provider_boundary_timeout":
        disposition = "route_failure"
        eligible = False
        reason = "Provider-boundary timeout is observed; underlying cause and task result are unresolved."
    elif row.get("classification") == "candidate_or_verifier_setup_failure":
        disposition = "fixture_or_evaluator_issue"
        eligible = False
        reason = "Verifier setup ended before contract tests ran; this is not a completed verifier failure."
    elif row.get("codegg_defect_reproduced") is True:
        disposition = "reproduced_harness_defect"
        eligible = True
        reason = "S001 records an independently reproduced CodeGG defect."
    elif row.get("observed_outcome", {}).get("task_outcome") == "failed":
        disposition = "observed_failure"
        eligible = False
        reason = "Task or horizon criteria failed without an independently reproduced CodeGG cause."
    else:
        disposition = "unattributed"
        eligible = False
        reason = "Available receipt evidence does not establish a reproducible owner-specific defect."
    pointers = [f"{row.get('receipt_path')}#/{field}" for field in (
        "task_outcome", "horizon_outcome", "independent_verifier", "event_capture"
    )]
    return {
        "evidence_id": trial_id,
        "receipt_sha256": row.get("receipt_sha256"),
        "disposition": disposition,
        "campaign_eligible": eligible,
        "symptom": row.get("symptom"),
        "suspected_owner": row.get("suspected_owner", "unknown"),
        "cause_assertion": row.get("cause_assertion"),
        "evidence_pointers": pointers,
        "reason": reason,
    }


def build_dossier(repo_root: Path, report_path: Path = DEFAULT_REPORT) -> dict[str, Any]:
    root = repo_root.resolve(strict=True)
    report_rel = report_path.as_posix()
    report = _load_object(_repo_read(root, report_rel), report_rel)
    manifest_rel = h007_attribution.DEFAULT_MANIFEST.as_posix()
    manifest_raw = _repo_read(root, manifest_rel)
    manifest_sha = _sha256(manifest_raw)
    if report.get("report_schema") != h007_attribution.SCHEMA:
        raise IntakeError("unsupported S001 evidence report")
    if report.get("input_manifest_sha256") != manifest_sha:
        raise IntakeError("S001 report is stale for the current frozen input manifest")
    if not isinstance(report.get("baseline_source_sha"), str):
        raise IntakeError("S001 baseline source identity is missing")
    rows = report.get("trials")
    if not isinstance(rows, list) or [row.get("trial_id") for row in rows if isinstance(row, dict)] != list(
        h007_attribution.EXPECTED_TRIALS
    ):
        raise IntakeError("S001 evidence rows are incomplete or duplicated")
    if len(rows) != len(h007_attribution.EXPECTED_TRIALS):
        raise IntakeError("S001 evidence rows are malformed")

    historical = _repo_read(root, HISTORICAL_C003.as_posix())
    identity = {
        "baseline_source_sha": report["baseline_source_sha"],
        "case_identity": report.get("case_identity"),
        "input_manifest_sha256": manifest_sha,
        "report_sha256": _sha256(_repo_read(root, report_rel)),
    }
    stable = json.dumps(identity, sort_keys=True, separators=(",", ":")).encode()
    items = [_row_intake(row) for row in rows]
    reproducible = [item for item in items if item["disposition"] == "reproduced_harness_defect"]
    global_disposition = "eligible_defects_present" if reproducible else "no_reproducible_defect"
    return {
        "schema_version": 1,
        "schema": SCHEMA,
        "dossier_id": f"h007-intake-{_sha256(stable)[:16]}",
        "evidence_identity": identity,
        "disposition": global_disposition,
        "campaign_eligible": bool(reproducible),
        "current_red_reproduction": None,
        "current_red_reproduction_reason": (
            "S001 reproduced no current CodeGG defect; no failing-first native ACP red test exists."
        ),
        "historical_reference": {
            "class": "historical_fixture",
            "source": HISTORICAL_C003.as_posix(),
            "sha256": _sha256(historical),
            "status": "historical accepted correction; not current eligibility evidence",
        },
        "candidate_identity_requirements": [
            "baseline source SHA", "binary SHA-256", "case and hidden-verifier digests",
            "route", "policy", "independent red-test and holdout digests",
        ],
        "candidate_allow_prefixes": [],
        "protected_controls": [
            "H007 provider timeouts remain route failures",
            "H007 verifier setup errors remain unmeasured",
            "task failure alone does not establish a CodeGG defect",
            "candidate cannot change verifier, holdout, controller, budget, or evidence",
        ],
        "alternatives_and_falsifiers": [
            "Provider timeout: reproduce at provider boundary with known transport outcome before assigning a CodeGG owner.",
            "Fixture/verifier setup error: validate the fixture independently before scoring candidate behavior.",
            "Task or horizon miss: require a failing-first native CodeGG behavior and an independent expected result.",
        ],
        "evidence_completeness": {
            "sanitized_receipts": len(items),
            "companion_ledgers_used": False,
            "complete_event_trajectory_available": False,
            "unknown_is_not_false": True,
        },
        "items": items,
        "next_action": (
            "Do not propose or launch a provider-backed campaign. Keep S003 at "
            "no_reproducible_defect until a distinct current CodeGG red reproduction is supplied."
        ),
    }


def build_generic_dossier(repo_root: Path, manifest: dict[str, Any]) -> dict[str, Any]:
    """Validate a positive generic reproduction without promoting test data.

    The first generic intake accepts only explicit synthetic fixtures. This
    exercises stable identity, source ownership, independent verification and
    repeated-red requirements while making campaign eligibility impossible.
    A current CodeGG defect needs a separately reviewed schema/authority path.
    """
    root = repo_root.resolve(strict=True)
    required = {
        "schema", "evidence_kind", "defect_id", "owner", "source_revision",
        "source_path", "source_sha256", "test_path", "test_result_path", "verifier_path",
        "holdout_path", "candidate_scope", "failure_signature", "reproductions",
    }
    if not isinstance(manifest, dict) or set(manifest) != required:
        raise IntakeError("generic reproduction manifest has missing or unknown fields")
    if manifest.get("schema") != "defect_reproduction_manifest_v1":
        raise IntakeError("unsupported generic reproduction manifest")
    if manifest.get("evidence_kind") != "synthetic_fixture":
        raise IntakeError("this generic intake path accepts synthetic fixtures only")
    if manifest.get("owner") != "codegg_harness":
        raise IntakeError("generic reproduction owner must be codegg_harness")
    defect_id = manifest.get("defect_id")
    signature = manifest.get("failure_signature")
    revision = manifest.get("source_revision")
    if not isinstance(defect_id, str) or not defect_id or len(defect_id) > 128:
        raise IntakeError("generic defect id is invalid")
    if not isinstance(signature, str) or not signature or len(signature) > 256:
        raise IntakeError("generic failure signature is invalid")
    if not isinstance(revision, str) or len(revision) != 40 or any(c not in "0123456789abcdef" for c in revision):
        raise IntakeError("generic source revision must be a full lowercase Git SHA")

    artifact_fields = ("source_path", "test_path", "test_result_path", "verifier_path", "holdout_path")
    artifact_hashes: dict[str, str] = {}
    for field in artifact_fields:
        path = manifest.get(field)
        if not isinstance(path, str):
            raise IntakeError(f"generic {field} must be a repository-relative path")
        artifact_hashes[field] = _sha256(_repo_read(root, path))
    if manifest.get("source_sha256") != artifact_hashes["source_path"]:
        raise IntakeError("generic source digest does not match the pinned source artifact")
    if len(set(artifact_hashes.values())) < len(artifact_hashes):
        raise IntakeError("generic source, test, result, verifier and holdout artifacts must be distinct")
    test_result = _load_object(_repo_read(root, manifest["test_result_path"]), manifest["test_result_path"])
    verifier = _load_object(_repo_read(root, manifest["verifier_path"]), manifest["verifier_path"])
    holdout = _load_object(_repo_read(root, manifest["holdout_path"]), manifest["holdout_path"])
    if test_result != {
        "schema": "red_test_result_v1", "test_sha256": artifact_hashes["test_path"],
        "failure_signature": signature, "terminal": "red",
    }:
        raise IntakeError("generic red-test receipt does not bind the test and failure signature")
    if verifier != {
        "schema": "independent_verifier_result_v1", "verifier_owner": "independent",
        "source_sha256": artifact_hashes["source_path"], "test_sha256": artifact_hashes["test_path"],
        "holdout_sha256": artifact_hashes["holdout_path"], "verdict": "red_confirmed",
    }:
        raise IntakeError("generic independent verifier receipt is invalid")
    if holdout != {"schema": "protected_holdout_v1", "mutable_by_candidate": False}:
        raise IntakeError("generic holdout is not explicitly protected")

    scope = manifest.get("candidate_scope")
    if not isinstance(scope, list) or not scope or any(not isinstance(item, str) for item in scope):
        raise IntakeError("generic candidate scope must be a non-empty list")
    forbidden = ("dev/harness_lab/", "plans/closure/", "plans/implementation/", "plans/registry.md")
    normalized_scope = sorted(set(scope))
    for item in normalized_scope:
        parts = PurePosixPath(item).parts
        if not parts or "/".join(parts) + "/" != item or item.startswith("/") or ".." in parts \
                or item.startswith(forbidden):
            raise IntakeError("generic candidate scope is not a permitted normalized source prefix")

    reproductions = manifest.get("reproductions")
    if not isinstance(reproductions, list) or len(reproductions) < 2:
        raise IntakeError("generic intake requires at least two independent red reproductions")
    lanes: set[str] = set()
    rows = []
    for row in reproductions:
        if not isinstance(row, dict) or set(row) != {"lane_id", "failure_signature", "terminal"}:
            raise IntakeError("generic reproduction row is malformed")
        lane_id = row.get("lane_id")
        if not isinstance(lane_id, str) or not lane_id or lane_id in lanes:
            raise IntakeError("generic reproduction lane ids must be distinct")
        lanes.add(lane_id)
        if row.get("failure_signature") != signature or row.get("terminal") != "red":
            raise IntakeError("generic reproduction rows do not share the independently checked red result")
        rows.append({"lane_id": lane_id, "failure_signature": signature, "terminal": "red"})

    identity = {
        "schema": manifest["schema"], "evidence_kind": manifest["evidence_kind"],
        "defect_id": defect_id, "owner": manifest["owner"], "source_revision": revision,
        "artifact_sha256": artifact_hashes, "candidate_scope": normalized_scope,
        "failure_signature": signature, "reproductions": rows,
    }
    canonical = json.dumps(identity, sort_keys=True, separators=(",", ":")).encode()
    return {
        "schema_version": 1,
        "schema": "generic_defect_dossier_v1",
        "dossier_id": f"synthetic-intake-{_sha256(canonical)[:16]}",
        "evidence_kind": "synthetic_fixture",
        "disposition": "synthetic_reproduction_validated",
        "reproduction_validated": True,
        "campaign_eligible": False,
        "stable_identity_sha256": _sha256(canonical),
        "source_revision": revision,
        "artifact_sha256": artifact_hashes,
        "candidate_scope": normalized_scope,
        "reproductions": rows,
        "promotion_reason": "Synthetic intake mechanics are positive; synthetic evidence cannot establish a current CodeGG defect.",
    }
def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    parser.add_argument("--generic-manifest", type=Path,
                        help="validate an explicitly synthetic defect reproduction manifest")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.generic_manifest is None:
            dossier = build_dossier(args.repo_root, args.report)
        else:
            manifest_path = args.generic_manifest.as_posix()
            manifest = _load_object(_repo_read(args.repo_root.resolve(strict=True), manifest_path),
                                   manifest_path)
            dossier = build_generic_dossier(args.repo_root, manifest)
    except (IntakeError, h007_attribution.AttributionInputError, OSError) as exc:
        print(f"defect intake refused: {exc}", file=sys.stderr)
        return 2
    rendered = json.dumps(dossier, sort_keys=True, indent=2) + "\n"
    if args.output is None:
        sys.stdout.write(rendered)
    else:
        output = args.output if args.output.is_absolute() else args.repo_root / args.output
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(rendered, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

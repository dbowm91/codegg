#!/usr/bin/env python3
"""
Aggregate LSP compatibility manifests from five server jobs.

Each server job uploads a uniquely-named artifact directory
containing a per-server manifest (server-manifest.json) and
the compatibility report JSON. This script:

1. Recursively finds all server-manifest.json files
2. Validates exactly five manifests exist with expected labels
3. Validates commit SHA and workflow_run_id match across all
4. Verifies each referenced report exists and metadata agrees
5. Writes one aggregate root manifest

Exit codes:
  0 — success
  1 — validation error (MissingManifest, DuplicateLabel, CommitMismatch,
         RunIdMismatch, ReportMissing, ReportMetadataDisagrees,
         AggregateValidationError)
"""

import argparse
import json
import os
import sys
from pathlib import Path
from typing import Dict, List, Optional, Tuple

EXPECTED_SERVERS = [
    "rust-analyzer",
    "basedpyright",
    "gopls",
    "typescript-language-server",
    "clangd",
]


def find_server_manifests(input_dir: Path) -> List[Path]:
    """Recursively find all server-manifest.json files."""
    manifests = list(input_dir.rglob("server-manifest.json"))
    return sorted(manifests)


def load_manifest(path: Path) -> Dict:
    """Load and parse a server manifest JSON file."""
    with open(path, "r") as f:
        return json.load(f)


def validate_manifests(
    manifests: List[Tuple[Path, Dict]],
    expected_commit: Optional[str],
    expected_run_id: Optional[str],
) -> Tuple[Dict[str, Dict], List[str]]:
    """
    Validate all manifests and return (servers_dict, errors).

    servers_dict maps server_label -> manifest dict.
    errors is a list of validation error messages.
    """
    errors: List[str] = []
    servers: Dict[str, Dict] = {}
    seen_labels: set = set()

    for path, manifest in manifests:
        label = manifest.get("server_label", "")

        # Check for duplicate labels
        if label in seen_labels:
            errors.append(f"Duplicate server label '{label}' in {path}")
        seen_labels.add(label)

        # Check required fields
        if not label:
            errors.append(f"Missing server_label in {path}")
        if not manifest.get("commit"):
            errors.append(f"Missing commit in {path}")
        if not manifest.get("server_id"):
            errors.append(f"Missing server_id in {path}")
        if not manifest.get("report_path"):
            errors.append(f"Missing report_path in {path}")

        # Validate commit matches
        if expected_commit and manifest.get("commit") != expected_commit:
            errors.append(
                f"Commit mismatch in {path}: "
                f"expected {expected_commit}, got {manifest.get('commit')}"
            )

        # Validate workflow_run_id matches
        if expected_run_id and manifest.get("workflow_run_id") != expected_run_id:
            errors.append(
                f"workflow_run_id mismatch in {path}: "
                f"expected {expected_run_id}, got {manifest.get('workflow_run_id')}"
            )

        servers[label] = manifest

    return servers, errors


def validate_reports(
    input_dir: Path,
    servers: Dict[str, Dict],
) -> List[str]:
    """
    Verify each referenced report exists and its server_id/version
    match the manifest metadata.
    """
    errors: List[str] = []

    for label, manifest in servers.items():
        report_path_str = manifest.get("report_path", "")
        if not report_path_str:
            continue

        # The report_path is relative to the input directory root
        # (where all artifacts are downloaded)
        report_path = input_dir / report_path_str

        if not report_path.exists():
            errors.append(
                f"Report missing for '{label}': {report_path} "
                f"(referenced from {manifest.get('server_label')}/server-manifest.json)"
            )
            continue

        # Load and validate report metadata
        try:
            with open(report_path, "r") as f:
                report = json.load(f)
        except json.JSONDecodeError as e:
            errors.append(f"Invalid JSON in report for '{label}': {e}")
            continue

        report_server_id = report.get("server_id", "")
        manifest_server_id = manifest.get("server_id", "")

        if report_server_id != manifest_server_id:
            errors.append(
                f"Server ID mismatch for '{label}': "
                f"report has '{report_server_id}', manifest has '{manifest_server_id}'"
            )

        report_version = report.get("server_version")
        manifest_version = manifest.get("server_version")

        if report_version != manifest_version:
            errors.append(
                f"Version mismatch for '{label}': "
                f"report has {report_version}, manifest has {manifest_version}"
            )

        # Validate operation_support is non-empty
        operation_support = report.get("operation_support", [])
        if not operation_support:
            errors.append(f"Report for '{label}' has empty operation_support")

        # Validate shutdown_trace is present
        if "shutdown_trace" not in report:
            errors.append(f"Report for '{label}' missing shutdown_trace")

    return errors


def compute_summary_counts(
    input_dir: Path,
    servers: Dict[str, Dict],
) -> Dict:
    """Compute aggregate counts from all reports.

    Known limitations are classified by scope using a prefix convention:
    - "Protocol:" prefix — protocol-level limitations (shutdown hang, force-kill)
    - "Semantic:" prefix or no prefix — semantic-level limitations
    """
    required_ops = 0
    required_passing = 0
    known_limitations = 0
    protocol_known_limitations = 0
    semantic_known_limitations = 0
    protocol_failures = 0
    semantic_failures = 0

    for label, manifest in servers.items():
        report_path_str = manifest.get("report_path", "")
        if not report_path_str:
            continue

        report_path = input_dir / report_path_str
        if not report_path.exists():
            continue

        try:
            with open(report_path, "r") as f:
                report = json.load(f)
        except (json.JSONDecodeError, FileNotFoundError):
            continue

        for record in report.get("operation_support", []):
            req = record.get("requirement", "")
            exercised = record.get("exercised", False)
            request_succeeded = record.get("request_succeeded", False)
            response_parsed = record.get("response_parsed", False)
            semantic_assertion_passed = record.get("semantic_assertion_passed", False)
            known_limit = record.get("known_limit")

            if req == "Required":
                required_ops += 1
                if exercised and request_succeeded and response_parsed and semantic_assertion_passed:
                    required_passing += 1
                else:
                    if not request_succeeded or not response_parsed:
                        protocol_failures += 1
                    elif not semantic_assertion_passed:
                        semantic_failures += 1
            elif req == "KnownLimitation":
                if exercised:
                    known_limitations += 1
                    if known_limit and known_limit.startswith("Protocol:"):
                        protocol_known_limitations += 1
                    else:
                        semantic_known_limitations += 1

    return {
        "required_operations": required_ops,
        "required_operations_passing": required_passing,
        "known_limitations": known_limitations,
        "protocol_known_limitations": protocol_known_limitations,
        "semantic_known_limitations": semantic_known_limitations,
        "protocol_failures": protocol_failures,
        "semantic_failures": semantic_failures,
    }


def write_aggregate_manifest(
    output_path: Path,
    servers: Dict[str, Dict],
    expected_commit: Optional[str],
    expected_run_id: Optional[str],
    summary_counts: Dict,
) -> None:
    """Write the aggregate matrix manifest."""
    aggregate = {
        "commit": expected_commit or "unknown",
        "workflow_run_id": expected_run_id or "unknown",
        "complete": True,
        "expected_servers": EXPECTED_SERVERS,
        "servers": servers,
        **summary_counts,
    }

    output_path.parent.mkdir(parents=True, exist_ok=True)
    with open(output_path, "w") as f:
        json.dump(aggregate, f, indent=2)

    print(f"Wrote aggregate manifest to {output_path}")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Aggregate LSP compatibility manifests from five server jobs."
    )
    parser.add_argument(
        "--input",
        type=Path,
        required=True,
        help="Directory containing downloaded artifacts",
    )
    parser.add_argument(
        "--output",
        type=Path,
        required=True,
        help="Output path for the aggregate manifest",
    )
    parser.add_argument(
        "--expected-commit",
        type=str,
        default=None,
        help="Expected git commit SHA (from GITHUB_SHA)",
    )
    parser.add_argument(
        "--expected-run-id",
        type=str,
        default=None,
        help="Expected workflow run ID (from GITHUB_RUN_ID)",
    )
    args = parser.parse_args()

    # Find all server manifests
    manifests = find_server_manifests(args.input)

    if len(manifests) == 0:
        print("ERROR: No server-manifest.json files found", file=sys.stderr)
        return 1

    if len(manifests) != 5:
        print(
            f"ERROR: Expected exactly 5 server manifests, found {len(manifests)}",
            file=sys.stderr,
        )
        for m in manifests:
            print(f"  - {m}", file=sys.stderr)
        return 1

    # Load all manifests
    loaded = []
    for path in manifests:
        try:
            manifest = load_manifest(path)
            loaded.append((path, manifest))
        except (json.JSONDecodeError, FileNotFoundError) as e:
            print(f"ERROR: Failed to load manifest {path}: {e}", file=sys.stderr)
            return 1

    # Validate manifests
    servers, errors = validate_manifests(
        loaded, args.expected_commit, args.expected_run_id
    )
    if errors:
        for e in errors:
            print(f"ERROR: {e}", file=sys.stderr)
        return 1

    # Check expected server labels are present
    missing_labels = set(EXPECTED_SERVERS) - set(servers.keys())
    if missing_labels:
        print(
            f"ERROR: Missing expected server labels: {sorted(missing_labels)}",
            file=sys.stderr,
        )
        return 1

    extra_labels = set(servers.keys()) - set(EXPECTED_SERVERS)
    if extra_labels:
        print(
            f"ERROR: Unexpected server labels found: {sorted(extra_labels)}",
            file=sys.stderr,
        )
        return 1

    # Validate reports exist and metadata agrees
    report_errors = validate_reports(args.input, servers)
    if report_errors:
        for e in report_errors:
            print(f"ERROR: {e}", file=sys.stderr)
        return 1

    # Compute summary counts
    summary_counts = compute_summary_counts(args.input, servers)

    # Write aggregate manifest
    write_aggregate_manifest(
        args.output,
        servers,
        args.expected_commit,
        args.expected_run_id,
        summary_counts,
    )

    print(f"Successfully aggregated {len(servers)} server manifests")
    return 0


if __name__ == "__main__":
    sys.exit(main())
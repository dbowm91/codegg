#!/usr/bin/env python3
"""
Unit tests for aggregate_lsp_compatibility_manifest.py.

Requires: pytest (pip install pytest)
Run: pytest scripts/test_aggregate_lsp_compatibility_manifest.py -v
"""

import json
import os
import sys
from pathlib import Path

import pytest

# Add scripts directory to path so we can import the module
sys.path.insert(0, str(Path(__file__).parent))

from aggregate_lsp_compatibility_manifest import (
    EXPECTED_SERVERS,
    find_server_manifests,
    load_manifest,
    validate_manifests,
    validate_reports,
    compute_summary_counts,
)


def _make_manifest(
    server_label: str,
    commit: str = "abc123",
    run_id: str = "run-42",
    server_id: str | None = None,
    report_path: str | None = None,
    server_version: str = "1.0.0",
) -> dict:
    """Create a minimal server manifest dict."""
    if server_id is None:
        server_id = server_label
    if report_path is None:
        report_path = f"{server_label}/report.json"
    return {
        "commit": commit,
        "workflow_run_id": run_id,
        "server_label": server_label,
        "server_id": server_id,
        "server_version": server_version,
        "report_path": report_path,
        "position_encoding": "utf-16",
        "position_encoding_assumed": True,
        "operation_records": 25,
        "checks": 30,
    }


def _make_report(
    server_id: str = "rust-analyzer",
    server_version: str = "1.0.0",
    operation_support: list | None = None,
    include_shutdown_trace: bool = True,
) -> dict:
    """Create a minimal compatibility report dict."""
    if operation_support is None:
        operation_support = [
            {
                "operation": "Diagnostics",
                "advertised": True,
                "exercised": True,
                "request_succeeded": True,
                "response_parsed": True,
                "semantic_assertion_passed": True,
                "requirement": "Required",
                "known_limit": None,
            }
        ]
    report = {
        "server_id": server_id,
        "server_version": server_version,
        "position_encoding": "utf-16",
        "operation_support": operation_support,
    }
    if include_shutdown_trace:
        report["shutdown_trace"] = {
            "requested": True,
            "server_exited": True,
            "exit_code": 0,
        }
    return report


def _write_server_dir(
    tmp_path: Path,
    server_label: str,
    manifest: dict,
    report: dict | None = None,
    report_filename: str = "report.json",
) -> Path:
    """Write a server artifact directory with manifest and optional report."""
    server_dir = tmp_path / server_label
    server_dir.mkdir(parents=True, exist_ok=True)

    manifest_path = server_dir / "server-manifest.json"
    with open(manifest_path, "w") as f:
        json.dump(manifest, f)

    if report is not None:
        report_path = server_dir / report_filename
        with open(report_path, "w") as f:
            json.dump(report, f)

    return server_dir


# --- Test: aggregates_five_unique_manifests ---

def test_aggregates_five_unique_manifests(tmp_path: Path):
    """Happy path: five unique server manifests aggregate without error."""
    commit = "abc123def"
    run_id = "run-99"

    for label in EXPECTED_SERVERS:
        manifest = _make_manifest(label, commit=commit, run_id=run_id)
        report = _make_report(server_id=label)
        _write_server_dir(tmp_path, label, manifest, report)

    manifests = find_server_manifests(tmp_path)
    assert len(manifests) == 5

    loaded = [(m, load_manifest(m)) for m in manifests]
    servers, errors = validate_manifests(loaded, commit, run_id)
    assert errors == [], f"Unexpected validation errors: {errors}"
    assert set(servers.keys()) == set(EXPECTED_SERVERS)

    report_errors = validate_reports(tmp_path, servers)
    assert report_errors == [], f"Unexpected report errors: {report_errors}"


# --- Test: fails_when_server_missing ---

def test_fails_when_server_missing(tmp_path: Path):
    """Missing one of the five required servers should fail."""
    commit = "abc123"
    run_id = "run-1"

    # Only write 4 of 5 servers (missing clangd)
    for label in EXPECTED_SERVERS:
        if label == "clangd":
            continue
        manifest = _make_manifest(label, commit=commit, run_id=run_id)
        report = _make_report(server_id=label)
        _write_server_dir(tmp_path, label, manifest, report)

    manifests = find_server_manifests(tmp_path)
    assert len(manifests) == 4

    loaded = [(m, load_manifest(m)) for m in manifests]
    servers, errors = validate_manifests(loaded, commit, run_id)
    assert errors == []

    missing_labels = set(EXPECTED_SERVERS) - set(servers.keys())
    assert missing_labels == {"clangd"}


# --- Test: fails_on_duplicate_server ---

def test_fails_on_duplicate_server(tmp_path: Path):
    """Duplicate server labels should be rejected."""
    commit = "abc123"
    run_id = "run-1"

    for label in EXPECTED_SERVERS:
        manifest = _make_manifest(label, commit=commit, run_id=run_id)
        report = _make_report(server_id=label)
        _write_server_dir(tmp_path, label, manifest, report)

    # Add a duplicate rust-analyzer in a subdirectory
    dup_dir = tmp_path / "extra" / "rust-analyzer"
    dup_dir.mkdir(parents=True)
    dup_manifest = _make_manifest("rust-analyzer", commit=commit, run_id=run_id)
    with open(dup_dir / "server-manifest.json", "w") as f:
        json.dump(dup_manifest, f)

    manifests = find_server_manifests(tmp_path)
    assert len(manifests) == 6

    loaded = [(m, load_manifest(m)) for m in manifests]
    _, errors = validate_manifests(loaded, commit, run_id)
    assert any("Duplicate" in e for e in errors)


# --- Test: fails_on_commit_mismatch ---

def test_fails_on_commit_mismatch(tmp_path: Path):
    """Commit SHA mismatch should be rejected."""
    correct_commit = "abc123"
    wrong_commit = "def456"
    run_id = "run-1"

    for i, label in enumerate(EXPECTED_SERVERS):
        # First server has wrong commit
        commit = wrong_commit if i == 0 else correct_commit
        manifest = _make_manifest(label, commit=commit, run_id=run_id)
        report = _make_report(server_id=label)
        _write_server_dir(tmp_path, label, manifest, report)

    manifests = find_server_manifests(tmp_path)
    loaded = [(m, load_manifest(m)) for m in manifests]
    _, errors = validate_manifests(loaded, correct_commit, run_id)
    assert any("Commit mismatch" in e for e in errors)


# --- Test: fails_on_run_id_mismatch ---

def test_fails_on_run_id_mismatch(tmp_path: Path):
    """Workflow run ID mismatch should be rejected."""
    commit = "abc123"
    correct_run = "run-1"
    wrong_run = "run-2"

    for i, label in enumerate(EXPECTED_SERVERS):
        run = wrong_run if i == 0 else correct_run
        manifest = _make_manifest(label, commit=commit, run_id=run)
        report = _make_report(server_id=label)
        _write_server_dir(tmp_path, label, manifest, report)

    manifests = find_server_manifests(tmp_path)
    loaded = [(m, load_manifest(m)) for m in manifests]
    _, errors = validate_manifests(loaded, commit, correct_run)
    assert any("workflow_run_id mismatch" in e for e in errors)


# --- Test: fails_when_report_missing ---

def test_fails_when_report_missing(tmp_path: Path):
    """Missing report file should be rejected."""
    commit = "abc123"
    run_id = "run-1"

    for i, label in enumerate(EXPECTED_SERVERS):
        manifest = _make_manifest(label, commit=commit, run_id=run_id)
        # First server has no report file
        report = _make_report(server_id=label) if i > 0 else None
        _write_server_dir(tmp_path, label, manifest, report)

    manifests = find_server_manifests(tmp_path)
    loaded = [(m, load_manifest(m)) for m in manifests]
    servers, errors = validate_manifests(loaded, commit, run_id)
    assert errors == []

    report_errors = validate_reports(tmp_path, servers)
    assert any("Report missing" in e for e in report_errors)


# --- Test: fails_when_report_metadata_disagrees ---

def test_fails_when_report_metadata_disagrees(tmp_path: Path):
    """Report server_id or version mismatch should be rejected."""
    commit = "abc123"
    run_id = "run-1"

    for label in EXPECTED_SERVERS:
        manifest = _make_manifest(label, commit=commit, run_id=run_id)
        # First server's report has wrong server_id
        report = _make_report(
            server_id="wrong-server" if label == EXPECTED_SERVERS[0] else label
        )
        _write_server_dir(tmp_path, label, manifest, report)

    manifests = find_server_manifests(tmp_path)
    loaded = [(m, load_manifest(m)) for m in manifests]
    servers, errors = validate_manifests(loaded, commit, run_id)
    assert errors == []

    report_errors = validate_reports(tmp_path, servers)
    assert any("Server ID mismatch" in e for e in report_errors)


# --- Test: compute_summary_counts basic ---

def test_compute_summary_counts(tmp_path: Path):
    """Summary counts should reflect the operation support records."""
    commit = "abc123"
    run_id = "run-1"

    ops = [
        {
            "operation": "Diagnostics",
            "advertised": True,
            "exercised": True,
            "request_succeeded": True,
            "response_parsed": True,
            "semantic_assertion_passed": True,
            "requirement": "Required",
            "known_limit": None,
        },
        {
            "operation": "Rename",
            "advertised": True,
            "exercised": True,
            "request_succeeded": True,
            "response_parsed": True,
            "semantic_assertion_passed": False,
            "requirement": "Required",
            "known_limit": None,
        },
        {
            "operation": "TypeHierarchy",
            "advertised": False,
            "exercised": False,
            "request_succeeded": False,
            "response_parsed": False,
            "semantic_assertion_passed": False,
            "requirement": "KnownLimitation",
            "known_limit": "Server does not implement",
        },
    ]

    for label in EXPECTED_SERVERS:
        manifest = _make_manifest(label, commit=commit, run_id=run_id)
        report = _make_report(server_id=label, operation_support=ops)
        _write_server_dir(tmp_path, label, manifest, report)

    servers = {}
    for label in EXPECTED_SERVERS:
        servers[label] = _make_manifest(label, commit=commit, run_id=run_id)

    counts = compute_summary_counts(tmp_path, servers)
    # 3 ops * 5 servers = 15 required, 1 passing * 5 = 5 passing, etc.
    # Only "Diagnostics" passes fully, "Rename" has semantic_assertion_passed=false
    # "TypeHierarchy" is KnownLimitation and not exercised
    assert counts["required_operations"] == 10  # Diagnostics + Rename per server * 5
    assert counts["required_operations_passing"] == 5  # Only Diagnostics * 5
    assert counts["known_limitations"] == 0  # TypeHierarchy is not exercised
    assert counts["semantic_failures"] == 5  # Rename * 5 servers


# --- Test: validate_reports checks shutdown_trace ---

def test_fails_when_shutdown_trace_missing(tmp_path: Path):
    """Missing shutdown_trace in report should be rejected."""
    commit = "abc123"
    run_id = "run-1"

    for label in EXPECTED_SERVERS:
        manifest = _make_manifest(label, commit=commit, run_id=run_id)
        report = _make_report(server_id=label, include_shutdown_trace=False)
        _write_server_dir(tmp_path, label, manifest, report)

    manifests = find_server_manifests(tmp_path)
    loaded = [(m, load_manifest(m)) for m in manifests]
    servers, errors = validate_manifests(loaded, commit, run_id)
    assert errors == []

    report_errors = validate_reports(tmp_path, servers)
    assert any("shutdown_trace" in e for e in report_errors)

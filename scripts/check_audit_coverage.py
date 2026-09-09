#!/usr/bin/env python3
"""Static guard for audit instrumentation coverage (identity M005).

Ensures the Phase-11 required-event matrix stays executable: every
known audit action has a coverage row, every daemon operation is either
live-mapped or explicitly uninstrumented, live-mapped actions have at
least one operation mapping, instrumentation stays append-only with
trusted attribution, and the operator matrix doc exists.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
AUDIT_MODULE = REPO_ROOT / "crates" / "codegg-core" / "src" / "audit.rs"
INSTR_MODULE = REPO_ROOT / "crates" / "codegg-core" / "src" / "audit_instrumentation.rs"
AUTHZ_MODULE = REPO_ROOT / "crates" / "codegg-core" / "src" / "authorization.rs"
DAEMON_MODULE = REPO_ROOT / "src" / "core" / "daemon.rs"
AUDIT_DOC = REPO_ROOT / "architecture" / "audit.md"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _audit_actions() -> list[str]:
    source = _read(AUDIT_MODULE)
    match = re.search(r"pub const ALL:\s*\[&'static str;\s*\d+\]\s*=\s*\[(.*?)\];", source, re.DOTALL)
    if not match:
        print("  FAIL: AuditAction::ALL not found")
        return []
    return re.findall(r'"([^"]+)"', match.group(1))


def _coverage_actions() -> list[str]:
    source = _read(INSTR_MODULE)
    return re.findall(r'action:\s*"([^"]+)"', source)


def _instrumented_pairs() -> list[tuple[str, str]]:
    source = _read(INSTR_MODULE)
    match = re.search(
        r"pub const INSTRUMENTED_OPERATIONS[^=]*=\s*&\[(.*?)\];", source, re.DOTALL
    )
    if not match:
        print("  FAIL: INSTRUMENTED_OPERATIONS not found")
        return []
    # Avoid the doc-comment mention of UNINSTRUMENTED_OPERATIONS: slice
    # from the const definition only (match already starts there).
    return re.findall(r'\(\"([^\"]+)\",\s*\"([^\"]+)\"', match.group(1))


def _uninstrumented_ops() -> list[str]:
    source = _read(INSTR_MODULE)
    # Anchor on the const definition to avoid the doc-comment mention
    # inside the INSTRUMENTED block.
    match = re.search(
        r"pub const UNINSTRUMENTED_OPERATIONS[^=]*=\s*&\[(.*?)\];", source, re.DOTALL
    )
    if not match:
        print("  FAIL: UNINSTRUMENTED_OPERATIONS not found")
        return []
    return re.findall(r'"([^"]+)"', match.group(1))


def _authz_operations() -> set[str]:
    source = _read(AUTHZ_MODULE)
    ops = set(re.findall(r'OperationDescriptor::new\(\s*\"([^\"]+)\"', source))
    ops.discard("projection_scope")
    return ops


def check_matrix_covers_every_action() -> bool:
    known = _audit_actions()
    if not known:
        return False
    covered = _coverage_actions()
    missing = [action for action in known if action not in covered]
    if missing:
        print(f"  FAIL: coverage matrix missing actions: {', '.join(missing)}")
        return False
    if len(covered) != len(known):
        print(f"  FAIL: matrix has {len(covered)} rows for {len(known)} actions")
        return False
    return True


def check_every_operation_is_classified() -> bool:
    ops = _authz_operations()
    if not ops:
        return False
    pairs = _instrumented_pairs()
    instrumented = {op for op, _ in pairs}
    uninstrumented = set(_uninstrumented_ops())
    both = instrumented & uninstrumented
    if both:
        print(f"  FAIL: operations in both lists: {', '.join(sorted(both))}")
        return False
    missing = sorted(ops - instrumented - uninstrumented)
    if missing:
        print(f"  FAIL: unclassified daemon operations: {', '.join(missing)}")
        return False
    # Every mapped action must be a known audit action.
    known = set(_audit_actions())
    bad = sorted({action for _, action in pairs if action not in known})
    if bad:
        print(f"  FAIL: instrumented mappings to unknown actions: {', '.join(bad)}")
        return False
    return True


def check_live_mapped_actions_have_operation_mapping() -> bool:
    source = _read(INSTR_MODULE)
    # Split the coverage table into per-action blocks: each block starts
    # with `action: "..."` and contains a `live_mapped: true/false`.
    blocks = re.split(r"AuditCoverageEntry\s*\{", source)[1:]
    pairs = _instrumented_pairs()
    mapped_actions = {action for _, action in pairs}
    ok = True
    for block in blocks:
        action_match = re.search(r'action:\s*"([^"]+)"', block)
        live_match = re.search(r"live_mapped:\s*(true|false)", block)
        if not action_match or not live_match:
            continue
        action, live = action_match.group(1), live_match.group(1) == "true"
        if not live:
            continue
        # `authorization_decision` is live via the denial seam (every
        # denied operation), not via one operation-table row.
        if action == "authorization_decision":
            continue
        if action not in mapped_actions:
            print(f"  FAIL: live-mapped action has no operation mapping: {action}")
            ok = False
    return ok


def check_instrumentation_stays_append_only_and_trusted() -> bool:
    source = _read(INSTR_MODULE)
    if re.search(r"(?i)\b(UPDATE|DELETE)\s+.*audit_event\b", source):
        print("  FAIL: instrumentation must never UPDATE/DELETE audit_event rows")
        return False
    if re.search(r"(?i)DELETE\s+FROM\s+audit_event", source):
        print("  FAIL: instrumentation must never delete structural rows")
        return False
    for required in (
        "AuditDecisionProvenance",
        "AuthenticatedPrincipal",
        "deterministic_event_id",
        "AuditChainContext",
        "operation_to_audit_action",
    ):
        if required not in source:
            print(f"  FAIL: instrumentation missing trusted-attribution marker {required}")
            return False
    # Core boundary: the instrumentation module must not import the
    # authorization service (see check-core-boundary.sh). The daemon
    # seam owns the operation_descriptor -> operation_to_audit_action
    # step plus the audit_provenance bridge.
    if "crate::authorization" in source or "crate::auth" in source:
        print("  FAIL: instrumentation must not import crate::authorization (core boundary)")
        return False
    daemon = _read(DAEMON_MODULE)
    for required in (
        "emit_audit_for_authorized",
        "emit_audit_for_denial",
        "append_audit_event",
        "audit_chain_for_request",
    ):
        if required not in daemon:
            print(f"  FAIL: daemon seam missing {required}")
            return False
    return True


def check_audit_doc_has_matrix() -> bool:
    if not AUDIT_DOC.is_file():
        print("  FAIL: architecture/audit.md missing")
        return False
    source = _read(AUDIT_DOC)
    for required in (
        "coverage",
        "causation",
        "correlation",
        "redact",
        "backpressure",
        "audit.read",
    ):
        if required.lower() not in source.lower():
            print(f"  FAIL: audit doc missing {required}")
            return False
    return True


def main() -> int:
    verbose = "--verbose" in sys.argv or "-v" in sys.argv
    checks: list[tuple[str, object]] = [
        ("coverage matrix covers every audit action", check_matrix_covers_every_action),
        ("every daemon operation is classified", check_every_operation_is_classified),
        ("live-mapped actions have operation mappings", check_live_mapped_actions_have_operation_mapping),
        ("instrumentation stays append-only and trusted", check_instrumentation_stays_append_only_and_trusted),
        ("audit operator matrix doc exists", check_audit_doc_has_matrix),
    ]
    results: list[tuple[str, bool]] = []
    for name, check_fn in checks:
        if verbose:
            print(f"CHECK: {name} ... ", end="", flush=True)
        try:
            passed = bool(check_fn())  # type: ignore[operator]
        except Exception as exc:  # noqa: BLE001 - guard must fail loudly
            print(f"  FAIL: exception: {exc}")
            passed = False
        results.append((name, passed))
        if verbose:
            print("PASS" if passed else "FAIL")
    failed = [name for name, passed in results if not passed]
    if verbose:
        print()
        for name, passed in results:
            print(f"  [{'PASS' if passed else 'FAIL'}] {name}")
        print(f"\n{len(results) - len(failed)}/{len(results)} checks passed.")
    else:
        for name in failed:
            print(f"FAIL: {name}")
    if failed:
        print("Audit coverage invariants violated.")
        return 1
    print("All audit coverage invariants verified.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

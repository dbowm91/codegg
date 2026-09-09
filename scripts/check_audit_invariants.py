#!/usr/bin/env python3
"""Static guard for append-only audit foundation invariants (identity M004).

Checks that the coordinator-owned audit store stays append-only, that the
migration is additive and restart-safe, that metadata bounds and the secret
deny-list exist, that audit reads are authorized, and that the operator
documentation exists.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
AUDIT_MODULE = REPO_ROOT / "crates" / "codegg-core" / "src" / "audit.rs"
SCHEMA_MODULE = REPO_ROOT / "crates" / "codegg-core" / "src" / "session" / "schema.rs"
AUTHZ_MODULE = REPO_ROOT / "crates" / "codegg-core" / "src" / "authorization.rs"
DAEMON_MODULE = REPO_ROOT / "src" / "core" / "daemon.rs"
PROTOCOL_CORE = REPO_ROOT / "crates" / "codegg-protocol" / "src" / "core.rs"
AUDIT_DOC = REPO_ROOT / "architecture" / "audit.md"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def check_migration_additive() -> bool:
    """v54 creates audit_event/audit_body additively with sequence authority."""
    source = _read(SCHEMA_MODULE)
    if "migrate_v54" not in source:
        print("  FAIL: migrate_v54 not found")
        return False
    for required in (
        "CREATE TABLE IF NOT EXISTS audit_event",
        "CREATE TABLE IF NOT EXISTS audit_body",
        "seq INTEGER PRIMARY KEY AUTOINCREMENT",
        "event_id TEXT NOT NULL UNIQUE",
        "ON CONFLICT(event_id) DO NOTHING",
    ):
        if required not in source and required not in _read(AUDIT_MODULE):
            print(f"  FAIL: audit migration/store missing {required!r}")
            return False
    if "CREATE TABLE IF NOT EXISTS audit_event" not in source:
        print("  FAIL: audit_event table creation not found")
        return False
    if "CREATE TABLE IF NOT EXISTS audit_body" not in source:
        print("  FAIL: audit_body table creation not found")
        return False
    return True


def check_structural_rows_have_no_update_or_delete() -> bool:
    """Production code must never UPDATE or DELETE audit_event rows."""
    source = _read(AUDIT_MODULE)
    for statement in re.findall(r"(?i)\b(UPDATE|DELETE)\s+.*audit_event\b", source):
        print(f"  FAIL: audit_event has a mutating statement ({statement})")
        return False
    # Retention may delete from audit_body only; structural deletes are banned.
    if re.search(r"(?i)DELETE\s+FROM\s+audit_event", source):
        print("  FAIL: audit_event structural delete found")
        return False
    if "DELETE FROM audit_body" not in source:
        print("  FAIL: retention expiry must delete only from audit_body")
        return False
    return True


def check_bounded_metadata_and_redaction() -> bool:
    """Bounded metadata constants and the secret deny-list must exist."""
    source = _read(AUDIT_MODULE)
    for required in (
        "MAX_METADATA_ENTRIES",
        "MAX_METADATA_VALUE_LENGTH",
        "MAX_METADATA_TOTAL_BYTES",
        "MAX_BODY_BYTES",
        "MAX_QUERY_LIMIT",
        "MAX_EXPORT_EVENTS",
        "SECRET_KEY_SUBSTRINGS",
        "SECRET_VALUE_SUBSTRINGS",
        "SecretDetected",
    ):
        if required not in source:
            print(f"  FAIL: audit module missing {required}")
            return False
    return True


def check_query_is_authorized() -> bool:
    """Audit reads must require audit.read at the daemon gate."""
    authz = _read(AUTHZ_MODULE)
    for operation in ('"audit_query"', '"audit_export"'):
        if operation not in authz:
            print(f"  FAIL: authorization matrix missing {operation}")
            return False
    if "Capability::AuditRead" not in _read(AUDIT_MODULE) and "audit.read" not in authz:
        print("  FAIL: authorization matrix missing audit.read")
        return False
    if '"audit_query"' not in authz or "AuditRead" not in authz:
        print("  FAIL: authorization matrix missing audit.read grant")
        return False
    daemon = _read(DAEMON_MODULE)
    for variant in ("AuditQuery", "AuditExport", "AuditCapabilities"):
        if f"CoreRequest::{variant}" not in daemon:
            print(f"  FAIL: daemon dispatch missing CoreRequest::{variant}")
            return False
    if "authorize_request" not in daemon:
        print("  FAIL: daemon dispatch does not consult the authorization service")
        return False
    protocol = _read(PROTOCOL_CORE)
    for dto in ("AuditQueryRequestDto", "AuditExportRequestDto", "AuditEventDto"):
        if dto not in protocol:
            print(f"  FAIL: protocol missing {dto}")
            return False
    audit = _read(AUDIT_MODULE)
    if "AuditDecisionProvenance" not in audit or "AuthenticatedPrincipal" not in audit:
        print("  FAIL: audit builder must take transport-bound principal + provenance")
        return False
    if "audit_provenance" not in authz:
        print("  FAIL: authorization bridge audit_provenance not found")
        return False
    return True


def check_coordinator_sequence_and_idempotency() -> bool:
    """Sequence authority and idempotency wording must be pinned in code."""
    source = _read(AUDIT_MODULE)
    for required in ("AUTOINCREMENT", "ON CONFLICT(event_id) DO NOTHING", "max_seq"):
        if required not in source and required not in _read(SCHEMA_MODULE):
            print(f"  FAIL: sequence/idempotency marker missing: {required}")
            return False
    return True


def check_audit_doc_exists() -> bool:
    """Operator retention/failure documentation must exist."""
    if not AUDIT_DOC.is_file():
        print("  FAIL: architecture/audit.md missing")
        return False
    source = _read(AUDIT_DOC)
    for required in ("append-only", "retention", "backpressure", "audit.read", "redact"):
        if required.lower() not in source.lower():
            print(f"  FAIL: audit doc missing {required}")
            return False
    return True


def main() -> int:
    verbose = "--verbose" in sys.argv or "-v" in sys.argv
    checks: list[tuple[str, object]] = [
        ("audit migration is additive with sequence authority", check_migration_additive),
        ("structural rows have no UPDATE/DELETE path", check_structural_rows_have_no_update_or_delete),
        ("bounded metadata and secret redaction exist", check_bounded_metadata_and_redaction),
        ("audit reads are authorized end to end", check_query_is_authorized),
        ("coordinator sequence and idempotency pinned", check_coordinator_sequence_and_idempotency),
        ("audit operator documentation exists", check_audit_doc_exists),
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
        print("Audit foundation invariants violated.")
        return 1
    print("All audit foundation invariants verified.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

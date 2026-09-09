#!/usr/bin/env python3
"""Static guard for daemon authorization matrix invariants (identity M003).

Checks that every native ``CoreRequest`` variant is classified in the
centralized authorization service, that handlers do not hand-roll role
interpretation, and that the attribution migration stays additive.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
AUTHZ_MODULE = REPO_ROOT / "crates" / "codegg-core" / "src" / "authorization.rs"
PROTOCOL_CORE = REPO_ROOT / "crates" / "codegg-protocol" / "src" / "core.rs"
SCHEMA_MODULE = REPO_ROOT / "crates" / "codegg-core" / "src" / "session" / "schema.rs"
DAEMON_MODULE = REPO_ROOT / "src" / "core" / "daemon.rs"
AUTHZ_DOC = REPO_ROOT / "architecture" / "authorization.md"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _request_variants() -> set[str]:
    """Variant names of the native ``CoreRequest`` enum."""
    source = _read(PROTOCOL_CORE)
    match = re.search(r"pub enum CoreRequest \{(.*?)\n\}", source, re.DOTALL)
    if not match:
        print("  FAIL: CoreRequest enum not found")
        return set()
    body = match.group(1)
    return set(re.findall(r"^\s{4}(\w+)(?:\s*\{|\s*,|\s*$)", body, re.MULTILINE))


def check_matrix_covers_every_request() -> bool:
    """Every CoreRequest variant has a match arm in operation_descriptor."""
    variants = _request_variants()
    if not variants:
        return False
    source = _read(AUTHZ_MODULE)
    match = re.search(r"pub fn operation_descriptor\(.*?\{", source, re.DOTALL)
    if not match:
        print("  FAIL: operation_descriptor not found")
        return False
    # operation_descriptor ends where operation_capability_matrix begins.
    tail = source[match.end():]
    end = tail.find("pub fn operation_capability_matrix")
    body = tail[:end] if end != -1 else tail
    if re.search(r"\b_\s*=>", body):
        print("  FAIL: operation_descriptor must be exhaustive (no wildcard arm)")
        return False
    missing = sorted(v for v in variants if f"R::{v}" not in body)
    if missing:
        print(f"  FAIL: unclassified CoreRequest variants: {', '.join(missing)}")
        return False
    return True


def check_no_role_checks_in_daemon_dispatch() -> bool:
    """Daemon dispatch must ask for capabilities, not match on roles."""
    source = _read(DAEMON_MODULE)
    for pattern in ("ProjectRole::", "role_capabilities", "match role"):
        if pattern in source:
            print(f"  FAIL: daemon dispatch contains role interpretation ({pattern})")
            return False
    if "operation_descriptor" not in source or "authorize_request" not in source:
        print("  FAIL: daemon dispatch does not consult the authorization service")
        return False
    return True


def check_denials_carry_no_project_signal() -> bool:
    """denial_as_not_found must not embed project ids or secrets."""
    source = _read(AUTHZ_MODULE)
    match = re.search(r"pub fn denial_as_not_found\(\)(.*?)\n\}", source, re.DOTALL)
    if not match:
        print("  FAIL: denial_as_not_found not found")
        return False
    body = match.group(1)
    for forbidden in ("project_id", "principal", "secret", "token", "bearer"):
        if forbidden in body.lower():
            print(f"  FAIL: denial_as_not_found mentions {forbidden}")
            return False
    return True


def check_attribution_migration_additive() -> bool:
    """v53 creates origin_attribution additively with first-write-wins shape."""
    source = _read(SCHEMA_MODULE)
    if "migrate_v53" not in source:
        print("  FAIL: migrate_v53 not found")
        return False
    if "CREATE TABLE IF NOT EXISTS origin_attribution" not in source:
        print("  FAIL: origin_attribution table creation not found")
        return False
    if "PRIMARY KEY (scope_kind, scope_id)" not in source:
        print("  FAIL: origin_attribution immutability key not found")
        return False
    return True


def check_authorization_doc_exists() -> bool:
    """The operation-to-capability matrix doc must exist and name the service."""
    if not AUTHZ_DOC.is_file():
        print("  FAIL: architecture/authorization.md missing")
        return False
    source = _read(AUTHZ_DOC)
    for required in ("AuthorizationService", "LocalOwner", "visible_projects", "narrow"):
        if required not in source:
            print(f"  FAIL: authorization doc missing {required}")
            return False
    return True


def main() -> int:
    verbose = "--verbose" in sys.argv or "-v" in sys.argv
    checks: list[tuple[str, callable]] = [
        ("operation matrix covers every CoreRequest", check_matrix_covers_every_request),
        ("daemon dispatch asks capabilities, not roles", check_no_role_checks_in_daemon_dispatch),
        ("denials carry no project/existence signal", check_denials_carry_no_project_signal),
        ("attribution migration is additive", check_attribution_migration_additive),
        ("authorization architecture doc exists", check_authorization_doc_exists),
    ]

    results: list[tuple[str, bool]] = []
    for name, check_fn in checks:
        if verbose:
            print(f"CHECK: {name} ... ", end="", flush=True)
        try:
            passed = bool(check_fn())
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
        print("Authorization matrix invariants violated.")
        return 1
    print("All authorization matrix invariants verified.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

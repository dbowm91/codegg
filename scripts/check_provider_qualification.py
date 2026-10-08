#!/usr/bin/env python3
"""Guard the provider-connection qualification contract (M010).

Provisioning used to treat a successful `Provider::models()` call as proof
that a credential was accepted. That is false for every provider whose
`models()` returns a local/static array, for best-effort OpenAI-compatible
discovery that falls back instead of proving auth, and for provider
`/models` endpoints that are publicly readable.

This guard makes the corrected contract mechanically checkable instead of a
convention:

1. The catalog probe strategy must not carry a name that can be mistaken for
   credential verification (`DirectModels` meant "authenticate by calling
   models"). It must be a catalog-only strategy named as such.
2. Every durable `credential_status` value written into
   `provider_connection_health` must come from the typed
   `CredentialVerification` contract, never a bare string literal.
3. The persisted CHECK constraint must keep matching the Rust enum's codes,
   so storage cannot silently drift from the typed model.

Run directly (`python3 scripts/check_provider_qualification.py`) or with
`--self-test` to verify the guard itself still detects regressions.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

QUALIFICATION_RS = REPO_ROOT / "crates" / "codegg-providers" / "src" / "qualification.rs"
SETUP_CATALOG_RS = REPO_ROOT / "crates" / "codegg-providers" / "src" / "setup_catalog.rs"
SCHEMA_RS = REPO_ROOT / "crates" / "codegg-core" / "src" / "session" / "schema.rs"
EGGPOOL_RS = REPO_ROOT / "src" / "core" / "eggpool.rs"

RUST_SOURCE_ROOTS = [REPO_ROOT / "src", REPO_ROOT / "crates"]

# Strategy variants whose names assert credential semantics from catalog
# discovery. Reintroducing either silently restores the original defect.
FORBIDDEN_STRATEGY_TOKENS = ("SetupProbeStrategy::DirectModels", "SetupProbeStrategy::CompatibleProbe")

# The typed contract the durable column must agree with.
EXPECTED_CREDENTIAL_CODES = (
    "verified",
    "unverified",
    "authentication_failed",
    "no_credential_required",
)


def _fail(message: str) -> None:
    sys.stderr.write(f"provider qualification guard: {message}\n")


def _rust_sources() -> list[Path]:
    sources: list[Path] = []
    for root in RUST_SOURCE_ROOTS:
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*.rs")):
            if "target" in path.parts:
                continue
            sources.append(path)
    return sources


def check_no_credential_implying_strategy_names() -> bool:
    """Catalog discovery must not be named as if it authenticated anything."""
    offenders: list[str] = []
    for path in _rust_sources():
        text = path.read_text(encoding="utf-8")
        for token in FORBIDDEN_STRATEGY_TOKENS:
            if token in text:
                offenders.append(f"{path.relative_to(REPO_ROOT)}: {token}")
    if offenders:
        _fail(
            "catalog discovery must not carry a credential-implying strategy name "
            f"(use SetupProbeStrategy::ProviderCatalog / ::AuthenticatedCompatibleCatalog): "
            + "; ".join(offenders)
        )
        return False
    return True


def check_catalog_strategy_is_evidence_typed() -> bool:
    """The catalog strategy must declare it is not credential evidence."""
    text = SETUP_CATALOG_RS.read_text(encoding="utf-8")
    if "pub const fn credential_evidence(" not in text:
        _fail(
            "SetupProbeStrategy must expose credential_evidence() so catalog "
            "discovery cannot silently imply credential verification"
        )
        return False
    if "Self::ProviderCatalog => CredentialEvidence::CatalogOnly" not in text:
        _fail(
            "SetupProbeStrategy::ProviderCatalog must map to CredentialEvidence::CatalogOnly"
        )
        return False
    return True


def check_credential_writes_are_typed() -> bool:
    """No bare credential-verdict literals may reach provider_connection_health.

    Scans production write statements only. The test module is excluded
    deliberately: a test that *asserts* `credential_status = 'verified'` must be
    able to name the literal, while every production write must bind a typed
    `CredentialVerification::code()` value instead.
    """
    full = EGGPOOL_RS.read_text(encoding="utf-8")
    marker = "\nmod tests {"
    production = full.split(marker, 1)[0] if marker in full else full
    if marker in full:
        if "#[cfg(test)]" not in full[: full.index(marker)]:
            _fail("could not confirm the eggpool test module boundary; refusing to guess")
            return False
    offenders: list[str] = []
    for line in production.splitlines():
        if "provider_connection_health" not in line:
            continue
        if not any(keyword in line for keyword in ("UPDATE", "INSERT", "update", "insert")):
            continue
        for code in EXPECTED_CREDENTIAL_CODES:
            # A literal verdict embedded directly in the SQL statement.
            if f"'{code}'" in line:
                offenders.append(line.strip())
    if offenders:
        _fail(
            "provider_connection_health writes must bind a typed "
            "CredentialVerification::code() value, not a string literal: "
            + " | ".join(offenders)
        )
        return False
    if "credential_status" in production and "CredentialVerification::" not in production:
        _fail(
            "src/core/eggpool.rs sets credential_status without going through the "
            "CredentialVerification contract"
        )
        return False
    return True


def check_storage_contract_matches_type() -> bool:
    """The persisted CHECK constraint must mirror the typed enum's codes."""
    schema = SCHEMA_RS.read_text(encoding="utf-8")
    # The migration statement is a Rust string literal split across lines with
    # trailing backslashes; rejoin it so the CHECK list reads as one clause.
    flattened = re.sub(r"\\\s*\n\s*", " ", schema)
    match = re.search(
        r"credential_status\s+TEXT.*?CHECK\s*\(([^)]*)\)", flattened, re.DOTALL
    )
    if match is None:
        _fail(
            "could not locate the provider_connection_health.credential_status "
            "CHECK constraint in schema.rs"
        )
        return False
    constraint = match.group(1)
    missing = [code for code in EXPECTED_CREDENTIAL_CODES if f"'{code}'" not in constraint]
    if missing:
        _fail(
            "provider_connection_health.credential_status CHECK constraint is missing "
            f"typed code(s): {', '.join(missing)} (found: {constraint.strip()})"
        )
        return False

    qualification = QUALIFICATION_RS.read_text(encoding="utf-8")
    missing_codes = [
        code for code in EXPECTED_CREDENTIAL_CODES if f'"{code}"' not in qualification
    ]
    if missing_codes:
        _fail(
            "CredentialVerification no longer produces stored code(s): "
            f"{', '.join(missing_codes)}"
        )
        return False
    return True


CHECKS = (
    ("no credential-implying probe strategy names", check_no_credential_implying_strategy_names),
    ("catalog strategy declares its evidence type", check_catalog_strategy_is_evidence_typed),
    ("credential health writes are typed", check_credential_writes_are_typed),
    ("storage contract matches the typed model", check_storage_contract_matches_type),
)


def _self_test() -> int:
    """Verify the guard still fails on the pre-M010 shape."""
    failures = 0
    for token in FORBIDDEN_STRATEGY_TOKENS:
        synthetic = f"match x {{ SetupProbeStrategy::{token.split('::')[-1]} => todo() }}"
        detected = any(candidate in synthetic for candidate in FORBIDDEN_STRATEGY_TOKENS)
        if not detected:
            _fail(f"self-test: guard does not detect {token}")
            failures += 1
    synthetic_sql = "UPDATE provider_connection_health SET credential_status = 'verified'"
    detected = any(f"'{code}'" in synthetic_sql for code in EXPECTED_CREDENTIAL_CODES)
    if not detected:
        _fail("self-test: guard does not detect a literal credential verdict in SQL")
        failures += 1
    print("self-test: ok" if not failures else f"self-test: {failures} check(s) FAILED")
    return 1 if failures else 0


def main() -> int:
    if "--self-test" in sys.argv:
        return _self_test()
    verbose = "--verbose" in sys.argv or "-v" in sys.argv
    passed = 0
    failed = 0
    for label, check in CHECKS:
        ok = check()
        if verbose or not ok:
            print(f"  [{'PASS' if ok else 'FAIL'}] {label}")
        passed += int(ok)
        failed += int(not ok)
    print(f"{passed}/{len(CHECKS)} provider qualification checks passed.")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
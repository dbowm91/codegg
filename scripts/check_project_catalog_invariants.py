#!/usr/bin/env python3
"""Static guard for project catalog invariants.

Checks that the catalog module in codegg-core enforces safety invariants
for remote locators, migration schema, and module exports.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CATALOG_MODULE = REPO_ROOT / "crates" / "codegg-core" / "src" / "project_catalog.rs"
SCHEMA_MODULE = REPO_ROOT / "crates" / "codegg-core" / "src" / "session" / "schema.rs"
STORAGE_MODULE = REPO_ROOT / "crates" / "codegg-core" / "src" / "storage" / "mod.rs"
LIB_MODULE = REPO_ROOT / "crates" / "codegg-core" / "src" / "lib.rs"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def check_catalog_module_exists() -> bool:
    return CATALOG_MODULE.is_file()


def check_attach_locator_local_only() -> bool:
    """Verify attach_locator only does workspace binding for Local, not Ssh/LinkedNode."""
    source = _read(CATALOG_MODULE)

    # Find the attach_locator function body
    match = re.search(r"pub async fn attach_locator\(", source)
    if not match:
        print("  FAIL: attach_locator function not found")
        return False

    # Find the function body (scan forward to find closing brace at depth 0)
    start = match.start()
    depth = 0
    found_open = False
    func_body = ""
    for ch in source[start:]:
        func_body += ch
        if ch == "{":
            depth += 1
            found_open = True
        elif ch == "}":
            depth -= 1
            if found_open and depth == 0:
                break

    # Check that the Local arm validates workspace binding
    if "if let Locator::Local" not in func_body:
        print("  FAIL: attach_locator missing if let Locator::Local arm")
        return False
    if "workspace_project_binding" not in func_body:
        print("  FAIL: attach_locator Local arm does not validate workspace binding")
        return False

    # For Ssh and LinkedNode, there should be no .canonical_root or .as_path() access
    # within match arms that are NOT inside the Local branch.
    # We check that Ssh/LinkedNode arms in the storage tuple extract None for canonical_root.
    # Find the match on &locator that builds the storage tuple
    storage_match = re.search(r"let \(.*?\) = match &locator \{", func_body, re.DOTALL)
    if not storage_match:
        print("  FAIL: could not find storage tuple match in attach_locator")
        return False

    # Extract from the match to the end of attach_locator
    match_body = func_body[storage_match.start():]

    # Find the Ssh arm and verify it returns None for canonical_root
    ssh_arm_match = re.search(r"Locator::Ssh\s*\{.*?\}\s*=>\s*\((.*?)\)", match_body, re.DOTALL)
    if ssh_arm_match:
        ssh_arm_body = ssh_arm_match.group(1)
        # canonical_root should be None in the Ssh arm tuple
        # The tuple has: kind, ws_id, canonical_root, ssh_host, ...
        # canonical_root is the 3rd element (index 2)
        if "Some(" in ssh_arm_body.split(",")[2] if len(ssh_arm_body.split(",")) > 2 else True:
            # More reliable: check that None appears in the ssh arm before ssh_host
            if "canonical_root" in ssh_arm_body or "as_path" in ssh_arm_body:
                print("  FAIL: Ssh arm accesses canonical_root or as_path")
                return False

    # Check that Locator::Ssh and Locator::LinkedNode do not have methods returning &Path or PathBuf
    for variant in ["Ssh", "LinkedNode"]:
        # Check for impl methods that match on the variant and return Path/&Path
        impl_match = re.search(
            rf"impl Locator\s*\{{(.*?)\}}",
            source,
            re.DOTALL,
        )
        if impl_match:
            impl_body = impl_match.group(1)
            # Look for methods that return PathBuf or &Path
            for method_match in re.finditer(
                r"fn\s+\w+\(.*?\)\s*->\s*(?:&?\s*Path(?:Buf)?|.*Path(?:Buf)?)\s*\{",
                impl_body,
            ):
                method_text = impl_body[method_match.start():method_match.end() + 200]
                # Check if this method matches on Ssh or LinkedNode to return a path
                if f"Locator::{variant}" in method_text and (
                    "canonical_root" in method_text or "as_path" in method_text
                ):
                    print(f"  FAIL: Locator::{variant} has a method returning a path")
                    return False

    return True


def check_no_unwrap_or_default_pathbuf() -> bool:
    """Check for unwrap_or_default() followed by PathBuf::from on remote locator fields.

    The anti-pattern is coercing an optional remote string (ssh_path,
    linked_node_path_hint, path_hint) into a PathBuf via
    unwrap_or_default(). Local canonical_root is legitimate since it
    comes from a workspace record and is a real filesystem path.
    """
    source = _read(CATALOG_MODULE)

    # Only flag PathBuf::from applied to remote locator field names
    remote_field_patterns = [
        re.compile(r"ssh_path\.map\(PathBuf::from\)\.unwrap_or_default\(\)"),
        re.compile(r"linked_node_path_hint\.map\(PathBuf::from\)\.unwrap_or_default\(\)"),
        re.compile(r"path_hint\.map\(PathBuf::from\)\.unwrap_or_default\(\)"),
        re.compile(r"ssh_path\.unwrap_or_default\(\).*PathBuf::from"),
        re.compile(r"linked_node_path_hint\.unwrap_or_default\(\).*PathBuf::from"),
        re.compile(r"path_hint\.unwrap_or_default\(\).*PathBuf::from"),
        # Broader: any .map(PathBuf::from) on an ssh/linked field
        re.compile(r"ssh_\w+\.map\(PathBuf::from\)"),
        re.compile(r"linked_node_\w+\.map\(PathBuf::from\)"),
    ]

    for pat in remote_field_patterns:
        if pat.search(source):
            print(f"  FAIL: anti-pattern found: {pat.pattern}")
            return False
    return True


def check_catalog_migration_tables() -> bool:
    """Verify catalog and discovery migrations create their tables."""
    source = _read(SCHEMA_MODULE)

    tables = [
        "project_locator",
        "project_health",
        "legacy_catalog_association_marker",
        "discovery_root",
        "discovery_scan",
        "discovery_observation",
    ]

    for table in tables:
        if f"CREATE TABLE IF NOT EXISTS {table}" not in source:
            print(f"  FAIL: v28 migration missing CREATE TABLE for {table}")
            return False
    return True


def check_catalog_migration_columns() -> bool:
    """Verify v28 migration adds the 5 new columns to logical_project."""
    source = _read(SCHEMA_MODULE)

    columns = [
        "ALTER TABLE logical_project ADD COLUMN archived_at INTEGER",
        "ALTER TABLE logical_project ADD COLUMN description TEXT",
        "ALTER TABLE logical_project ADD COLUMN tags TEXT",
        "ALTER TABLE logical_project ADD COLUMN registration_source TEXT",
        "ALTER TABLE logical_project ADD COLUMN time_last_opened INTEGER",
    ]

    for col in columns:
        if col not in source:
            print(f"  FAIL: v28 migration missing: {col}")
            return False
    return True


def check_storage_layout_version() -> bool:
    """Verify STORAGE_LAYOUT_VERSION is 29."""
    source = _read(STORAGE_MODULE)
    match = re.search(r"STORAGE_LAYOUT_VERSION\s*:\s*u32\s*=\s*(\d+)", source)
    if not match:
        print("  FAIL: STORAGE_LAYOUT_VERSION not found")
        return False
    version = int(match.group(1))
    if version != 29:
        print(f"  FAIL: STORAGE_LAYOUT_VERSION is {version}, expected 29")
        return False
    return True


def check_lib_reexport() -> bool:
    """Verify lib.rs has pub mod project_catalog."""
    source = _read(LIB_MODULE)
    if "pub mod project_catalog" not in source:
        print("  FAIL: lib.rs missing 'pub mod project_catalog'")
        return False
    return True


def main() -> int:
    verbose = "--verbose" in sys.argv or "-v" in sys.argv
    checks: list[tuple[str, callable]] = [
        ("Catalog module file exists", check_catalog_module_exists),
        (
            "attach_locator only binds workspace for Local variant",
            check_attach_locator_local_only,
        ),
        ("No unwrap_or_default PathBuf anti-pattern", check_no_unwrap_or_default_pathbuf),
        ("catalog/discovery migrations create tables", check_catalog_migration_tables),
        ("v28 migration adds 5 columns to logical_project", check_catalog_migration_columns),
        ("STORAGE_LAYOUT_VERSION is 29", check_storage_layout_version),
        ("lib.rs re-exports project_catalog", check_lib_reexport),
    ]

    results: list[tuple[str, bool]] = []
    for name, check_fn in checks:
        if verbose:
            print(f"CHECK: {name} ... ", end="", flush=True)
        ok = check_fn()
        if verbose:
            print("PASS" if ok else "FAIL")
        results.append((name, ok))

    print()
    passed = sum(1 for _, ok in results if ok)
    failed = sum(1 for _, ok in results if not ok)

    for name, ok in results:
        status = "PASS" if ok else "FAIL"
        print(f"  [{status}] {name}")

    print(f"\n{passed}/{len(results)} checks passed.")
    if failed:
        print(f"{failed} check(s) FAILED.")
        return 1

    print("All project catalog invariants verified.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

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


def _parse_storage_layout_version(path: Path) -> int:
    """Parse the canonical layout marker from the storage module."""
    source = path.read_text(encoding="utf-8")
    match = re.search(r"STORAGE_LAYOUT_VERSION\s*:\s*u32\s*=\s*(\d+)", source)
    if not match:
        raise ValueError(f"STORAGE_LAYOUT_VERSION not found in {path}")
    return int(match.group(1))


def _highest_wired_migration(path: Path) -> int:
    """Derive the highest migration wired into the canonical schema path.

    Three independent wirings must agree: the sequential `migrate()` upgrade
    chain (`migrate_and_record(pool, N)`), the `migrate_and_record` dispatch
    arms (`N => migrate_vN`), and the `migrate_vN` function definitions.
    Returns the shared maximum; raises `ValueError` on any disagreement so
    a half-wired migration fails the guard instead of silently passing.
    """
    source = path.read_text(encoding="utf-8")
    chain = {int(v) for v in re.findall(r"migrate_and_record\(pool,\s*(\d+)\)", source)}
    arms = {
        (int(key), int(func))
        for key, func in re.findall(r"(\d+)\s*=>\s*migrate_v(\d+)", source)
    }
    defined = {int(v) for v in re.findall(r"async fn migrate_v(\d+)\s*\(", source)}
    if not chain:
        raise ValueError(f"no migrate_and_record chain found in {path}")
    if not arms:
        raise ValueError(f"no migrate_v dispatch arms found in {path}")
    if not defined:
        raise ValueError(f"no migrate_v definitions found in {path}")
    mismatched_arms = sorted(key for key, func in arms if key != func)
    if mismatched_arms:
        raise ValueError(f"dispatch arm(s) {mismatched_arms} do not map to migrate_vN")
    arm_keys = {key for key, _ in arms}
    if chain != arm_keys or arm_keys != defined:
        raise ValueError(
            "migration wiring disagrees: upgrade chain covers "
            f"{sorted(chain)}, dispatch arms cover {sorted(arm_keys)}, "
            f"definitions cover {sorted(defined)}"
        )
    highest = max(chain)
    expected = set(range(1, highest + 1))
    if chain != expected:
        raise ValueError(
            f"migration chain is not contiguous 1..={highest}: "
            f"missing {sorted(expected - chain)}"
        )
    return highest


def _self_test() -> int:
    """Exercise the layout/migration relationship against synthetic sources.

    Proves the guard is future-proof (a matched future-number pair passes
    without editing the guard) and still sensitive (any mismatch fails).
    Uses only the standard library and temporary files.
    """
    import tempfile

    failures = 0

    def write_pair(tmp: Path, layout: int, wired: int) -> tuple[Path, Path]:
        storage = tmp / "storage_mod.rs"
        storage.write_text(
            f"pub const STORAGE_LAYOUT_VERSION: u32 = {layout};\n", encoding="utf-8"
        )
        arms = "\n".join(f"            {v} => migrate_v{v}(&mut tx).await?,"
                         for v in range(1, wired + 1))
        chain = "\n".join(f"    if current_version < {v} {{\n"
                          f"        migrate_and_record(pool, {v}).await?;\n    }}"
                          for v in range(1, wired + 1))
        defns = "\n".join(f"async fn migrate_v{v}(tx: &mut Tx) -> Result<()> {{ Ok(()) }}"
                          for v in range(1, wired + 1))
        schema = tmp / "schema.rs"
        schema.write_text(f"{chain}\nmatch version {{\n{arms}\n}}\n{defns}\n",
                          encoding="utf-8")
        return storage, schema

    cases = [
        # (layout, wired, should_pass, description)
        (56, 56, True, "current matched pair passes"),
        (99, 99, True, "matched future-number pair passes without guard edits"),
        (57, 56, False, "layout bumped without migration fails"),
        (56, 57, False, "migration wired without layout bump fails"),
    ]
    for layout, wired, should_pass, description in cases:
        with tempfile.TemporaryDirectory() as raw:
            tmp = Path(raw)
            storage, schema = write_pair(tmp, layout, wired)
            try:
                ok = _parse_storage_layout_version(storage) == _highest_wired_migration(
                    schema
                )
            except ValueError:
                ok = False
            passed = ok == should_pass
            print(f"  [{'PASS' if passed else 'FAIL'}] self-test: {description}")
            if not passed:
                failures += 1

    if failures:
        print(f"{failures} self-test case(s) FAILED.")
        return 1
    print("All guard self-test cases passed.")
    return 0


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


def check_storage_layout_tracks_wired_migrations() -> bool:
    """Verify the storage layout marker tracks the wired schema migration path.

    The canonical layout marker (`STORAGE_LAYOUT_VERSION` in
    `crates/codegg-core/src/storage/mod.rs`) must equal the highest schema
    migration actually wired into the canonical migration path in
    `crates/codegg-core/src/session/schema.rs`. The executable contract for
    this relationship also lives in `tests/storage_migrations.rs`, which
    asserts a fully migrated database reports exactly
    `STORAGE_LAYOUT_VERSION`.

    This compares two independently derived values, so a future
    storage-layout increment paired with its migration passes without
    editing this guard, while bumping one side without the other fails.
    No volatile expected version number is embedded here.
    """
    try:
        layout_version = _parse_storage_layout_version(STORAGE_MODULE)
    except ValueError as exc:
        print(f"  FAIL: {exc}")
        return False
    try:
        highest = _highest_wired_migration(SCHEMA_MODULE)
    except ValueError as exc:
        print(f"  FAIL: {exc}")
        return False
    if layout_version != highest:
        print(
            "  FAIL: STORAGE_LAYOUT_VERSION "
            f"({layout_version}) does not match highest wired schema "
            f"migration ({highest}); bump the layout marker together with "
            "its migration, or vice versa"
        )
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
    if "--self-test" in sys.argv:
        return _self_test()
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
        (
            "storage layout marker tracks highest wired schema migration",
            check_storage_layout_tracks_wired_migrations,
        ),
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

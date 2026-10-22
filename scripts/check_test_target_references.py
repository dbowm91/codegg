#!/usr/bin/env python3
"""Static guard: every named test target must exist, and the recorded count must be true.

Three consumers name a root integration-test target by name:

- `.config/nextest.toml` — the `ci` profile's whole-binary exclusions,
  which reserve the test runner for a binary that must not share its slot;
- `.github/workflows/ci.yml` — the `-E 'not binary(...)'` filter that skips
  the live-Eggwork binary when the change does not touch it;
- `docs/test-ownership.toml` — the ownership manifest, checked separately by
  `scripts/check_test_ownership.py`.

The M004 family consolidation renamed most of those targets. A filter that
names a target which no longer exists is **not** a harmless no-op: it either
silently stops reserving the runner for a binary that needs it, or it silently
stops excluding a binary that must be excluded. This guard makes that loud.

It also checks the recorded integration-test-file count in
`architecture/overview.md`'s "Verified counts" table, which is the single
source of truth for counts in this repository, against the tree.

The measure this guard computes, and which the table row must name:

    root integration test modules = `tests/*.rs` + `tests/*/*.inc`
    (the `tests/common/` helper tree is excluded — it is shared helpers,
    not test modules)

Run with `--self-test` to prove the guard rejects an injected violation.

Exit code 0 if all checks pass, 1 if any fail.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import tempfile

import tomllib

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
NEXTEST_CONFIG = REPO_ROOT / ".config" / "nextest.toml"
CI_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "ci.yml"
OVERVIEW = REPO_ROOT / "architecture" / "overview.md"

# `binary(<name>)` inside a nextest filter expression or a CI `-E` filter.
BINARY_REFERENCE = re.compile(r"\bbinary\(\s*([A-Za-z0-9_.-]+)\s*\)")

# The "Verified counts" table row this guard keeps honest.
COUNT_ROW = re.compile(r"^\|\s*Integration test files\s*\|\s*(\d+)\s*\|", re.M)


def workspace_test_targets() -> dict[str, set[str]]:
    """Every test target in the workspace, by package."""
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(f"cargo metadata failed: {result.stderr.strip()}")
    metadata = json.loads(result.stdout)
    targets: dict[str, set[str]] = {}
    for package in metadata["packages"]:
        names = {t["name"] for t in package["targets"] if t["kind"] == ["test"]}
        names.add(package["name"].replace("-", "_"))
        targets[package["name"]] = names
    return targets


def all_target_names(targets: dict[str, set[str]]) -> set[str]:
    return {name for names in targets.values() for name in names}


def root_integration_test_modules() -> int:
    """The measured count of root integration test modules."""
    tests = REPO_ROOT / "tests"
    count = sum(1 for _ in tests.glob("*.rs"))
    for family in tests.iterdir():
        if not family.is_dir() or family.name == "common":
            continue
        count += sum(1 for _ in family.glob("*.inc"))
    return count


def referenced_binaries(text: str) -> set[str]:
    return set(BINARY_REFERENCE.findall(text))


def check_nextest_filters(targets: set[str], path: Path = NEXTEST_CONFIG) -> list[str]:
    violations: list[str] = []
    if not path.is_file():
        return [f"{path} not found"]
    text = path.read_text(encoding="utf-8")
    for name in sorted(referenced_binaries(text)):
        if name not in targets:
            violations.append(
                f".config/nextest.toml names binary({name}), which is not a test target "
                f"in the workspace. The filter would silently stop reserving the runner "
                f"for that binary."
            )
    return violations


def check_ci_filters(targets: set[str], path: Path = CI_WORKFLOW) -> list[str]:
    violations: list[str] = []
    if not path.is_file():
        return [f"{path} not found"]
    text = path.read_text(encoding="utf-8")
    for name in sorted(referenced_binaries(text)):
        if name not in targets:
            violations.append(
                f".github/workflows/ci.yml names binary({name}), which is not a test "
                f"target in the workspace. The filter would silently stop excluding "
                f"that binary."
            )
    return violations


def check_recorded_count(path: Path = OVERVIEW) -> list[str]:
    violations: list[str] = []
    if not path.is_file():
        return [f"{path} not found"]
    text = path.read_text(encoding="utf-8")
    match = COUNT_ROW.search(text)
    if not match:
        return [
            "architecture/overview.md has no '| Integration test files | N |' row in its "
            "Verified counts table. The recorded count is the single source of truth "
            "for counts, so the row must exist for this guard to check it."
        ]
    recorded = int(match.group(1))
    measured = root_integration_test_modules()
    if recorded != measured:
        violations.append(
            f"architecture/overview.md records {recorded} integration test files, but "
            f"the tree holds {measured} root integration test modules "
            f"(`tests/*.rs` + `tests/*/*.inc`, excluding `tests/common/`). "
            f"Re-measure and update the row, or change the measure and the guard "
            f"together."
        )
    return violations


def check() -> bool:
    try:
        targets = all_target_names(workspace_test_targets())
    except RuntimeError as error:
        print(f"  FAIL: {error}")
        return False
    violations = check_nextest_filters(targets) + check_ci_filters(targets) + check_recorded_count()
    for message in violations:
        print(f"  FAIL: {message}")
    return not violations


def _self_test() -> int:
    """Prove the guard rejects each violation class."""
    targets = all_target_names(workspace_test_targets())
    original_nextest = NEXTEST_CONFIG.read_text(encoding="utf-8")
    original_ci = CI_WORKFLOW.read_text(encoding="utf-8")
    original_overview = OVERVIEW.read_text(encoding="utf-8")

    staged_nextest = NEXTEST_CONFIG.with_name("nextest.toml.self-test")
    staged_ci = CI_WORKFLOW.with_name("ci.yml.self-test")
    staged_overview = OVERVIEW.with_name("overview.md.self-test")
    try:
        # Violation 1: a nextest filter naming a target that does not exist.
        staged_nextest.write_text(
            original_nextest.replace(
                'filter = "binary(eggwork_remote_execution_live)',
                'filter = "binary(definitely_not_a_test_target)',
                1,
            ),
            encoding="utf-8",
        )
        try:
            violations = [
                v
                for v in check_nextest_filters(targets, staged_nextest)
                if "definitely_not_a_test_target" in v
            ]
            if violations:
                print("  ok: injected a nextest filter naming a nonexistent target rejected")
            else:
                print("  FAIL: self-test injected a bad nextest filter and it passed")
                return 1
        finally:
            staged_nextest.unlink()

        # Violation 2: a CI `-E` filter naming a target that does not exist.
        staged_ci.write_text(
            original_ci.replace(
                "binary(eggwork_remote_execution_live)",
                "binary(definitely_not_a_test_target)",
                1,
            ),
            encoding="utf-8",
        )
        try:
            violations = [
                v
                for v in check_ci_filters(targets, staged_ci)
                if "definitely_not_a_test_target" in v
            ]
            if violations:
                print("  ok: injected a CI filter naming a nonexistent target rejected")
            else:
                print("  FAIL: self-test injected a bad CI filter and it passed")
                return 1
        finally:
            staged_ci.unlink()

        # Violation 3: a recorded count that no longer matches the tree.
        staged_overview.write_text(
            original_overview.replace(
                COUNT_ROW.search(original_overview).group(0),
                "| Integration test files | 99999 |",
                1,
            ),
            encoding="utf-8",
        )
        try:
            violations = check_recorded_count(staged_overview)
            if violations:
                print("  ok: injected a stale recorded count rejected")
            else:
                print("  FAIL: self-test injected a stale count and it passed")
                return 1
        finally:
            staged_overview.unlink()

        # And the unmodified tree must still pass.
        violations = (
            check_nextest_filters(targets)
            + check_ci_filters(targets)
            + check_recorded_count()
        )
        if violations:
            print("  FAIL: self-test passed but the real tree has violations")
            for violation in violations:
                print(f"    {violation}")
            return 1
        print("  ok: unmodified tree still passes")
        return 0
    except (OSError, AttributeError) as error:
        print(f"  FAIL: self-test could not stage a violation: {error}")
        return 1


def main() -> int:
    if "--self-test" in sys.argv:
        print("check_test_target_references --self-test")
        return _self_test()

    checks = [
        (
            "every named test target exists and the recorded count is true",
            check,
        ),
    ]
    failed = []
    for name, check_fn in checks:
        try:
            passed = check_fn()
        except OSError as error:
            print(f"  FAIL: {name}: {error}")
            passed = False
        print(f"  {'ok' if passed else 'FAIL'}: {name}")
        if not passed:
            failed.append(name)
    if failed:
        print(f"check_test_target_references: {len(failed)} failing")
        return 1
    print("check_test_target_references: all checks pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())

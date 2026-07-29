#!/usr/bin/env python3
"""Baseline-aware regression guard for bare #[tokio::test] annotations.

Ensures all new tokio tests specify an explicit runtime flavor. Historical
bare tests are tracked in a checked-in baseline file.

Exit codes:
  0 — no new violations and no stale baseline entries
  1 — new bare tests found, stale entries, or malformed baseline

Usage:
    python3 scripts/check-tokio-test-flavors.py [--self-test] [--emit-current]
"""

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

DEFAULT_BASELINE = REPO_ROOT / "scripts" / "tokio-test-flavor-baseline.txt"

SKIP_PATHS = {
    "target",
    ".git",
    "node_modules",
    "examples",
}

# Match a bare #[tokio::test] without arguments (end of line)
BARE_TOKIO_TEST_RE = re.compile(r"#\s*\[\s*tokio::test\s*\]$")

# Match a tokio::test with explicit flavor (NOT bare)
FLAVORED_TOKIO_TEST_RE = re.compile(r"#\s*\[\s*tokio::test\s*\(")

# Match a function definition
FN_RE = re.compile(r"^\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)")


def find_rust_files(root: Path) -> list[Path]:
    files = []
    for path in root.rglob("*.rs"):
        parts = path.relative_to(root).parts
        if any(part in SKIP_PATHS for part in parts):
            continue
        files.append(path)
    return sorted(files)


def extract_bare_test_identities(root: Path) -> list[str]:
    """Find all bare #[tokio::test] and return stable identities."""
    identities = []
    for filepath in find_rust_files(root):
        content = filepath.read_text(errors="replace")
        lines = content.split("\n")
        rel_path = str(filepath.relative_to(root))

        i = 0
        while i < len(lines):
            stripped = lines[i].strip()
            if BARE_TOKIO_TEST_RE.search(stripped):
                # Look ahead for the function name, skipping #[cfg(...)] and other attrs
                fn_line = i + 1
                while fn_line < len(lines):
                    fn_stripped = lines[fn_line].strip()
                    # Skip attribute lines, doc comments, and empty lines
                    if fn_stripped.startswith("#[") or fn_stripped.startswith("//") or fn_stripped == "":
                        fn_line += 1
                        continue
                    m = FN_RE.match(fn_stripped)
                    if m:
                        identities.append(f"{rel_path}::{m.group(1)}")
                    else:
                        # Malformed: bare tokio::test not followed by fn
                        identities.append(f"{rel_path}::???UNRESOLVED_LINE_{i+1}")
                    break
                else:
                    # EOF without function
                    identities.append(f"{rel_path}::???UNRESOLVED_EOF_{i+1}")
            i += 1
    return sorted(identities)


def load_baseline(path: Path) -> tuple[list[str], list[str]]:
    """Load baseline and return (entries, errors)."""
    if not path.exists():
        return [], [f"baseline file not found: {path}"]

    entries = []
    errors = []
    seen = set()

    for lineno, raw in enumerate(path.read_text().splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue

        # Reject wildcards and directory suppressions
        if "*" in line or line.endswith("/"):
            errors.append(f"  line {lineno}: wildcard or directory suppression not allowed: {line}")
            continue

        # Validate format: must be path::function or path::???UNRESOLVED_*
        if "::" not in line:
            errors.append(f"  line {lineno}: missing '::' separator: {line}")
            continue

        if line in seen:
            errors.append(f"  line {lineno}: duplicate entry: {line}")
            continue

        seen.add(line)
        entries.append(line)

    return sorted(entries), errors


def run_self_test() -> int:
    """Validate the script can detect both bare and non-bare patterns."""
    test_cases = [
        # (description, source_lines, expected_bare_identities)
        (
            "bare #[tokio::test] alone",
            ["#[tokio::test]", "async fn test1() {}"],
            ["test.rs::test1"],
        ),
        (
            "bare after blank lines",
            ["", "", "#[tokio::test]", "async fn test2() {}"],
            ["test.rs::test2"],
        ),
        (
            "bare after #[cfg(test)]",
            ["#[cfg(test)]", "#[tokio::test]", "async fn test3() {}"],
            ["test.rs::test3"],
        ),
        (
            "bare after #[cfg(all(...))]",
            ["#[cfg(all(test, feature = \"x\"))]", "#[tokio::test]", "async fn test4() {}"],
            ["test.rs::test4"],
        ),
        (
            "flavored current_thread passes",
            ["#[tokio::test(flavor = \"current_thread\")]", "async fn test5() {}"],
            [],
        ),
        (
            "flavored multi_thread passes",
            ["#[tokio::test(flavor = \"multi_thread\", worker_threads = 2)]", "async fn test6() {}"],
            [],
        ),
        (
            "bare after flavored (only bare detected)",
            [
                "#[tokio::test(flavor = \"current_thread\")]",
                "async fn test7() {}",
                "#[tokio::test]",
                "async fn test8() {}",
            ],
            ["test.rs::test8"],
        ),
        (
            "multiple bare tests",
            ["#[tokio::test]", "async fn test9() {}", "#[tokio::test]", "async fn test10() {}"],
            ["test.rs::test9", "test.rs::test10"],
        ),
        (
            "bare with extra whitespace",
            ["#[ tokio::test ]", "async fn test11() {}"],
            ["test.rs::test11"],
        ),
        (
            "not a test attribute (#[tokio::main])",
            ["#[tokio::main]", "async fn test12() {}"],
            [],
        ),
    ]

    passed = 0
    failed = 0

    print("Running self-test...\n")

    for desc, source_lines, expected in test_cases:
        actual = []
        lines = source_lines
        for idx, line in enumerate(lines):
            if BARE_TOKIO_TEST_RE.search(line.strip()):
                fn_idx = idx + 1
                while fn_idx < len(lines):
                    fs = lines[fn_idx].strip()
                    if fs.startswith("#[") or fs.startswith("//") or fs == "":
                        fn_idx += 1
                        continue
                    m = FN_RE.match(fs)
                    if m:
                        actual.append(f"test.rs::{m.group(1)}")
                    break

        if actual == expected:
            print(f"  PASS: {desc}")
            passed += 1
        else:
            print(f"  FAIL: {desc}")
            print(f"    Expected: {expected}")
            print(f"    Got:      {actual}")
            failed += 1

    # Test baseline loading
    print("\n  Baseline validation tests:")

    import tempfile

    for desc, content, expect_ok, expect_count in [
        ("valid baseline", "a::b\nc::d\n", True, 2),
        ("duplicate entry", "a::b\na::b\n", False, 0),
        ("wildcard entry", "a::*\n", False, 0),
        ("missing separator", "abc\n", False, 0),
        ("comment lines", "# comment\na::b\n", True, 1),
        ("empty lines", "\n\na::b\n\n", True, 1),
    ]:
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".txt", delete=False
        ) as f:
            f.write(content)
            temp_baseline = Path(f.name)

        try:
            entries, errors = load_baseline(temp_baseline)
            ok = len(errors) == 0
            if ok == expect_ok and (not expect_ok or len(entries) == expect_count):
                print(f"    PASS: {desc}")
                passed += 1
            else:
                print(f"    FAIL: {desc}")
                print(f"      Expected ok={expect_ok}, got ok={ok}")
                if expect_ok:
                    print(f"      Expected {expect_count} entries, got {len(entries)}")
                print(f"      Errors: {errors}")
                failed += 1
        finally:
            temp_baseline.unlink()

    print(f"\nSelf-test results: {passed} passed, {failed} failed")

    if failed > 0:
        print("\nSelf-test FAILED")
        return 1
    else:
        print("\nSelf-test PASSED")
        return 0


def main():
    parser = argparse.ArgumentParser(
        description="Baseline-aware regression guard for bare #[tokio::test]"
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run self-test to validate detection logic",
    )
    parser.add_argument(
        "--emit-current",
        action="store_true",
        help="Print sorted current bare identities (for baseline generation)",
    )
    parser.add_argument(
        "--baseline",
        type=Path,
        default=DEFAULT_BASELINE,
        help=f"Path to baseline file (default: {DEFAULT_BASELINE})",
    )
    args = parser.parse_args()

    if args.self_test:
        return run_self_test()

    # Find current bare identities
    current = extract_bare_test_identities(REPO_ROOT)

    if args.emit_current:
        for ident in current:
            print(ident)
        return 0

    # Load baseline
    baseline, baseline_errors = load_baseline(args.baseline)

    if baseline_errors:
        print("Baseline errors:")
        for err in baseline_errors:
            print(err)
        return 1

    current_set = set(current)
    baseline_set = set(baseline)

    new_violations = sorted(current_set - baseline_set)
    stale_baseline = sorted(baseline_set - current_set)

    if not new_violations and not stale_baseline:
        print(
            f"Tokio flavor guard: {len(current)} bare tests in baseline, no new violations."
        )
        return 0

    if new_violations:
        print(f"NEW bare #[tokio::test] violations ({len(new_violations)}):")
        for v in new_violations:
            print(f"  {v}")
        print()

    if stale_baseline:
        print(f"STALE baseline entries ({len(stale_baseline)}):")
        for s in stale_baseline:
            print(f"  {s}")
        print()

    if new_violations:
        print(
            "New bare tests must use explicit flavor:\n"
            '  #[tokio::test(flavor = "current_thread")]  — for most tests\n'
            '  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]  — for concurrency\n'
        )
    if stale_baseline:
        print(
            "Stale baseline entries must be removed from the baseline file.\n"
            "Converted tests no longer need baseline entries.\n"
        )

    return 1


if __name__ == "__main__":
    sys.exit(main())

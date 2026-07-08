#!/usr/bin/env python3
"""Check for bare #[tokio::test] annotations without explicit flavor.

This script is a regression guard that ensures all tokio tests specify an
explicit runtime flavor (`current_thread` or `multi_thread`). Bare
`#[tokio::test]` uses the default multi-threaded runtime, which is
unnecessary overhead for most tests.

Exit codes:
  0 — no bare tests found (or all are in allowlist)
  1 — bare tests found (CI failure)

Usage:
    python3 scripts/check-tokio-test-flavors.py [--allowlist FILE] [--self-test]
"""

import argparse
import re
import sys
import textwrap
from pathlib import Path

# Files that are allowed to have bare #[tokio::test] (one per line)
DEFAULT_ALLOWLIST = ""

SKIP_PATHS = {
    "target",
    ".git",
    "node_modules",
    "examples",
}

# Regex to match a bare #[tokio::test] without arguments.
# This matches the exact string with flexible whitespace.
BARE_TOKIO_TEST_RE = re.compile(r"#\s*\[\s*tokio::test\s*\]$")

# Regex to match a tokio::test with explicit flavor (NOT bare).
FLAVORED_TOKIO_TEST_RE = re.compile(r"#\s*\[\s*tokio::test\s*\(")

# Regex to match cfg attributes that may precede #[tokio::test]
CFG_LINE_RE = re.compile(r"^\s*#\[cfg\(")


def find_rust_files(root: Path) -> list[Path]:
    """Find all Rust source files, excluding build artifacts."""
    files = []
    for path in root.rglob("*.rs"):
        parts = path.relative_to(root).parts
        if any(part in SKIP_PATHS for part in parts):
            continue
        files.append(path)
    return sorted(files)


def check_file(filepath: Path, allowlist: set[str]) -> list[dict]:
    """Check a file for bare #[tokio::test] annotations.

    Handles multiline patterns where #[cfg(...)] or #[cfg(all(...))]
    may precede #[tokio::test]. Returns list of {line, line_number}
    for bare tests.
    """
    content = filepath.read_text(errors="replace")
    lines = content.split("\n")
    results = []

    rel_path = str(filepath)
    if rel_path in allowlist:
        return results

    i = 0
    while i < len(lines):
        line = lines[i]
        stripped = line.strip()

        # Check if this line is a bare #[tokio::test]
        if BARE_TOKIO_TEST_RE.search(stripped):
            # Look back to find preceding #[cfg(...)] lines
            # The bare tokio::test is the violation line
            results.append(
                {
                    "line": stripped,
                    "line_number": i + 1,
                }
            )
        elif FLAVORED_TOKIO_TEST_RE.search(stripped):
            # This has explicit flavor — skip
            pass

        i += 1

    return results


def load_allowlist(allowlist_path: Path | None) -> set[str]:
    """Load allowlist from file."""
    if allowlist_path is None:
        # Use default allowlist
        allowlist_text = DEFAULT_ALLOWLIST
    else:
        allowlist_text = allowlist_path.read_text()

    # Parse allowlist: one path per line, # for comments, blank lines ignored
    allowlist = set()
    for line in allowlist_text.split("\n"):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        allowlist.add(line)

    return allowlist


def run_self_test() -> int:
    """Validate the script can detect both bare and non-bare patterns.

    Returns 0 if all tests pass, 1 if any test fails.
    """
    test_cases = [
        # (description, source_lines, expected_violations)
        (
            "bare #[tokio::test] alone",
            ["#[tokio::test]", "async fn test1() {}"],
            [1],
        ),
        (
            "bare #[tokio::test] after blank lines",
            ["", "", "#[tokio::test]", "async fn test2() {}"],
            [3],
        ),
        (
            "bare #[tokio::test] after #[cfg(test)]",
            ["#[cfg(test)]", "#[tokio::test]", "async fn test3() {}"],
            [2],
        ),
        (
            "bare #[tokio::test] after #[cfg(all(...))]",
            ["#[cfg(all(test, feature = \"x\"))]", "#[tokio::test]", "async fn test4() {}"],
            [2],
        ),
        (
            "flavored #[tokio::test(flavor = \"current_thread\")]",
            ["#[tokio::test(flavor = \"current_thread\")]", "async fn test5() {}"],
            [],
        ),
        (
            "flavored #[tokio::test(flavor = \"multi_thread\", worker_threads = 2)]",
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
            [3],
        ),
        (
            "multiple bare tests",
            ["#[tokio::test]", "async fn test9() {}", "#[tokio::test]", "async fn test10() {}"],
            [1, 3],
        ),
        (
            "bare with extra whitespace",
            ["#[ tokio::test ]", "async fn test11() {}"],
            [1],
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

    for desc, source_lines, expected_violations in test_cases:
        # Write test content to temp file and check it
        import tempfile

        with tempfile.NamedTemporaryFile(mode="w", suffix=".rs", delete=False) as f:
            f.write("\n".join(source_lines))
            temp_path = Path(f.name)

        try:
            results = check_file(temp_path, set())
            actual_line_numbers = [r["line_number"] for r in results]

            if actual_line_numbers == expected_violations:
                print(f"  PASS: {desc}")
                passed += 1
            else:
                print(f"  FAIL: {desc}")
                print(f"    Expected: {expected_violations}")
                print(f"    Got:      {actual_line_numbers}")
                failed += 1
        finally:
            temp_path.unlink()

    print(f"\nSelf-test results: {passed} passed, {failed} failed")

    if failed > 0:
        print("\nSelf-test FAILED")
        return 1
    else:
        print("\nSelf-test PASSED")
        return 0


def main():
    parser = argparse.ArgumentParser(
        description="Check for bare #[tokio::test] annotations"
    )
    parser.add_argument(
        "--allowlist",
        type=Path,
        default=None,
        help="Path to allowlist file (one path per line)",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run self-test to validate detection logic",
    )
    parser.add_argument(
        "paths",
        nargs="*",
        default=["."],
        help="Paths to scan (default: current directory)",
    )
    args = parser.parse_args()

    if args.self_test:
        return run_self_test()

    root = Path.cwd()
    allowlist = load_allowlist(args.allowlist)
    violations = []

    for path_str in args.paths:
        path = Path(path_str)
        if not path.exists():
            continue

        if path.is_file() and path.suffix == ".rs":
            results = check_file(path, allowlist)
            for r in results:
                violations.append({"file": str(path), **r})
        elif path.is_dir():
            for filepath in find_rust_files(path):
                results = check_file(filepath, allowlist)
                for r in results:
                    violations.append({"file": str(filepath), **r})

    if violations:
        print(f"Found {len(violations)} bare #[tokio::test] annotation(s):\n")
        for v in violations:
            rel_path = (
                Path(v["file"]).relative_to(root)
                if Path(v["file"]).is_relative_to(root)
                else v["file"]
            )
            print(f"  {rel_path}:{v['line_number']}: {v['line']}")

        print(
            "\nAll tokio tests must specify an explicit flavor:"
        )
        print("  #[tokio::test(flavor = \"current_thread\")]  — for most tests")
        print("  #[tokio::test(flavor = \"multi_thread\", worker_threads = 2)]  — for concurrency")
        print(
            "\nAdd exceptions to scripts/check-tokio-test-flavors.py allowlist if needed."
        )
        return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())

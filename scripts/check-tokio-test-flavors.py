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
    python3 scripts/check-tokio-test-flavors.py [--allowlist FILE]
"""

import argparse
import re
import sys
from pathlib import Path

# Files that are allowed to have bare #[tokio::test] (one per line)
DEFAULT_ALLOWLIST = """
# Tests that legitimately use multi-thread runtime:
# - real_server_smoke.rs: spawns actual language server subprocesses
# - tests that use default multi-thread for compatibility verification
crates/egglsp/tests/real_server_smoke.rs
""".strip()

SKIP_PATHS = {
    "target",
    ".git",
    "node_modules",
    "examples",
}


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

    Returns list of {line, line_number} for bare tests.
    """
    content = filepath.read_text(errors="replace")
    lines = content.split("\n")
    results = []

    for i, line in enumerate(lines):
        stripped = line.strip()

        # Match bare #[tokio::test] without flavor
        if stripped == "#[tokio::test]":
            # Check if this file is in the allowlist
            rel_path = str(filepath)
            if rel_path in allowlist:
                continue
            results.append(
                {
                    "line": stripped,
                    "line_number": i + 1,
                }
            )

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
        "paths",
        nargs="*",
        default=["."],
        help="Paths to scan (default: current directory)",
    )
    args = parser.parse_args()

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
